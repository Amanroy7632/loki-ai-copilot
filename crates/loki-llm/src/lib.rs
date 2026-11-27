//! # loki-llm
//!
//! Model provider trait plus mock, Ollama and (later) embedded llama.cpp adapters.
//!
//! **Status:** P1-T04 implemented the `ChatProvider` trait and [`MockProvider`]. Next tickets:
//! P2-T01..T03 (Ollama, cancellation), P2-T05 (onboarding support).
//!
//! ## Responsibilities
//!
//! - Define the `ChatProvider` trait: streaming chat, tool-call events, cancellation, health and context limits.
//! - Provide `MockProvider` (scripted and deterministic) for tests and the CLI.
//! - Provide the Ollama HTTP adapter (P2) and the embedded llama.cpp adapter (later).
//! - Keep provider-specific prompt templates and tool-call parsing inside each adapter.
//!
//! ## Must not
//!
//! - Fall back to any cloud provider, silently or otherwise.
//! - Make network calls except through the runtime's `NetworkPolicy`-checked HTTP client.
//! - Execute tools. It only reports tool-call *proposals*.
//!
//! ## Allowed internal dependencies
//!
//! `loki-core`. Adding another edge needs an ADR (see `docs/ARCHITECTURE.md`).

pub mod error;
pub mod mock;
pub mod provider;

pub use error::LlmError;
pub use mock::{MockErrorKind, MockProvider, MockScript, MockScriptError, MockStep};
pub use provider::{
    ChatEvent, ChatMessage, ChatProvider, ChatRequest, ChatStream, ProviderHealth, ToolCallDraft,
    Usage,
};
/// Re-exported so callers don't need their own `tokio-util` dependency to cancel a stream.
pub use tokio_util::sync::CancellationToken;
