//! # loki-runtime
//!
//! Composition root: configuration, paths, job scheduling, lifecycle and dependency wiring.
//!
//! **Status:** P1-T06..T08 plus P2-T05 (model onboarding), P2-T06 (network policy) and the P2 wiring of the$
//! [`Runtime`] API are implemented. Next tickets: P2-T06 (network policy), P3-T04 (jobs),
//! P5-T02 (Tauri API).
//!
//! ## Responsibilities
//!
//! - Load the versioned configuration from the per-user app-data dir, migrating it atomically.
//! - Build concrete providers, platform adapters and repositories, and inject them into the agent.
//! - Own the network policy and the shared HTTP client (local-only mode blocks everything but loopback).
//! - Run the job queue: tool semaphore, cancellation tokens, recovering incomplete tasks as `Interrupted` on startup.
//! - Expose the single API that the CLI and the Tauri commands call.
//!
//! ## Must not
//!
//! - Hold business rules that belong in agent, policy or tools. It wires; it doesn't decide.
//! - Hard-code home directories.
//!
//! ## Allowed internal dependencies
//!
//! `loki-core`, `loki-llm`, `loki-tools`, `loki-policy`, `loki-memory`, `loki-voice`, `loki-platform`, `loki-agent`. Adding another edge needs an ADR (see `docs/ARCHITECTURE.md`).

pub mod app;
pub mod autostart;
pub mod config;
pub mod error;
pub mod ids;
pub mod lifecycle;
pub mod logging;
pub mod models;
pub mod net;
pub mod paths;
pub mod redact;
pub mod stream;
pub mod tooling;
pub mod voice;

pub use app::{PermissionRow, ReplyOutcome, Runtime, RuntimeOptions};
pub use config::{Config, NetworkMode, ProviderKind};
pub use error::RuntimeError;
pub use lifecycle::LifecycleControl;
pub use loki_agent::{DenyAllApprovals, ToolEvent, ToolUi};
pub use loki_llm::CancellationToken;
pub use loki_llm::{MockScript, ProviderHealth, PullProgress};
pub use loki_memory::{TaskRow, ToolRunFilter, ToolRunRow};
pub use loki_policy::PendingApproval;
/// Test double for `--mock-platform` and tests (fake apps; never launches anything).
pub use loki_tools::platform::MockPlatform;
pub use paths::{DATA_DIR_ENV, Paths};
pub use redact::{Redactor, redact};
pub use stream::EventSink;
