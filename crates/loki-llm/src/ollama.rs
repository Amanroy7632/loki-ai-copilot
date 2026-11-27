//! Ollama adapter (P2-T01, P2-T02): `/api/chat` streaming, tool-call events, cancellation,
//! health, and the model-management calls onboarding needs (`/api/tags`, `/api/pull`).
//!
//! The HTTP client is **injected**: `loki-runtime::net` builds it with the network policy
//! (loopback only, no proxy, loopback-only redirects). This module never builds a client.
//!
//! Request defaults, from the P0-T03 spike (`docs/p0/inference.md`):
//! - `think` is off by default (fast first token) and set per model from the reviewed allowlist
//!   (ADR-0010): some models leak reasoning into the reply unless thinking is on. Any `thinking`
//!   text Ollama returns separately is dropped, never shown or stored.
//! - `options.num_ctx` set explicitly, so memory use is predictable and `context_limit()` is true.

use std::collections::VecDeque;
use std::pin::Pin;

use futures::FutureExt;
use futures::future::BoxFuture;
use futures::stream::{self, BoxStream, Stream, StreamExt};
use loki_core::{ToolName, ToolSpec};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::error::LlmError;
use crate::provider::{
    ChatEvent, ChatMessage, ChatProvider, ChatRequest, ChatStream, ProviderHealth, ToolCallDraft,
    Usage,
};

/// The longest NDJSON line accepted before the stream is treated as broken (1 MiB).
const MAX_LINE_BYTES: usize = 1 << 20;

// ------------------------------------------------------------------------------- low-level API

/// Thin typed access to the Ollama HTTP API, shared by the provider and model onboarding.
#[derive(Debug, Clone)]
pub struct OllamaApi {
    client: reqwest::Client,
    base: Url,
}

/// One installed model, from `/api/tags`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InstalledModel {
    /// Full name with tag, e.g. `qwen3:4b`.
    pub name: String,
    /// Size on disk in bytes.
    #[serde(default)]
    pub size: u64,
    /// Content digest (sha256), as reported by Ollama.
    #[serde(default)]
    pub digest: String,
}

/// One progress update from `/api/pull`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PullProgress {
    /// e.g. `pulling manifest`, `pulling <digest>`, `verifying sha256 digest`, `success`.
    pub status: String,
    /// Layer digest being downloaded, if any.
    #[serde(default)]
    pub digest: Option<String>,
    /// Layer size in bytes, if known.
    #[serde(default)]
    pub total: Option<u64>,
    /// Bytes of the layer downloaded so far.
    #[serde(default)]
    pub completed: Option<u64>,
}

impl OllamaApi {
    /// Wraps a policy-checked client (from `loki-runtime::net`) and the server's base URL.
    pub fn new(client: reqwest::Client, base: Url) -> Self {
        Self { client, base }
    }

    /// The server's base URL, for messages.
    pub fn base_url(&self) -> &Url {
        &self.base
    }

    fn url(&self, path: &str) -> Result<Url, LlmError> {
        self.base
            .join(path)
            .map_err(|error| LlmError::Protocol(error.to_string()))
    }

    fn not_running(&self) -> LlmError {
        LlmError::Unavailable(format!(
            "Ollama is not running at {}. Start it with scripts/ollama-serve.sh",
            self.base
        ))
    }

    /// `/api/version`. Fails with `Unavailable` if the server is down.
    pub async fn version(&self) -> Result<String, LlmError> {
        let response = self
            .client
            .get(self.url("/api/version")?)
            .send()
            .await
            .map_err(|_| self.not_running())?;
        let body: Value = response
            .json()
            .await
            .map_err(|error| LlmError::Protocol(error.to_string()))?;
        body["version"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| LlmError::Protocol(String::from("/api/version has no version")))
    }

    /// `/api/tags`: installed models.
    pub async fn installed_models(&self) -> Result<Vec<InstalledModel>, LlmError> {
        #[derive(Deserialize)]
        struct Tags {
            models: Vec<InstalledModel>,
        }
        let response = self
            .client
            .get(self.url("/api/tags")?)
            .send()
            .await
            .map_err(|_| self.not_running())?;
        let tags: Tags = response
            .json()
            .await
            .map_err(|error| LlmError::Protocol(error.to_string()))?;
        Ok(tags.models)
    }

    /// Whether `model` is installed. `qwen3` matches `qwen3:latest`.
    pub async fn has_model(&self, model: &str) -> Result<bool, LlmError> {
        let wanted = normalize_model_name(model);
        Ok(self
            .installed_models()
            .await?
            .iter()
            .any(|installed| normalize_model_name(&installed.name) == wanted))
    }

