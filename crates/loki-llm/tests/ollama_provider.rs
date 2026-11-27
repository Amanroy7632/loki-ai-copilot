//! P2-T01 / P2-T02: the Ollama adapter against mock HTTP servers (no real Ollama needed).

// Test helpers outside #[test] fns aren't covered by clippy.toml's allow-*-in-tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use futures::StreamExt;
use loki_core::{Capability, Risk, Role, ToolName, ToolSpec};
use loki_llm::{
    CancellationToken, ChatEvent, ChatMessage, ChatProvider, ChatRequest, LlmError, OllamaApi,
    OllamaProvider, ProviderHealth, Usage,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Tests build their own client; product code gets one from `loki-runtime::net`.
fn provider(base: &str) -> OllamaProvider {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    OllamaProvider::new(
        OllamaApi::new(client, base.parse().unwrap()),
        "qwen3:4b",
        4096,
    )
}

fn request() -> ChatRequest {
    ChatRequest {
        messages: vec![ChatMessage::new(Role::User, "hi")],
        tools: Vec::new(),
    }
}

fn ndjson(lines: &[serde_json::Value]) -> String {
    lines.iter().map(|l| format!("{l}\n")).collect()
}

async fn collect(
    provider: &OllamaProvider,
    request: ChatRequest,
) -> Vec<Result<ChatEvent, LlmError>> {
    provider
        .stream_chat(request, CancellationToken::new())
        .collect()
        .await
}

#[tokio::test]
async fn streams_tokens_then_done_and_sends_the_expected_body() {
    let server = MockServer::start().await;
    let body = ndjson(&[
        json!({ "message": { "role": "assistant", "content": "Hel" }, "done": false }),
        json!({ "message": { "role": "assistant", "content": "lo" }, "done": false }),
        json!({ "message": { "role": "assistant", "content": "" }, "done": true, "prompt_eval_count": 9, "eval_count": 2 }),
    ]);
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .and(body_partial_json(json!({
            "model": "qwen3:4b", "stream": true, "think": false, "options": { "num_ctx": 4096 },
            "messages": [{ "role": "user", "content": "hi" }]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .expect(1)
        .mount(&server)
        .await;

    let events = collect(&provider(&server.uri()), request()).await;
    assert_eq!(
        events,
        vec![
            Ok(ChatEvent::Token { text: "Hel".into() }),
            Ok(ChatEvent::Token { text: "lo".into() }),
            Ok(ChatEvent::Done(Usage {
                prompt_tokens: Some(9),
                completion_tokens: Some(2)
            })),
        ]
    );
}

#[tokio::test]
async fn tool_call_response_becomes_a_tool_call_event() {
    let server = MockServer::start().await;
    let body = ndjson(&[
        json!({ "message": { "role": "assistant", "content": "",
            "tool_calls": [{ "function": { "name": "time.now", "arguments": { "timezone": "Asia/Tokyo" } } }] }, "done": false }),
        json!({ "message": { "role": "assistant", "content": "" }, "done": true }),
    ]);
    Mock::given(path("/api/chat"))
        .and(body_partial_json(
            json!({ "tools": [{ "type": "function", "function": { "name": "time.now" } }] }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;
    let mut req = request();
    req.tools.push(ToolSpec {
        name: ToolName::new("time.now"),
        description: "Current time".into(),
        input_schema: json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        risk: Risk::ReadOnly,
        capability: Capability::Clock,
    });
    let events = collect(&provider(&server.uri()), req).await;
    assert_eq!(events.len(), 2);
    let Ok(ChatEvent::ToolCall(draft)) = &events[0] else {
        panic!("{events:?}")
    };
    assert_eq!(draft.tool.as_str(), "time.now");
    assert_eq!(draft.arguments["timezone"], "Asia/Tokyo");
    assert!(matches!(events[1], Ok(ChatEvent::Done(_))));
}

#[tokio::test]
async fn malformed_line_ends_the_stream_with_a_protocol_error() {
    let server = MockServer::start().await;
    let body = format!(
        "{}\n{{ not json\n{}\n",
        json!({ "message": { "content": "ok" }, "done": false }),
        json!({ "message": { "content": "never seen" }, "done": true })
    );
    Mock::given(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;
    let events = collect(&provider(&server.uri()), request()).await;
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[0], Ok(ChatEvent::Token { text: "ok".into() }));
    assert!(matches!(events[1], Err(LlmError::Protocol(_))));
}

#[tokio::test]
async fn stream_without_done_is_a_protocol_error() {
    let server = MockServer::start().await;
    let body = ndjson(&[json!({ "message": { "content": "cut off" }, "done": false })]);
    Mock::given(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;
    let events = collect(&provider(&server.uri()), request()).await;
    assert!(
        matches!(events.last(), Some(Err(LlmError::Protocol(_)))),
        "{events:?}"
    );
}

#[tokio::test]
async fn error_line_from_ollama_is_reported() {
    let server = MockServer::start().await;
    let body = ndjson(&[json!({ "error": "model runner has unexpectedly stopped" })]);
    Mock::given(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;
    let events = collect(&provider(&server.uri()), request()).await;
    assert_eq!(
        events,
        vec![Err(LlmError::Protocol(
            "model runner has unexpectedly stopped".into()
        ))]
    );
}

#[tokio::test]
async fn missing_model_is_a_typed_error() {
    let server = MockServer::start().await;
    Mock::given(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(404).set_body_string(r#"{"error":"model 'qwen3:4b' not found"}"#),
        )
        .mount(&server)
        .await;
    let events = collect(&provider(&server.uri()), request()).await;
    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0], Err(LlmError::ModelMissing(m)) if m.contains("qwen3:4b")),
        "{events:?}"
    );
}

#[tokio::test]
async fn server_down_is_unavailable() {
    // Bind and drop a listener to get a port with nothing behind it.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let provider = provider(&format!("http://127.0.0.1:{port}"));
    let events = collect(&provider, request()).await;
    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0], Err(LlmError::Unavailable(m)) if m.contains("not running")),
        "{events:?}"
    );
    assert!(matches!(
        provider.health().await,
        ProviderHealth::Unavailable { .. }
    ));
}

#[tokio::test]
async fn health_reports_ready_only_when_the_model_is_installed() {
    let server = MockServer::start().await;
    Mock::given(path("/api/version"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"version":"0.40.18"}"#))
        .mount(&server)
        .await;
    Mock::given(path("/api/tags"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"models":[{"name":"bonsai-2:4b","size":1,"digest":"x"}]}"#),
        )
        .mount(&server)
        .await;
    let missing = provider(&server.uri()).health().await;
    assert!(
        matches!(&missing, ProviderHealth::Unavailable { reason } if reason.contains("not installed")),
        "{missing:?}"
    );

    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let bonsai = OllamaProvider::new(
        OllamaApi::new(client, server.uri().parse().unwrap()),
        "bonsai-2:4b",
        4096,
    );
    assert_eq!(bonsai.health().await, ProviderHealth::Ready);
}

/// A raw HTTP server that sends one NDJSON line, then stalls for 30 s mid-body. wiremock can
/// only delay a whole response, so this is the only way to test cancelling *during* the body.
async fn stalling_server() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0_u8; 4096];
        let _ = socket.read(&mut request).await;
        let line = format!(
            "{}\n",
            json!({ "message": { "content": "first" }, "done": false })
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/x-ndjson\r\ntransfer-encoding: chunked\r\n\r\n{:x}\r\n{line}\r\n",
            line.len()
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.flush().await.unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    format!("http://{address}")
}

/// P2-T02 acceptance: the stream ends within 200 ms of cancel against a slow server.
#[tokio::test]
async fn cancel_ends_the_stream_within_200ms_mid_body() {
    let provider = provider(&stalling_server().await);
    let cancel = CancellationToken::new();
    let mut stream = provider.stream_chat(request(), cancel.clone());
    assert_eq!(
        stream.next().await,
        Some(Ok(ChatEvent::Token {
            text: "first".into()
        }))
    );

    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        trigger.cancel();
    });
    let waited = Instant::now();
    let next = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .unwrap();
    let after_cancel = waited.elapsed().saturating_sub(Duration::from_millis(50));
    assert_eq!(next, Some(Err(LlmError::Cancelled)));
    assert!(
        after_cancel < Duration::from_millis(200),
        "took {after_cancel:?} after cancel"
    );
    assert_eq!(stream.next().await, None);
}

#[tokio::test]
async fn cancel_while_waiting_for_headers_ends_promptly() {
    let server = MockServer::start().await;
    Mock::given(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)))
        .mount(&server)
        .await;
    let cancel = CancellationToken::new();
    let mut stream = provider(&server.uri()).stream_chat(request(), cancel.clone());
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        trigger.cancel();
    });
    let started = Instant::now();
    assert_eq!(stream.next().await, Some(Err(LlmError::Cancelled)));
    assert!(started.elapsed() < Duration::from_millis(250 + 50));
}

