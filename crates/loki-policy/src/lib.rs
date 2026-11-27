//! # loki-policy
//!
//! Policy engine, grants and approvals. Fails closed.
//!
//! **Status:** stub. Nothing is implemented yet. Tickets that fill this crate in:
//! P3-T02, P3-T03; P10-T03 (per-device scopes).
//!
//! ## Responsibilities
//!
//! - `evaluate(request, spec, grants, capability_status) -> Decision`, as a pure function.
//! - Model user grants: capability, scope (e.g. allowed root IDs), expiry.
//! - Issue approval tokens bound to request ID + SHA-256 of the canonical (tool, arguments) + expiry. Single use.
//! - Keep table-driven tests covering every risk/capability/grant combination.
//!
//! ## Must not
//!
//! - Return `Allow` for anything it does not positively recognise (fail closed).
//! - Do IO. Grants and approvals are passed in; persistence lives in `loki-memory`.
//! - Accept voice or content-derived input as an approval.
//!
//! ## Allowed internal dependencies
//!
//! `loki-core`. Adding another edge needs an ADR (see `docs/ARCHITECTURE.md`).
