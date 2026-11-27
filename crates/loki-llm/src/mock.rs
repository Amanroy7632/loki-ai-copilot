//! `MockProvider`: a deterministic, scripted [`ChatProvider`] for tests and the P1 CLI.
//!
//! **This is a mock.** It never runs a model. The runtime only selects it when the config says
//! `provider = "mock"`, and the CLI labels its replies as mock output.
//!
//! Scripts are JSON (see `fixtures/mock_provider/`):
//!
//! ```json
//! { "steps": [
//!     { "type": "token", "text": "Hello" },
//!     { "type": "delay", "ms": 50 },
//!     { "type": "tool_call", "tool": "time.now", "arguments": {} },
//!     { "type": "done" }
//! ] }
//! ```

use std::collections::VecDeque;
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use futures::stream::{self, StreamExt};
use loki_core::{Role, ToolName};
use serde::Deserialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::error::LlmError;
use crate::provider::{
    ChatEvent, ChatProvider, ChatRequest, ChatStream, ProviderHealth, ToolCallDraft, Usage,
};

/// Context window the mock claims to have. Arbitrary but realistic for small local models.
const MOCK_CONTEXT_LIMIT: u32 = 8_192;

/// One scripted step.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MockStep {
    /// Emit `ChatEvent::Token`.
    Token {
        /// The fragment to emit.
        text: String,
    },
    /// Emit `ChatEvent::ToolCall`.
    ToolCall {
        /// Proposed tool name.
        tool: String,
        /// Proposed arguments, passed through untouched (they may be deliberately invalid).
        arguments: Value,
    },
    /// Wait before the next step. Cancellation interrupts the wait.
    Delay {
        /// How long to wait, in milliseconds.
        ms: u64,
    },
    /// Emit `ChatEvent::Done` and end the stream.
    Done {
        /// Reported prompt tokens.
        #[serde(default)]
        prompt_tokens: Option<u32>,
        /// Reported completion tokens.
        #[serde(default)]
        completion_tokens: Option<u32>,
    },
    /// End the stream with this error.
    Error {
        /// Which error to emit.
        error: MockErrorKind,
        /// The error message.
        message: String,
    },
}

/// Errors a script can emit. `Cancelled` is deliberately absent: only real cancellation produces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MockErrorKind {
    /// Emit [`LlmError::Unavailable`].
    Unavailable,
    /// Emit [`LlmError::Protocol`].
    Protocol,
    /// Emit [`LlmError::ContextLimit`].
    ContextLimit,
}

/// A validated script: a non-empty list of steps ending in exactly one `done` or `error`.
#[derive(Debug, Clone, PartialEq)]
pub struct MockScript {
    steps: Vec<MockStep>,
}

/// Why a script was rejected.
#[derive(Debug, thiserror::Error)]
pub enum MockScriptError {
    /// Not valid JSON for the script format (including unknown fields or step types).
    #[error("invalid mock script JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The script breaks the stream protocol (see [`crate::ChatStream`]).
    #[error("invalid mock script: {0}")]
    Protocol(&'static str),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScript {
    steps: Vec<MockStep>,
}

impl MockScript {
    /// Builds a script from steps, enforcing the stream protocol so a test can't accidentally
    /// script something a real provider would never do.
    pub fn new(steps: Vec<MockStep>) -> Result<Self, MockScriptError> {
        let terminal =
            |step: &MockStep| matches!(step, MockStep::Done { .. } | MockStep::Error { .. });
        match steps.iter().position(terminal) {
            None => Err(MockScriptError::Protocol(
                "script must end with a `done` or `error` step",
            )),
            Some(index) if index + 1 != steps.len() => Err(MockScriptError::Protocol(
                "no steps may follow `done` or `error`",
            )),
            Some(_) => Ok(Self { steps }),
        }
    }

    /// Parses and validates a JSON script.
    pub fn from_json(json: &str) -> Result<Self, MockScriptError> {
        let raw: RawScript = serde_json::from_str(json)?;
        Self::new(raw.steps)
    }

    /// The validated steps.
    pub fn steps(&self) -> &[MockStep] {
        &self.steps
    }
}

#[derive(Debug, Clone)]
enum Behaviour {
    /// Replays the same script for every request.
    Scripted(MockScript),
    /// Replies with a fixed, clearly labelled echo of the last user message.
    Echo,
}

/// Scripted, deterministic provider. See the module docs.
#[derive(Debug, Clone)]
pub struct MockProvider {
    behaviour: Behaviour,
    token_delay: Duration,
    health: ProviderHealth,
}

impl MockProvider {
    /// Replays `script` for every request.
    pub fn scripted(script: MockScript) -> Self {
        Self {
            behaviour: Behaviour::Scripted(script),
            token_delay: Duration::ZERO,
            health: ProviderHealth::Ready,
        }
    }