#[tokio::test]
async fn pull_streams_progress_until_success() {
    let server = MockServer::start().await;
    let body = ndjson(&[
        json!({ "status": "pulling manifest" }),
        json!({ "status": "pulling abc", "digest": "sha256:abc", "total": 100, "completed": 50 }),
        json!({ "status": "verifying sha256 digest" }),
        json!({ "status": "success" }),
    ]);
    Mock::given(path("/api/pull"))
        .and(body_partial_json(
            json!({ "model": "qwen3:4b", "stream": true }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let api = OllamaApi::new(client, server.uri().parse().unwrap());
    let progress: Vec<_> = api
        .pull("qwen3:4b", CancellationToken::new())
        .collect()
        .await;
    assert_eq!(progress.len(), 4);
    assert_eq!(progress[1].as_ref().unwrap().completed, Some(50));
    assert_eq!(progress[3].as_ref().unwrap().status, "success");
}

#[tokio::test]
async fn think_flag_is_sent_when_enabled() {
    let server = MockServer::start().await;
    let body = ndjson(&[
        json!({ "message": { "role": "assistant", "content": "", "thinking": "private reasoning" }, "done": false }),
        json!({ "message": { "role": "assistant", "content": "Clean answer." }, "done": false }),
        json!({ "message": { "role": "assistant", "content": "" }, "done": true }),
    ]);
    Mock::given(path("/api/chat"))
        .and(body_partial_json(json!({ "think": true })))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .expect(1)
        .mount(&server)
        .await;
    let events = collect(&provider(&server.uri()).with_think(true), request()).await;
    assert_eq!(
        events[0],
        Ok(ChatEvent::Token {
            text: "Clean answer.".into()
        }),
        "thinking never surfaces"
    );
    assert_eq!(events.len(), 2);
}
