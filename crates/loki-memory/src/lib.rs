//! # loki-memory
//!
//! SQLite storage: migrations, repositories, FTS5 retrieval, retention and deletion.
//!
//! **Status:** P1-T05 implemented opening, migrations (ADR-0007) and the conversations/messages
//! repository. Next tickets: P3-T04 (tasks, tool_runs), P7-T03..T05 (memories, reminders).
//!
//! ## Responsibilities
//!
//! - Open the database (foreign keys on, WAL, busy timeout) in the per-user data directory.
//! - Apply the forward-only migrations in `/migrations` transactionally.
//! - Provide repositories for conversations, messages, memories, tasks, tool_runs, approvals, permissions, settings, devices and reminders.
//! - Retrieve memories with FTS5 and attach provenance to each result.
//! - Export, delete and apply retention, including derived indexes.
//!
//! ## Must not
//!
//! - Store secrets, tokens or raw credentials.
//! - Hold a connection or transaction across an `.await`.
//! - Build SQL from strings. Always use parameters.
//!
//! ## Allowed internal dependencies
//!
//! `loki-core`. Adding another edge needs an ADR (see `docs/ARCHITECTURE.md`).

pub mod audit;
pub mod conversations;
pub mod db;
pub mod error;
pub mod store;

pub use audit::{ConsumedApprovalRow, TaskRow, ToolRunFilter, ToolRunRow};
pub use db::{Database, SCHEMA_VERSION};
pub use error::MemoryError;
pub use store::MemoryStore;