    /// `/api/pull`, streamed. Ollama downloads each layer and verifies its sha256 itself.
    /// Cancelling the token drops the connection and ends the stream with `Err(Cancelled)`.
    pub fn pull(
        &self,
        model: &str,
        cancel: CancellationToken,
    ) -> BoxStream<'static, Result<PullProgress, LlmError>> {
        let request = self.url("/api/pull").map(|url| {
            self.client
                .post(url)
                .json(&json!({ "model": model, "stream": true }))
        });
        ndjson_stream(
            request,
            cancel,
            self.not_running(),
            model.to_owned(),
            pull_line,
            |item| matches!(item, Ok(progress) if progress.status == "success"),
        )
        .boxed()
    }
}

fn pull_line(line: Value) -> Vec<Result<PullProgress, LlmError>> {
    if let Some(error) = line.get("error") {
        return vec![Err(LlmError::Protocol(error_text(error)))];
    }
    vec![
        serde_json::from_value::<PullProgress>(line)
            .map_err(|error| LlmError::Protocol(error.to_string())),
    ]
}

/// `name` -> `name:latest`, so untagged and tagged names compare equal.
fn normalize_model_name(name: &str) -> String {
    if name.contains(':') {
        name.to_owned()
    } else {
        format!("{name}:latest")
    }
}

fn error_text(error: &Value) -> String {
    error
        .as_str()
        .map_or_else(|| error.to_string(), str::to_owned)
}

// ------------------------------------------------------------------------------- NDJSON reader

type Body = Pin<Box<dyn Stream<Item = reqwest::Result<Vec<u8>>> + Send>>;

enum Phase {
    // Boxed: a RequestBuilder is far larger than the other variants.
    Start(Box<Result<reqwest::RequestBuilder, LlmError>>),
    Reading(Body),
    Finished,
}

struct Reader<T, H, F> {
    phase: Phase,
    cancel: CancellationToken,
    buffer: Vec<u8>,
    pending: VecDeque<Result<T, LlmError>>,
    handler: H,
    is_terminal: F,
    terminal_seen: bool,
    ended: bool,
    not_running: LlmError,
    model: String,
}

/// Streams an NDJSON response through `handler` (one JSON line -> zero or more items).
///
/// Guarantees the [`ChatStream`] protocol shape: the stream ends right after the first item
/// for which `is_terminal` is true or the first `Err`. If the body ends before that, the last
/// item is `Err(Protocol)`. Cancellation drops the connection and yields `Err(Cancelled)`.
fn ndjson_stream<T, H, F>(
    request: Result<reqwest::RequestBuilder, LlmError>,
    cancel: CancellationToken,
    not_running: LlmError,
    model: String,
    handler: H,
    is_terminal: F,
) -> impl Stream<Item = Result<T, LlmError>> + Send + 'static
where
    T: Send + 'static,
    H: FnMut(Value) -> Vec<Result<T, LlmError>> + Send + 'static,
    F: Fn(&Result<T, LlmError>) -> bool + Send + 'static,
{
    let reader = Reader {
        phase: Phase::Start(Box::new(request)),
        cancel,
        buffer: Vec::new(),
        pending: VecDeque::new(),
        handler,
        is_terminal,
        terminal_seen: false,
        ended: false,
        not_running,
        model,
    };
    stream::unfold(reader, |mut reader| async move {
        let item = reader.next_item().await?;
        Some((item, reader))
    })
}

