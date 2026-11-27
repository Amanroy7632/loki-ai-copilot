//! # loki-policy
//!
//! Policy engine, grants and approvals. Fails closed.
//!
//! **Status:** P3-T02 (`evaluate`) and P3-T03 (approval binding, the `Allowed` permit) implemented.
//! Later: P10-T03 (per-device scopes).
//!
//! ## Responsibilities
//!
//! - `evaluate(input) -> Decision`, as a pure function over the request, spec, capability status,
//!   grants and network mode.
//! - Issue the [`Allowed`] permit that `loki-tools` requires to run anything (ADR-0011).
//! - Bind approvals to request ID + SHA-256 of the canonical `{tool, arguments}` + expiry.
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

pub mod engine;
pub mod hash;
pub mod permit;

pub use engine::{PolicyInput, evaluate};
pub use hash::{args_hash, canonical_json};
pub use permit::{
    Allowed, AllowedVia, ApprovalError, Authorization, ConsumedApproval, DEFAULT_APPROVAL_TTL_MS,
    PendingApproval, allow_approved, authorize,
};
