//! # loki-tools
//!
//! Tool trait, registry, JSON Schema validation, executor and built-in tools.
//!
//! **Status:** stub. Nothing is implemented yet. Tickets that fill this crate in:
//! P3-T01, P3-T04, P3-T06..T08; P7-T01 (web.search).
//!
//! ## Responsibilities
//!
//! - Define the `Tool` trait: `spec()` and `execute(validated_args, ctx)`.
//! - Keep the registry: unique names, and schemas with `additionalProperties: false`.
//! - Validate arguments against the schema before policy sees them.
//! - Run the executor: semaphore, deadline, cancellation, result-size cap, audit hook.
//! - Hold the built-in tools: `time.now`, `system.info`, `app.open`, `files.search`, `files.read`, `web.search`, `dev.docker_status`.
//!
//! ## Must not
//!
//! - Decide whether a call is allowed. That is `loki-policy`; the executor requires a `Decision::Allow` or a valid approval.
//! - Spawn shells or interpret model text as commands.
//! - Touch OS APIs directly. Go through `loki-platform` adapters.
//!
//! ## Allowed internal dependencies
//!
//! `loki-core`, `loki-platform`. Adding another edge needs an ADR (see `docs/ARCHITECTURE.md`).
