//! The provider contract: what every model backend (mock, Ollama, embedded) implements.
//!
//! See ADR-0006 for why the stream is boxed and errors travel as `Err` items.

use futures::future::BoxFuture;
use futures::stream::BoxStream;
use loki_core::{Role, ToolName, ToolSpec};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::error::LlmError;

/// The stream returned by [`ChatProvider::stream_chat`].
///
/// **Protocol** (every provider must follow it, and the agent relies on it):
/// - zero or more `Ok(Token)` / `Ok(ToolCall)` items, in generation order, then
/// - exactly one terminal item: `Ok(ChatEvent::Done(_))` or `Err(_)`, then
/// - the stream ends. Nothing is yielded after a terminal item.
///
/// Cancellation yields `Err(LlmError::Cancelled)` as the terminal item, promptly.
pub type ChatStream = BoxStream<'static, Result<ChatEvent, LlmError>>;

/// One message sent to the model: only what the provider needs, without storage IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatMessage {
    /// Who wrote it.
    pub role: Role,
    /// The text content.
    pub content: String,
}

impl ChatMessage {
    /// Convenience constructor used by context building and tests.
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}

/// Everything a provider needs for one generation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChatRequest {
    /// The bounded context, oldest first. Building it (and the token budget) is the agent's job.
    pub messages: Vec<ChatMessage>,
    /// Tools the model may propose calls to. Empty means plain chat.
    pub tools: Vec<ToolSpec>,
}

/// A tool call **proposed** by the model.
///
/// It is only a draft: the agent assigns the request ID, session and deadline to make a
/// [`loki_core::ToolRequest`], and then validation and policy decide whether it ever runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCallDraft {
    /// The tool the model asked for. It may not exist; the registry checks.
    pub tool: ToolName,
    /// Raw arguments from the model. Untrusted until validated against the tool schema.
    pub arguments: Value,
}

/// Token accounting reported at the end of a generation. Providers that can't count report `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    /// Tokens in the prompt, if known.
    #[serde(default)]
    pub prompt_tokens: Option<u32>,
    /// Tokens generated, if known.
    #[serde(default)]
    pub completion_tokens: Option<u32>,
}

/// One successful item of a [`ChatStream`]. Errors are `Err(LlmError)` items, not a variant here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEvent {
    /// A piece of response text. Concatenating all tokens gives the full reply.
    Token {
        /// The text fragment.
        text: String,
    },
    /// The model proposed a tool call.
    ToolCall(ToolCallDraft),
    /// Generation finished normally. Always the last successful item.
    Done(Usage),
}

/// Whether a provider can serve requests right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ProviderHealth {
    /// Reachable, and the configured model is available.
    Ready,
    /// Not usable. The reason is shown to the user (e.g. "Ollama is not running").
    Unavailable {
        /// Human-readable explanation.
        reason: String,
    },
}

/// A model backend.
///
/// Object-safe on purpose (`Box<dyn ChatProvider>` / `Arc<dyn ChatProvider>`), so the runtime
/// can choose the provider from config and inject it into the agent without generics.
pub trait ChatProvider: Send + Sync {
    /// A short stable name for logs and the UI, e.g. `mock` or `ollama`.
    fn name(&self) -> &str;

    /// Starts generating a reply. Must return immediately; the work happens as the stream is polled.
    ///
    /// Implementations must watch `cancel` and end the stream promptly with
    /// `Err(LlmError::Cancelled)` once it fires, including while waiting on the backend.
    fn stream_chat(&self, request: ChatRequest, cancel: CancellationToken) -> ChatStream;

    /// Checks whether the backend is usable. Never panics; failures become `Unavailable`.
    fn health(&self) -> BoxFuture<'_, ProviderHealth>;

    /// The model's context window in tokens, used by the agent's context budget.
    fn context_limit(&self) -> u32;
}