impl<T, H, F> Reader<T, H, F>
where
    H: FnMut(Value) -> Vec<Result<T, LlmError>>,
    F: Fn(&Result<T, LlmError>) -> bool,
{
    async fn next_item(&mut self) -> Option<Result<T, LlmError>> {
        loop {
            if self.ended {
                return None;
            }
            if let Some(item) = self.pending.pop_front() {
                if item.is_err() || (self.is_terminal)(&item) {
                    self.finish();
                }
                return Some(item);
            }
            if self.cancel.is_cancelled() {
                self.finish();
                return Some(Err(LlmError::Cancelled));
            }
            match std::mem::replace(&mut self.phase, Phase::Finished) {
                Phase::Finished => {
                    self.ended = true;
                    return None;
                }
                Phase::Start(request) => self.start(*request).await,
                Phase::Reading(body) => self.read(body).await,
            }
        }
    }

    /// Ends the stream after the current item and drops the connection.
    fn finish(&mut self) {
        self.ended = true;
        self.terminal_seen = true;
        self.pending.clear();
        self.phase = Phase::Finished;
    }

    async fn start(&mut self, request: Result<reqwest::RequestBuilder, LlmError>) {
        let request = match request {
            Ok(request) => request,
            Err(error) => return self.pending.push_back(Err(error)),
        };
        let response = tokio::select! {
            biased;
            () = self.cancel.cancelled() => return self.pending.push_back(Err(LlmError::Cancelled)),
            response = request.send() => response,
        };
        let response = match response {
            Ok(response) => response,
            Err(_) => return self.pending.push_back(Err(self.not_running.clone())),
        };
        let status = response.status();
        if status.is_success() {
            let body = response
                .bytes_stream()
                .map(|chunk| chunk.map(|bytes| bytes.to_vec()));
            self.phase = Phase::Reading(Box::pin(body));
            return;
        }
        let body = response.text().await.unwrap_or_default();
        let detail = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|value| value.get("error").map(error_text))
            .unwrap_or(body);
        self.pending
            .push_back(Err(if status == StatusCode::NOT_FOUND {
                LlmError::ModelMissing(format!("{} ({detail})", self.model))
            } else {
                LlmError::Protocol(format!("HTTP {status}: {detail}"))
            }));
    }

    async fn read(&mut self, mut body: Body) {
        let next = tokio::select! {
            biased;
            // Returning drops `body`, which closes the connection, and Ollama aborts the request.
            () = self.cancel.cancelled() => return self.pending.push_back(Err(LlmError::Cancelled)),
            next = body.next() => next,
        };
        match next {
            None => {
                if !self.terminal_seen {
                    self.pending.push_back(Err(LlmError::Protocol(String::from(
                        if self.buffer.iter().all(u8::is_ascii_whitespace) {
                            "stream ended before completion"
                        } else {
                            "stream ended in the middle of a line"
                        },
                    ))));
                }
            }
            Some(Err(_)) => self
                .pending
                .push_back(Err(LlmError::Unavailable(String::from(
                    "connection to Ollama was lost mid-stream",
                )))),
            Some(Ok(bytes)) => {
                self.buffer.extend_from_slice(&bytes);
                self.drain_lines();
                if self.buffer.len() > MAX_LINE_BYTES {
                    self.pending.push_back(Err(LlmError::Protocol(String::from(
                        "response line too long",
                    ))));
                }
                self.phase = Phase::Reading(body);
            }
        }
    }

    fn drain_lines(&mut self) {
        while let Some(newline) = self.buffer.iter().position(|&byte| byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=newline).collect();
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            match serde_json::from_slice::<Value>(&line) {
                Ok(value) => self.pending.extend((self.handler)(value)),
                Err(_) => {
                    self.pending.push_back(Err(LlmError::Protocol(String::from(
                        "malformed JSON line from Ollama",
                    ))));
                    return;
                }
            }
        }
    }
}

// ------------------------------------------------------------------------------- provider

/// [`ChatProvider`] backed by a local Ollama server.
#[derive(Debug, Clone)]
pub struct OllamaProvider {
    api: OllamaApi,
    model: String,
    num_ctx: u32,
    think: bool,
}

impl OllamaProvider {
    /// `model` is an Ollama tag such as `qwen3:4b`; `num_ctx` is the context window to request.
    pub fn new(api: OllamaApi, model: impl Into<String>, num_ctx: u32) -> Self {
        Self {
            api,
            model: model.into(),
            num_ctx,
            think: false,
        }
    }

    /// Sets Ollama's `think` flag. With `true`, the model's reasoning arrives in a separate
    /// `thinking` field, which this adapter drops; the reply text stays clean. Needed for models
    /// that leak reasoning into the reply when thinking is off (qwen3 on Ollama 0.40.18, ADR-0010).
    #[must_use]
    pub fn with_think(mut self, think: bool) -> Self {
        self.think = think;
        self
    }

    /// The model this provider talks to.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The underlying API, for onboarding commands.
    pub fn api(&self) -> &OllamaApi {
        &self.api
    }

    /// The exact JSON body sent to `/api/chat`. Public for tests and debugging.
    pub fn request_body(&self, request: &ChatRequest) -> Value {
        let messages: Vec<Value> = request.messages.iter().map(message_json).collect();
        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
            "think": self.think,
            "options": { "num_ctx": self.num_ctx },
        });
        if !request.tools.is_empty() {
            body["tools"] = Value::Array(request.tools.iter().map(tool_json).collect());
        }
        body
    }
}

