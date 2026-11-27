//! The policy engine (P3-T02): a pure, fail-closed `evaluate()`.

use loki_core::{CapabilityStatus, Decision, Grant, NetworkMode, Risk, ToolRequest, ToolSpec};

/// Everything a policy decision may depend on. Nothing else (in particular no content from the
/// model, web pages or files) can influence it.
#[derive(Debug, Clone, Copy)]
pub struct PolicyInput<'a> {
    /// The validated request.
    pub request: &'a ToolRequest,
    /// The registered spec, or `None` if no tool has that name.
    pub spec: Option<&'a ToolSpec>,
    /// Status of the spec's capability as reported by the platform. `None` = not reported.
    pub capability_status: Option<&'a CapabilityStatus>,
    /// The scope this request touches (e.g. an allowed-root ID), or `*` for tools without scopes.
    pub scope: &'a str,
    /// The user's active grants.
    pub grants: &'a [Grant],
    /// The outbound network mode.
    pub network_mode: NetworkMode,
}

/// Evaluates in the fixed order from P3-T02. Every path that isn't explicitly `Allow` or
/// `RequireApproval` is `Deny`.
pub fn evaluate(input: &PolicyInput<'_>) -> Decision {
    let deny = |reason: String| Decision::Deny { reason };

    // 1. Unknown tool, or a spec that isn't the requested tool (an internal error).
    let Some(spec) = input.spec else {
        return deny(format!("unknown tool '{}'", input.request.tool));
    };
    if spec.name != input.request.tool {
        return deny(String::from(
            "internal error: spec does not match the request",
        ));
    }

    // 2. The capability must be Supported right now.
    match input.capability_status {
        Some(CapabilityStatus::Supported) => {}
        Some(CapabilityStatus::PermissionRequired { reason })
        | Some(CapabilityStatus::Denied { reason })
        | Some(CapabilityStatus::Unavailable { reason }) => {
            return deny(format!(
                "capability {:?} is not available: {reason}",
                spec.capability
            ));
        }
        None => {
            return deny(format!(
                "capability {:?} status is unknown",
                spec.capability
            ));
        }
    }

    // 3. The user must have granted the capability for this scope.
    if !input
        .grants
        .iter()
        .any(|grant| grant.covers(spec.capability, input.scope))
    {
        return deny(format!(
            "no grant for {:?} in scope '{}'. Grant it with `loki permissions grant`",
            spec.capability, input.scope
        ));
    }

    // 4/5. Risk.
    match spec.risk {
        Risk::Network if input.network_mode == NetworkMode::LocalOnly => {
            deny(String::from("network tools are blocked in local_only mode"))
        }
        Risk::LocalChange | Risk::Sensitive => Decision::RequireApproval,
        Risk::ReadOnly | Risk::Network => Decision::Allow,
    }
}