    /// Replies `Mock reply: <last user message>`, one word per token. Used by the P1 CLI.
    pub fn echo() -> Self {
        Self {
            behaviour: Behaviour::Echo,
            token_delay: Duration::ZERO,
            health: ProviderHealth::Ready,
        }
    }

    /// Waits `delay` before every token, to simulate a slow model (and give Ctrl+C time to act).
    #[must_use]
    pub fn with_token_delay(mut self, delay: Duration) -> Self {
        self.token_delay = delay;
        self
    }

    /// Makes [`ChatProvider::health`] report `health`.
    #[must_use]
    pub fn with_health(mut self, health: ProviderHealth) -> Self {
        self.health = health;
        self
    }

    fn steps_for(&self, request: &ChatRequest) -> Vec<MockStep> {
        match &self.behaviour {
            Behaviour::Scripted(script) => script.steps.clone(),
            Behaviour::Echo => echo_steps(request),
        }
    }
}

/// Text the echo mock replies with. Public so tests can assert on it without duplicating it.
pub fn echo_reply(last_user_message: Option<&str>) -> String {
    match last_user_message {
        Some(text) if !text.trim().is_empty() => format!("Mock reply: {}", text.trim()),
        _ => String::from("Mock reply: (no user message)"),
    }
}

fn echo_steps(request: &ChatRequest) -> Vec<MockStep> {
    let last_user = request
        .messages
        .iter()
        .rev()
        .find(|message| message.role == Role::User)
        .map(|message| message.content.as_str());
    let reply = echo_reply(last_user);
    let words: Vec<&str> = reply.split(' ').collect();
    let count = words.len();
    let mut steps: Vec<MockStep> = words
        .into_iter()
        .enumerate()
        .map(|(index, word)| MockStep::Token {
            text: if index + 1 < count {
                format!("{word} ")
            } else {
                word.to_owned()
            },
        })
        .collect();
    steps.push(MockStep::Done {
        prompt_tokens: None,
        completion_tokens: u32::try_from(count).ok(),
    });
    steps
}

struct StreamState {
    steps: VecDeque<MockStep>,
    cancel: CancellationToken,
    token_delay: Duration,
    finished: bool,
}

/// Sleeps unless cancelled first. Returns `false` if cancelled.
async fn sleep_or_cancel(duration: Duration, cancel: &CancellationToken) -> bool {
    if duration.is_zero() {
        return !cancel.is_cancelled();
    }
    tokio::select! {
        biased;
        () = cancel.cancelled() => false,
        () = tokio::time::sleep(duration) => true,
    }
}

async fn next_item(mut state: StreamState) -> Option<(Result<ChatEvent, LlmError>, StreamState)> {
    if state.finished {
        return None;
    }
    loop {
        if state.cancel.is_cancelled() {
            state.finished = true;
            return Some((Err(LlmError::Cancelled), state));
        }
        let Some(step) = state.steps.pop_front() else {
            // Unreachable for validated scripts; end the stream rather than hang.
            state.finished = true;
            return Some((
                Err(LlmError::Protocol("mock script ended early".into())),
                state,
            ));
        };
        let item = match step {
            MockStep::Delay { ms } => {
                if sleep_or_cancel(Duration::from_millis(ms), &state.cancel).await {
                    continue;
                }
                state.finished = true;
                Err(LlmError::Cancelled)
            }
            MockStep::Token { text } => {
                if sleep_or_cancel(state.token_delay, &state.cancel).await {
                    Ok(ChatEvent::Token { text })
                } else {
                    state.finished = true;
                    Err(LlmError::Cancelled)
                }
            }
            MockStep::ToolCall { tool, arguments } => Ok(ChatEvent::ToolCall(ToolCallDraft {
                tool: ToolName::new(tool),
                arguments,
            })),
            MockStep::Done {
                prompt_tokens,
                completion_tokens,
            } => {
                state.finished = true;
                Ok(ChatEvent::Done(Usage {
                    prompt_tokens,
                    completion_tokens,
                }))
            }
            MockStep::Error { error, message } => {
                state.finished = true;
                Err(match error {
                    MockErrorKind::Unavailable => LlmError::Unavailable(message),
                    MockErrorKind::Protocol => LlmError::Protocol(message),
                    MockErrorKind::ContextLimit => LlmError::ContextLimit(message),
                })
            }
        };
        return Some((item, state));
    }
}

impl ChatProvider for MockProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn stream_chat(&self, request: ChatRequest, cancel: CancellationToken) -> ChatStream {
        let state = StreamState {
            steps: self.steps_for(&request).into(),
            cancel,
            token_delay: self.token_delay,
            finished: false,
        };
        stream::unfold(state, next_item).boxed()
    }

    fn health(&self) -> BoxFuture<'_, ProviderHealth> {
        let health = self.health.clone();
        async move { health }.boxed()
    }

    fn context_limit(&self) -> u32 {
        MOCK_CONTEXT_LIMIT
    }
}
