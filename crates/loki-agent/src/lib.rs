//! # loki-agent
//!
//! Agent orchestration: context building and the model <-> tool loop.
//!
//! **Status:** P2-T03 (context budget), P2-T04 (orchestrator) and P3-T05 (tool loop) implemented.
//! Tickets that fill this crate in: P2-T04 (chat loop), P3-T05 (tool loop), P7-T04 (memory retrieval).
//!
//! ## Responsibilities
//!
//! - Run the turn lifecycle: normalize input, load bounded context, stream the model, handle tool calls, return the final answer.
//! - Enforce the context token budget, always keeping the system prompt and the latest user turn.
//! - Bound the tool loop (6 calls per turn initially).
//! - Feed tool results back to the model as data, wrapped and labelled as untrusted.
//!
//! ## Must not
//!
//! - Depend on concrete providers or platforms. Use traits only; the runtime injects implementations.
//! - Execute a tool without a policy decision.
//! - Contain UI or OS code.
//!
//! ## Allowed internal dependencies
//!
//! `loki-core`, `loki-llm`, `loki-tools`, `loki-policy`, `loki-memory`. Adding another edge needs an ADR (see `docs/ARCHITECTURE.md`).

pub mod context;
pub mod orchestrator;
pub mod tools;
pub mod turn;

pub use context::{BuiltContext, ContextBudget, build_context, estimate_tokens};
pub use orchestrator::{
    AgentError, ChatStore, DEFAULT_SYSTEM_PROMPT, IdSource, Orchestrator, StoreError,
    TOOL_LIMIT_NOTE, TOOLS_UNAVAILABLE_REPLY, TurnResult,
};
pub use tools::{
    AuditStore, DEFAULT_MAX_TOOL_CALLS, DEFAULT_TOOL_TIMEOUT, DenyAllApprovals, Dispatched,
    ToolEvent, ToolHost, ToolUi, wrap_untrusted,
};
pub use turn::{TurnOutcome, stream_reply};
