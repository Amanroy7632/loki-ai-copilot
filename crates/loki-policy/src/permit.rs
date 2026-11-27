//! Proof of policy (P3-T03/T04, ADR-0011).
//!
//! [`Allowed`] is the only way to run a tool (`loki_tools::Tool::execute` takes `&Allowed`).
//! It has private fields, no public constructor and no `Clone`, so it can only come from:
//! - [`authorize`], when `evaluate()` returns `Allow`, or
//! - [`allow_approved`], given an approval the store has just consumed for this exact request.
//!
//! An approval binds `{request_id, args_hash, expires_at}` and is single use. Single use is
//! enforced by the store (a conditional `UPDATE … WHERE used_at IS NULL`); this module checks
//! everything else.

use loki_core::{ApprovalId, Decision, RequestId, Risk, ToolName, ToolRequest, UnixMillis};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::{PolicyInput, evaluate};
use crate::hash::args_hash;

/// Default lifetime of a pending approval.
pub const DEFAULT_APPROVAL_TTL_MS: i64 = 2 * 60 * 1000;

/// How a permit was obtained (recorded in the audit log).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllowedVia {
    /// Policy allowed it outright.
    Policy,
    /// The user approved this exact request.
    Approval,
}

/// Permission to run exactly one request. See the module docs.
#[derive(Debug, PartialEq, Eq)]
pub struct Allowed {
    request_id: RequestId,
    args_hash: String,
    via: AllowedVia,
}

impl Allowed {
    /// The request this permit is for.
    pub fn request_id(&self) -> &RequestId {
        &self.request_id
    }

    /// The bound arguments hash.
    pub fn args_hash(&self) -> &str {
        &self.args_hash
    }

    /// How it was obtained.
    pub fn via(&self) -> AllowedVia {
        self.via
    }

    /// `true` only if `request` is the exact request this permit was issued for: same ID, same
    /// tool and same arguments. The executor checks this before running anything.
    pub fn matches(&self, request: &ToolRequest) -> bool {
        self.request_id == request.id
            && self.args_hash == args_hash(&request.tool, &request.arguments)
    }
}

/// An approval waiting for the user. Persist it, show [`PendingApproval::preview`], and only on
/// an explicit "yes" mark it approved and consume it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingApproval {
    /// Approval ID (the runtime generates it).
    pub id: ApprovalId,
    /// The request it is bound to.
    pub request_id: RequestId,
    /// Tool name, shown to the user.
    pub tool: ToolName,
    /// Risk class, shown to the user.
    pub risk: Risk,
    /// **Every** argument, shown to the user.
    pub arguments: Value,
    /// Hash of `{tool, arguments}`.
    pub args_hash: String,
    /// After this instant the approval can't be used.
    pub expires_at: UnixMillis,
}

impl PendingApproval {
    /// Human-readable preview: tool, risk and every argument (pretty JSON, nothing elided).
    pub fn preview(&self) -> String {
        let arguments = serde_json::to_string_pretty(&self.arguments)
            .unwrap_or_else(|_| self.arguments.to_string());
        format!(
            "Tool:      {}\nRisk:      {:?}\nArguments: {}",
            self.tool, self.risk, arguments
        )
    }
}

/// The outcome of [`authorize`].
#[derive(Debug, PartialEq, Eq)]
pub enum Authorization {
    /// Run it.
    Allowed(Allowed),
    /// Ask the user first.
    NeedsApproval(PendingApproval),
    /// Never run it.
    Denied {
        /// Why (audited, and returned to the model as a `Denied` result).
        reason: String,
    },
}

/// Evaluates policy and turns the decision into a permit, an approval request or a denial.
pub fn authorize(
    input: &PolicyInput<'_>,
    approval_id: ApprovalId,
    now: UnixMillis,
    approval_ttl_ms: i64,
) -> Authorization {
    let hash = args_hash(&input.request.tool, &input.request.arguments);
    match evaluate(input) {
        Decision::Allow => Authorization::Allowed(Allowed {
            request_id: input.request.id.clone(),
            args_hash: hash,
            via: AllowedVia::Policy,
        }),
        Decision::RequireApproval => match input.spec {
            Some(spec) => Authorization::NeedsApproval(PendingApproval {
                id: approval_id,
                request_id: input.request.id.clone(),
                tool: input.request.tool.clone(),
                risk: spec.risk,
                arguments: input.request.arguments.clone(),
                args_hash: hash,
                expires_at: UnixMillis(now.0.saturating_add(approval_ttl_ms.max(0))),
            }),
            // evaluate() never asks for approval without a spec; fail closed anyway.
            None => Authorization::Denied {
                reason: String::from("internal error: approval without a tool spec"),
            },
        },
        Decision::Deny { reason } => Authorization::Denied { reason },
    }
}

/// An approval as read back from the store **after** it was atomically marked used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumedApproval {
    /// Approval ID.
    pub id: ApprovalId,
    /// The request it was issued for.
    pub request_id: RequestId,
    /// The bound hash.
    pub request_hash: String,
    /// Expiry.
    pub expires_at: UnixMillis,
    /// When it was consumed (must be set, and before expiry).
    pub used_at: Option<UnixMillis>,
}

/// Why an approval can't authorize a request (SEC-05).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApprovalError {
    /// The approval was never consumed by the store, so single use isn't proven.
    #[error("approval was not consumed")]
    NotConsumed,
    /// Issued for another request ID.
    #[error("approval belongs to a different request")]
    WrongRequest,
    /// The tool or arguments changed since the user approved.
    #[error("arguments changed after approval")]
    ArgumentsChanged,
    /// Expired before it was used.
    #[error("approval expired")]
    Expired,
}

/// Turns a consumed approval into a permit, if it matches `request` exactly and was used in time.
pub fn allow_approved(
    consumed: &ConsumedApproval,
    request: &ToolRequest,
) -> Result<Allowed, ApprovalError> {
    let used_at = consumed.used_at.ok_or(ApprovalError::NotConsumed)?;
    if consumed.request_id != request.id {
        return Err(ApprovalError::WrongRequest);
    }
    let hash = args_hash(&request.tool, &request.arguments);
    if consumed.request_hash != hash {
        return Err(ApprovalError::ArgumentsChanged);
    }
    if used_at.has_reached(consumed.expires_at) {
        return Err(ApprovalError::Expired);
    }
    Ok(Allowed {
        request_id: request.id.clone(),
        args_hash: hash,
        via: AllowedVia::Approval,
    })
}