fn message_json(message: &ChatMessage) -> Value {
    json!({ "role": message.role.as_str(), "content": message.content })
}

/// Loki `ToolSpec` -> Ollama/OpenAI function tool. The dotted name is passed through unchanged;
/// the P0-T03 spike measured dotted (`time.now`) against underscore names.
fn tool_json(spec: &ToolSpec) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": spec.name.as_str(),
            "description": spec.description,
            "parameters": spec.input_schema,
        }
    })
}

/// Maps one `/api/chat` NDJSON line to events. `done: true` produces the terminal `Done`.
fn chat_line(line: Value) -> Vec<Result<ChatEvent, LlmError>> {
    if let Some(error) = line.get("error") {
        return vec![Err(LlmError::Protocol(error_text(error)))];
    }
    let mut events = Vec::new();
    let message = &line["message"];
    if let Some(text) = message["content"].as_str().filter(|text| !text.is_empty()) {
        events.push(Ok(ChatEvent::Token {
            text: text.to_owned(),
        }));
    }
    for call in message["tool_calls"].as_array().into_iter().flatten() {
        let function = &call["function"];
        match function["name"].as_str().filter(|name| !name.is_empty()) {
            Some(name) => events.push(Ok(ChatEvent::ToolCall(ToolCallDraft {
                tool: ToolName::new(name),
                arguments: function["arguments"].clone(),
            }))),
            None => {
                events.push(Err(LlmError::Protocol(String::from(
                    "tool call without a function name",
                ))));
                return events;
            }
        }
    }
    if line["done"].as_bool() == Some(true) {
        let count = |key: &str| line[key].as_u64().and_then(|n| u32::try_from(n).ok());
        events.push(Ok(ChatEvent::Done(Usage {
            prompt_tokens: count("prompt_eval_count"),
            completion_tokens: count("eval_count"),
        })));
    }
    events
}

impl ChatProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn stream_chat(&self, request: ChatRequest, cancel: CancellationToken) -> ChatStream {
        let body = self.request_body(&request);
        let http = self
            .api
            .url("/api/chat")
            .map(|url| self.api.client.post(url).json(&body));
        ndjson_stream(
            http,
            cancel,
            self.api.not_running(),
            self.model.clone(),
            chat_line,
            |item| matches!(item, Ok(ChatEvent::Done(_))),
        )
        .boxed()
    }

    fn health(&self) -> BoxFuture<'_, ProviderHealth> {
        async move {
            if let Err(error) = self.api.version().await {
                return ProviderHealth::Unavailable {
                    reason: error.to_string(),
                };
            }
            match self.api.has_model(&self.model).await {
                Ok(true) => ProviderHealth::Ready,
                Ok(false) => ProviderHealth::Unavailable {
                    reason: format!(
                        "model '{}' is not installed. Run `loki models pull {}`",
                        self.model, self.model
                    ),
                },
                Err(error) => ProviderHealth::Unavailable {
                    reason: error.to_string(),
                },
            }
        }
        .boxed()
    }

    fn context_limit(&self) -> u32 {
        self.num_ctx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untagged_names_match_latest() {
        assert_eq!(normalize_model_name("qwen3"), "qwen3:latest");
        assert_eq!(normalize_model_name("qwen3:4b"), "qwen3:4b");
    }

    #[test]
    fn chat_line_maps_content_tool_calls_and_done() {
        let events = chat_line(json!({
            "message": { "role": "assistant", "content": "hi",
                         "tool_calls": [{ "function": { "name": "time.now", "arguments": { "timezone": "Asia/Tokyo" } } }] },
            "done": true, "prompt_eval_count": 12, "eval_count": 3
        }));
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[0], Ok(ChatEvent::Token { text }) if text == "hi"));
        assert!(matches!(&events[1], Ok(ChatEvent::ToolCall(d)) if d.tool.as_str() == "time.now"));
        assert!(matches!(
            &events[2],
            Ok(ChatEvent::Done(Usage {
                prompt_tokens: Some(12),
                completion_tokens: Some(3)
            }))
        ));
    }

    #[test]
    fn thinking_text_is_dropped() {
        let events = chat_line(
            json!({ "message": { "role": "assistant", "content": "", "thinking": "secret plan" }, "done": false }),
        );
        assert!(events.is_empty());
    }

    #[test]
    fn nameless_tool_call_is_a_protocol_error() {
        let events = chat_line(
            json!({ "message": { "tool_calls": [{ "function": { "arguments": {} } }] }, "done": false }),
        );
        assert!(matches!(events.last(), Some(Err(LlmError::Protocol(_)))));
    }
}
