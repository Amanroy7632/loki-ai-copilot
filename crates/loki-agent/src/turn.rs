//! Consuming one model reply stream (P1: plain chat, no tools yet).
//!
//! This is the seed of the P2-T04 chat loop: it enforces the [`ChatStream`] protocol and turns
//! the stream into a single outcome. Persistence stays with the caller (`loki-runtime`).

use futures::StreamExt;
use loki_llm::{
    CancellationToken, ChatEvent, ChatProvider, ChatRequest, LlmError, ToolCallDraft, Usage,
};

/// How one reply ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnOutcome {
    /// The model finished normally.
    Completed {
        /// The full reply text.
        text: String,
        /// Token accounting from the provider.
        usage: Usage,
        /// Tool calls the model proposed, in order. Nothing has run: the orchestrator decides.
        tool_calls: Vec<ToolCallDraft>,
    },
    /// The user cancelled. `partial` holds whatever text arrived first.
    Cancelled {
        /// Text received before cancellation.
        partial: String,
    },
}

/// Streams one reply from `provider`, calling `on_token` for each text fragment as it arrives.
///
/// Returns `Err` for provider failures, including a stream that ends without a terminal item
/// (a protocol violation). Cancellation is not an error: it returns [`TurnOutcome::Cancelled`].
pub async fn stream_reply(
    provider: &dyn ChatProvider,
    request: ChatRequest,
    cancel: CancellationToken,
    mut on_token: impl FnMut(&str),
) -> Result<TurnOutcome, LlmError> {
    let mut stream = provider.stream_chat(request, cancel);
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(ChatEvent::Token { text: fragment }) => {
                on_token(&fragment);
                text.push_str(&fragment);
            }
            Ok(ChatEvent::ToolCall(draft)) => {
                tracing::debug!(tool = %draft.tool, "tool call proposed");
                tool_calls.push(draft);
            }
            Ok(ChatEvent::Done(usage)) => {
                return Ok(TurnOutcome::Completed {
                    text,
                    usage,
                    tool_calls,
                });
            }
            Err(LlmError::Cancelled) => return Ok(TurnOutcome::Cancelled { partial: text }),
            Err(error) => return Err(error),
        }
    }
    Err(LlmError::Protocol(String::from(
        "stream ended without done or error",
    )))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use loki_core::Role;
    use loki_llm::{ChatMessage, MockProvider, MockScript, MockStep};
    use serde_json::json;

    use super::*;

    fn request() -> ChatRequest {
        ChatRequest {
            messages: vec![ChatMessage::new(Role::User, "hello there")],
            tools: Vec::new(),
        }
    }

    #[tokio::test]
    async fn completed_reply_collects_all_tokens_in_order() {
        let provider = MockProvider::echo();
        let mut seen = Vec::new();
        let outcome = stream_reply(&provider, request(), CancellationToken::new(), |t| {
            seen.push(t.to_owned());
        })
        .await
        .unwrap();
        let TurnOutcome::Completed { text, .. } = outcome else {
            panic!("expected completion");
        };
        assert_eq!(text, "Mock reply: hello there");
        assert_eq!(seen.concat(), text);
    }

    #[tokio::test]
    async fn cancellation_returns_partial_text() {
        let provider = MockProvider::echo().with_token_delay(Duration::from_secs(30));
        let cancel = CancellationToken::new();
        cancel.cancel();
        let outcome = stream_reply(&provider, request(), cancel, |_| {})
            .await
            .unwrap();
        assert_eq!(
            outcome,
            TurnOutcome::Cancelled {
                partial: String::new()
            }
        );
    }

    #[tokio::test]
    async fn provider_error_is_returned() {
        let script = MockScript::new(vec![
            MockStep::Token { text: "a".into() },
            MockStep::Error {
                error: loki_llm::MockErrorKind::Unavailable,
                message: "down".into(),
            },
        ])
        .unwrap();
        let error = stream_reply(
            &MockProvider::scripted(script),
            request(),
            CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap_err();
        assert_eq!(error, LlmError::Unavailable("down".into()));
    }

    #[tokio::test]
    async fn tool_call_proposals_are_returned_not_executed() {
        let script = MockScript::new(vec![
            MockStep::ToolCall {
                tool: "app.open".into(),
                arguments: json!({ "app_id": "calculator" }),
            },
            MockStep::Token { text: "ok".into() },
            MockStep::Done {
                prompt_tokens: None,
                completion_tokens: None,
            },
        ])
        .unwrap();
        let outcome = stream_reply(
            &MockProvider::scripted(script),
            request(),
            CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        let TurnOutcome::Completed { tool_calls, .. } = outcome else {
            panic!()
        };
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].tool.as_str(), "app.open");
    }
}
