//! Integration tests for `MockProvider` and the `ChatStream` protocol (P1-T04).

// Test helpers outside #[test] fns aren't covered by clippy.toml's allow-*-in-tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use loki_core::Role;
use loki_llm::{
    CancellationToken, ChatEvent, ChatMessage, ChatProvider, ChatRequest, LlmError, MockProvider,
    MockScript, MockStep, ProviderHealth, Usage,
};

fn fixture(name: &str) -> MockScript {
    let path = format!(
        "{}/../../fixtures/mock_provider/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let json = std::fs::read_to_string(&path).unwrap();
    MockScript::from_json(&json).unwrap()
}

fn request(text: &str) -> ChatRequest {
    ChatRequest {
        messages: vec![ChatMessage::new(Role::User, text)],
        tools: Vec::new(),
    }
}

async fn collect(
    provider: &dyn ChatProvider,
    request: ChatRequest,
) -> Vec<Result<ChatEvent, LlmError>> {
    provider
        .stream_chat(request, CancellationToken::new())
        .collect()
        .await
}

fn token(text: &str) -> Result<ChatEvent, LlmError> {
    Ok(ChatEvent::Token { text: text.into() })
}

#[tokio::test]
async fn mock_streams_tokens_in_order_then_done() {
    let provider = MockProvider::scripted(fixture("hello"));
    let events = collect(&provider, request("hi")).await;
    assert_eq!(
        events,
        vec![
            token("Hello"),
            token(", "),
            token("world"),
            Ok(ChatEvent::Done(Usage {
                prompt_tokens: Some(3),
                completion_tokens: Some(3)
            })),
        ]
    );
}

#[tokio::test]
async fn mock_emits_tool_call_proposal() {
    let provider = MockProvider::scripted(fixture("tool_call"));
    let events = collect(&provider, request("what time is it?")).await;
    assert_eq!(events.len(), 3);
    match &events[1] {
        Ok(ChatEvent::ToolCall(draft)) => {
            assert_eq!(draft.tool.as_str(), "time.now");
            assert_eq!(draft.arguments["timezone"], "Europe/Berlin");
        }
        other => panic!("expected a tool call, got {other:?}"),
    }
    assert!(matches!(events[2], Ok(ChatEvent::Done(_))));
}

#[tokio::test]
async fn mock_error_is_propagated_as_terminal_item() {
    let provider = MockProvider::scripted(fixture("error_midstream"));
    let events = collect(&provider, request("hi")).await;
    assert_eq!(
        events,
        vec![
            token("Partial"),
            Err(LlmError::Unavailable("model server stopped".into()))
        ]
    );
}

#[tokio::test]
async fn mock_cancellation_mid_stream_ends_promptly() {
    let provider = MockProvider::scripted(fixture("slow"));
    let cancel = CancellationToken::new();
    let mut stream = provider.stream_chat(request("hi"), cancel.clone());

    assert_eq!(stream.next().await, Some(token("first")));

    // The next step is a 30 s delay. Cancel from another task shortly after it starts.
    let canceller = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        canceller.cancel();
    });

    let started = Instant::now();
    let next = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("stream did not react to cancellation");
    assert_eq!(next, Some(Err(LlmError::Cancelled)));
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(
        stream.next().await,
        None,
        "nothing may follow a terminal item"
    );
}

#[tokio::test]
async fn mock_cancelled_before_first_poll_yields_only_cancelled() {
    let provider = MockProvider::scripted(fixture("hello"));
    let cancel = CancellationToken::new();
    cancel.cancel();
    let events: Vec<_> = provider.stream_chat(request("hi"), cancel).collect().await;
    assert_eq!(events, vec![Err(LlmError::Cancelled)]);
}

#[tokio::test]
async fn mock_cancellation_interrupts_slow_token_delay() {
    let provider = MockProvider::echo().with_token_delay(Duration::from_secs(30));
    let cancel = CancellationToken::new();
    let mut stream = provider.stream_chat(request("hi"), cancel.clone());
    cancel.cancel();
    let next = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("stream did not react to cancellation");
    assert_eq!(next, Some(Err(LlmError::Cancelled)));
    assert_eq!(stream.next().await, None);
}

#[tokio::test]
async fn mock_echo_replies_to_last_user_message() {
    let provider = MockProvider::echo();
    let request = ChatRequest {
        messages: vec![
            ChatMessage::new(Role::System, "be brief"),
            ChatMessage::new(Role::User, "first"),
            ChatMessage::new(Role::Assistant, "ok"),
            ChatMessage::new(Role::User, "second question"),
        ],
        tools: Vec::new(),
    };
    let events = collect(&provider, request).await;
    let text: String = events
        .iter()
        .filter_map(|event| match event {
            Ok(ChatEvent::Token { text }) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Mock reply: second question");
    assert_eq!(text, loki_llm::mock::echo_reply(Some("second question")));
    assert!(matches!(events.last(), Some(Ok(ChatEvent::Done(_)))));
}

#[tokio::test]
async fn mock_provider_is_object_safe_and_reports_health() {
    let provider: Arc<dyn ChatProvider> = Arc::new(MockProvider::echo().with_health(
        ProviderHealth::Unavailable {
            reason: "test".into(),
        },
    ));
    assert_eq!(provider.name(), "mock");
    assert!(provider.context_limit() > 0);
    assert_eq!(
        provider.health().await,
        ProviderHealth::Unavailable {
            reason: "test".into()
        }
    );
}

#[test]
fn mock_script_rejects_protocol_violations() {
    // No terminal step.
    assert!(MockScript::new(vec![MockStep::Token { text: "x".into() }]).is_err());
    // Steps after a terminal step.
    assert!(
        MockScript::new(vec![
            MockStep::Done {
                prompt_tokens: None,
                completion_tokens: None
            },
            MockStep::Token { text: "x".into() },
        ])
        .is_err()
    );
    // Empty.
    assert!(MockScript::new(Vec::new()).is_err());
}

#[test]
fn mock_script_rejects_unknown_fields_and_step_types() {
    assert!(MockScript::from_json(r#"{"steps":[{"type":"done"}],"extra":1}"#).is_err());
    assert!(
        MockScript::from_json(r#"{"steps":[{"type":"shell","cmd":"ls"},{"type":"done"}]}"#)
            .is_err()
    );
    assert!(
        MockScript::from_json(r#"{"steps":[{"type":"token","text":"a","x":1},{"type":"done"}]}"#)
            .is_err()
    );
}

#[test]
fn every_fixture_parses() {
    for name in ["hello", "tool_call", "error_midstream", "slow"] {
        let script = fixture(name);
        assert!(!script.steps().is_empty(), "{name}");
    }
}
