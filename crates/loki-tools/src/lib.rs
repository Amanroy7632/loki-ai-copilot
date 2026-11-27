//! # loki-tools
//!
//! Tool trait, registry, JSON Schema validation, executor and built-in tools.
//!
//! **Status:** P3-T01 (trait, registry, strict schemas), P3-T04 (executor), P3-T06..T08
//! (`time.now`, `system.info`, `app.list`, `app.open`, `files.search`, `files.read`).
//!
//! ## Responsibilities
//!
//! - Define the `Tool` trait: `spec()` and `execute(validated_args, ctx, permit)`.
//! - Keep the registry: unique names, and schemas with `additionalProperties: false`.
//! - Validate arguments against the schema before policy sees them.
//! - Run the executor: semaphore, deadline, cancellation, result-size cap.
//!
//! ## Must not
//!
//! - Decide whether a call is allowed. That is `loki-policy`; `execute` requires its `Allowed` permit (ADR-0011).
//! - Spawn shells or interpret model text as commands.
//! - Touch OS APIs directly. Go through `loki-platform` adapters (file reads are plain `std::fs`
//!   inside canonical allowed roots).
//!
//! ## Allowed internal dependencies
//!
//! `loki-core`, `loki-platform`, `loki-policy` (ADR-0011). Adding another edge needs an ADR.

pub mod builtin;
pub mod executor;
pub mod registry;
pub mod tool;

pub use builtin::default_registry;
pub use executor::{DEFAULT_CONCURRENCY, DEFAULT_RESULT_CAP_BYTES, Execution, Executor};
/// Re-exported so `loki-agent` can name the `Platform` trait without a direct dependency edge
/// on `loki-platform` (it uses the trait object only, never a concrete adapter).
pub use loki_platform as platform;
pub use registry::{
    Registry, RegistryError, ValidationError, check_strict_schema, is_valid_tool_name,
};
pub use tool::{Tool, ToolContext, ToolError};
