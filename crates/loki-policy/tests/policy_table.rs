//! P3-T02 acceptance: every Risk × CapabilityStatus × grant × network-mode combination, with the
//! complete list of combinations that may be allowed. Anything not listed must not be `Allow`.
//! Plus P3-T03 / SEC-05 approval binding.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use loki_core::{
    ApprovalId, Capability, CapabilityStatus, Decision, Grant, NetworkMode, RequestId, Risk,
    SessionId, ToolName, ToolRequest, ToolSpec, UnixMillis,
};
use loki_policy::{
    ApprovalError, Authorization, ConsumedApproval, PolicyInput, allow_approved, args_hash,
    authorize, evaluate,
};
use serde_json::json;

const RISKS: [Risk; 4] = [
    Risk::ReadOnly,
    Risk::Network,
    Risk::LocalChange,
    Risk::Sensitive,
];
const MODES: [NetworkMode; 2] = [NetworkMode::LocalOnly, NetworkMode::ToolsAllowed];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Supported,
    PermissionRequired,
    Denied,
    Unavailable,
    Unreported,
}
const STATUSES: [Status; 5] = [
    Status::Supported,
    Status::PermissionRequired,
    Status::Denied,
    Status::Unavailable,
    Status::Unreported,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GrantCase {
    None,
    ExactScope,
    Wildcard,
    OtherScope,
    OtherCapability,
}
const GRANTS: [GrantCase; 5] = [
    GrantCase::None,
    GrantCase::ExactScope,
    GrantCase::Wildcard,
    GrantCase::OtherScope,
    GrantCase::OtherCapability,
];

fn status(s: Status) -> Option<CapabilityStatus> {
    let reason = String::from("test");
    match s {
        Status::Supported => Some(CapabilityStatus::Supported),
        Status::PermissionRequired => Some(CapabilityStatus::PermissionRequired { reason }),
        Status::Denied => Some(CapabilityStatus::Denied { reason }),
        Status::Unavailable => Some(CapabilityStatus::Unavailable { reason }),
        Status::Unreported => None,
    }
}

fn grants(case: GrantCase) -> Vec<Grant> {
    match case {
        GrantCase::None => vec![],
        GrantCase::ExactScope => vec![Grant {
            capability: Capability::FileRead,
            scope: "docs".into(),
        }],
        GrantCase::Wildcard => vec![Grant::any(Capability::FileRead)],
        GrantCase::OtherScope => vec![Grant {
            capability: Capability::FileRead,
            scope: "photos".into(),
        }],
        GrantCase::OtherCapability => vec![Grant::any(Capability::AppLaunch)],
    }
}

fn spec(risk: Risk) -> ToolSpec {
    ToolSpec {
        name: ToolName::new("files.search"),
        description: "test".into(),
        input_schema: json!({ "type": "object", "additionalProperties": false }),
        risk,
        capability: Capability::FileRead,
    }
}

fn request() -> ToolRequest {
    ToolRequest {
        id: RequestId::new("req-1"),
        session_id: SessionId::new("ses-1"),
        tool: ToolName::new("files.search"),
        arguments: json!({ "root_id": "docs", "query": "q", "limit": 5 }),
        deadline: UnixMillis(10_000),
    }
}

/// The complete list of combinations that may produce `Allow`. If the engine allows anything
/// else, the test fails.
fn allow_listed(risk: Risk, s: Status, g: GrantCase, mode: NetworkMode) -> bool {
    let supported = s == Status::Supported;
    let granted = matches!(g, GrantCase::ExactScope | GrantCase::Wildcard);
    supported
        && granted
        && (risk == Risk::ReadOnly || (risk == Risk::Network && mode == NetworkMode::ToolsAllowed))
}

/// The complete list of combinations that may ask for approval.
fn approval_listed(risk: Risk, s: Status, g: GrantCase) -> bool {
    s == Status::Supported
        && matches!(g, GrantCase::ExactScope | GrantCase::Wildcard)
        && matches!(risk, Risk::LocalChange | Risk::Sensitive)
}

#[test]
fn every_combination_matches_the_table() {
    let request = request();
    let mut combos = 0;
    let mut allowed = Vec::new();
    for risk in RISKS {
        let spec = spec(risk);
        for s in STATUSES {
            let capability_status = status(s);
            for g in GRANTS {
                let grants = grants(g);
                for mode in MODES {
                    combos += 1;
                    let decision = evaluate(&PolicyInput {
                        request: &request,
                        spec: Some(&spec),
                        capability_status: capability_status.as_ref(),
                        scope: "docs",
                        grants: &grants,
                        network_mode: mode,
                    });
                    let expected_allow = allow_listed(risk, s, g, mode);
                    let expected_approval = approval_listed(risk, s, g);
                    match &decision {
                        Decision::Allow => {
                            assert!(
                                expected_allow,
                                "unlisted Allow: {risk:?} {s:?} {g:?} {mode:?}"
                            );
                            allowed.push((risk, mode, g));
                        }
                        Decision::RequireApproval => {
                            assert!(
                                expected_approval,
                                "unlisted approval: {risk:?} {s:?} {g:?} {mode:?}"
                            )
                        }
                        Decision::Deny { reason } => {
                            assert!(
                                !expected_allow && !expected_approval,
                                "wrongly denied ({reason}): {risk:?} {s:?} {g:?} {mode:?}"
                            );
                            assert!(!reason.is_empty());
                        }
                    }
                }
            }
        }
    }
    assert_eq!(combos, 4 * 5 * 5 * 2);
    // ReadOnly: 2 grant cases × 2 modes; Network: 2 grant cases × ToolsAllowed only.
    assert_eq!(allowed.len(), 4 + 2, "{allowed:?}");
}

#[test]
fn unknown_tool_is_denied_even_with_every_grant() {
    let all: Vec<Grant> = [
        Capability::Clock,
        Capability::FileRead,
        Capability::AppLaunch,
        Capability::Network,
    ]
    .into_iter()
    .map(Grant::any)
    .collect();
    let decision = evaluate(&PolicyInput {
        request: &request(),
        spec: None,
        capability_status: Some(&CapabilityStatus::Supported),
        scope: "*",
        grants: &all,
        network_mode: NetworkMode::ToolsAllowed,
    });
    assert!(matches!(decision, Decision::Deny { reason } if reason.contains("unknown tool")));
}

#[test]
fn spec_for_another_tool_is_denied() {
    let mut other = spec(Risk::ReadOnly);
    other.name = ToolName::new("time.now");
    let decision = evaluate(&PolicyInput {
        request: &request(),
        spec: Some(&other),
        capability_status: Some(&CapabilityStatus::Supported),
        scope: "docs",
        grants: &[Grant::any(Capability::FileRead)],
        network_mode: NetworkMode::LocalOnly,
    });
    assert!(matches!(decision, Decision::Deny { .. }));
}

// ---------------------------------------------------------------------------- approvals (SEC-05)

fn make_pending() -> (ToolRequest, loki_policy::PendingApproval) {
    let request = request();
    let spec = spec(Risk::LocalChange);
    let grants = [Grant::any(Capability::FileRead)];
    let auth = authorize(
        &PolicyInput {
            request: &request,
            spec: Some(&spec),
            capability_status: Some(&CapabilityStatus::Supported),
            scope: "docs",
            grants: &grants,
            network_mode: NetworkMode::LocalOnly,
        },
        ApprovalId::new("ap-1"),
        UnixMillis(1_000),
        60_000,
    );
    let Authorization::NeedsApproval(pending) = auth else {
        panic!("{auth:?}")
    };
    (request, pending)
}

fn consumed(p: &loki_policy::PendingApproval, used_at: Option<i64>) -> ConsumedApproval {
    ConsumedApproval {
        id: p.id.clone(),
        request_id: p.request_id.clone(),
        request_hash: p.args_hash.clone(),
        expires_at: p.expires_at,
        used_at: used_at.map(UnixMillis),
    }
}

#[test]
fn approval_preview_shows_tool_risk_and_every_argument() {
    let (_, pending) = make_pending();
    let preview = pending.preview();
    for needle in [
        "files.search",
        "LocalChange",
        "\"root_id\": \"docs\"",
        "\"query\": \"q\"",
        "\"limit\": 5",
    ] {
        assert!(preview.contains(needle), "missing {needle} in\n{preview}");
    }
    assert_eq!(pending.expires_at, UnixMillis(61_000));
}

#[test]
fn consumed_matching_approval_allows_exactly_that_request() {
    let (request, pending) = make_pending();
    let allowed = allow_approved(&consumed(&pending, Some(2_000)), &request).unwrap();
    assert!(allowed.matches(&request));
    assert_eq!(
        allowed.args_hash(),
        args_hash(&request.tool, &request.arguments)
    );
}

#[test]
fn sec_05_changed_arguments_fail() {
    let (mut request, pending) = make_pending();
    request.arguments["query"] = json!("something else");
    assert_eq!(
        allow_approved(&consumed(&pending, Some(2_000)), &request),
        Err(ApprovalError::ArgumentsChanged)
    );
    let (mut request, pending) = make_pending();
    request.tool = ToolName::new("files.read");
    assert_eq!(
        allow_approved(&consumed(&pending, Some(2_000)), &request),
        Err(ApprovalError::ArgumentsChanged)
    );
}

#[test]
fn sec_05_different_request_id_fails() {
    let (mut request, pending) = make_pending();
    request.id = RequestId::new("req-2");
    assert_eq!(
        allow_approved(&consumed(&pending, Some(2_000)), &request),
        Err(ApprovalError::WrongRequest)
    );
}

#[test]
fn sec_05_expired_approval_fails() {
    let (request, pending) = make_pending();
    assert_eq!(
        allow_approved(&consumed(&pending, Some(61_000)), &request),
        Err(ApprovalError::Expired)
    );
    assert_eq!(
        allow_approved(&consumed(&pending, Some(99_000)), &request),
        Err(ApprovalError::Expired)
    );
}

#[test]
fn sec_05_unconsumed_approval_fails() {
    let (request, pending) = make_pending();
    assert_eq!(
        allow_approved(&consumed(&pending, None), &request),
        Err(ApprovalError::NotConsumed)
    );
}

#[test]
fn a_permit_does_not_match_a_modified_request() {
    let request = request();
    let spec = spec(Risk::ReadOnly);
    let grants = [Grant::any(Capability::FileRead)];
    let Authorization::Allowed(allowed) = authorize(
        &PolicyInput {
            request: &request,
            spec: Some(&spec),
            capability_status: Some(&CapabilityStatus::Supported),
            scope: "docs",
            grants: &grants,
            network_mode: NetworkMode::LocalOnly,
        },
        ApprovalId::new("unused"),
        UnixMillis(0),
        0,
    ) else {
        panic!()
    };
    assert!(allowed.matches(&request));
    let mut changed = request.clone();
    changed.arguments["limit"] = json!(50);
    assert!(!allowed.matches(&changed));
    let mut other_id = request;
    other_id.id = RequestId::new("req-9");
    assert!(!allowed.matches(&other_id));
}
