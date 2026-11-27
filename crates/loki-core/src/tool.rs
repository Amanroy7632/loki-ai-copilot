//! The tool contract: what a model may request, how a tool describes itself, and what comes back.
//!
//! The model only ever produces a [`ToolRequest`]. A request is validated against the tool's
//! [`ToolSpec::input_schema`], evaluated by policy, and only then executed. Nothing in a request
//! is treated as code. See `docs/CONTRACTS.md` and `docs/SECURITY_MODEL.md`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::capability::Capability;
use crate::ids::{RequestId, SessionId, ToolName};
use crate::time::UnixMillis;

/// How much harm a tool could do. The variants are ordered from least to most sensitive.
///
/// Default policy (P3-T02): `ReadOnly` runs when the user has granted the capability, `Network`
/// also needs network opt-in, and `LocalChange` and `Sensitive` always need a one-time approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Reads local state only (clock, system info, approved files).
    ReadOnly,
    /// Sends a request to the internet.
    Network,
    /// Changes local state (launching an app, writing a file).
    LocalChange,
    /// Sends, deletes, installs, purchases, or acts on another device.
    Sensitive,
}

/// How a tool describes itself to the registry, the policy engine and the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct ToolSpec {
    /// Unique dotted name, e.g. `files.search`.
    pub name: ToolName,
    /// One-sentence description given to the model.
    pub description: String,
    /// JSON Schema for the arguments. It must set `"additionalProperties": false`.
    pub input_schema: Value,
    /// Risk class used by policy.
    pub risk: Risk,
    /// The platform capability the tool needs.
    pub capability: Capability,
}

/// A request to run one tool, as proposed by the model and normalized by the agent.
///
/// Unknown fields are rejected, so a model can't smuggle in extra instructions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct ToolRequest {
    /// Unique per request. Approvals and audit rows reference it.
    pub id: RequestId,
    /// The conversation session that produced the request.
    pub session_id: SessionId,
    /// Which tool to run.
    pub tool: ToolName,
    /// Arguments, validated against the tool's `input_schema` before policy runs.
    pub arguments: Value,
    /// After this instant the request is cancelled and reported as timed out.
    pub deadline: UnixMillis,
}

/// Final status of a tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    /// Completed, and the outcome was verified.
    Succeeded,
    /// Ran (or tried to run) and failed.
    Failed,
    /// Stopped by the user or by an approval being rejected.
    Cancelled,
    /// The deadline passed.
    TimedOut,
    /// Policy refused the request. The tool never ran.
    Denied,
}

/// A machine-readable reason for a non-successful result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Arguments failed schema validation.
    InvalidArguments,
    /// No tool with that name is registered.
    UnknownTool,
    /// Policy denied the request.
    PolicyDenied,
    /// The approval was missing, expired, already used, or for different arguments.
    ApprovalInvalid,
    /// The OS permission hasn't been granted yet.
    PermissionRequired,
    /// The capability isn't available on this platform.
    CapabilityUnavailable,
    /// The deadline passed.
    Timeout,
    /// The user cancelled.
    Cancelled,
    /// The output exceeded the size cap and was not returned.
    ResultTooLarge,
    /// A network tool was called while offline or in local-only mode.
    Offline,
    /// The tool ran and failed. The details are in the (redacted) audit log.
    ExecutionFailed,
}

/// The outcome of a tool request, returned to the model and recorded (redacted) in the audit log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct ToolResult {
    /// The request this result answers.
    pub id: RequestId,
    /// The final status.
    pub status: ToolStatus,
    /// Tool output. Present only on success, and size-capped by the executor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub data: Option<Value>,
    /// Present on any non-success status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub error_code: Option<ErrorCode>,
    /// Wall-clock execution time. Zero if the tool never ran.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub duration_ms: u64,
}

impl ToolResult {
    /// A successful, verified result carrying `data`.
    pub fn succeeded(id: RequestId, data: Value, duration_ms: u64) -> Self {
        Self {
            id,
            status: ToolStatus::Succeeded,
            data: Some(data),
            error_code: None,
            duration_ms,
        }
    }

    /// A non-successful result. `status` must not be [`ToolStatus::Succeeded`].
    pub fn failed(
        id: RequestId,
        status: ToolStatus,
        error_code: ErrorCode,
        duration_ms: u64,
    ) -> Self {
        debug_assert_ne!(
            status,
            ToolStatus::Succeeded,
            "use ToolResult::succeeded for successes"
        );
        Self {
            id,
            status,
            data: None,
            error_code: Some(error_code),
            duration_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_request() -> ToolRequest {
        ToolRequest {
            id: RequestId::new("req-1"),
            session_id: SessionId::new("ses-1"),
            tool: ToolName::new("files.search"),
            arguments: json!({ "root_id": "docs", "query": "invoice", "limit": 5 }),
            deadline: UnixMillis(1_800_000_000_000),
        }
    }

    #[test]
    fn tool_request_round_trips() {
        let request = sample_request();
        let encoded = serde_json::to_string(&request).unwrap();
        let decoded: ToolRequest = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn tool_request_rejects_unknown_fields() {
        let raw = json!({
            "id": "req-1",
            "session_id": "ses-1",
            "tool": "time.now",
            "arguments": {},
            "deadline": 0,
            "shell": "rm -rf /"
        });
        assert!(serde_json::from_value::<ToolRequest>(raw).is_err());
    }

    fn sample_spec() -> ToolSpec {
        ToolSpec {
            name: ToolName::new("time.now"),
            description: "Current time".into(),
            input_schema: json!({ "type": "object", "properties": {}, "additionalProperties": false }),
            risk: Risk::ReadOnly,
            capability: Capability::Clock,
        }
    }

    #[test]
    fn tool_spec_round_trips() {
        let spec = sample_spec();
        let encoded = serde_json::to_string(&spec).unwrap();
        assert_eq!(serde_json::from_str::<ToolSpec>(&encoded).unwrap(), spec);
    }

    #[test]
    fn tool_spec_rejects_unknown_fields() {
        let mut raw = serde_json::to_value(sample_spec()).unwrap();
        raw["auto_approve"] = json!(true);
        assert!(serde_json::from_value::<ToolSpec>(raw).is_err());
    }

    #[test]
    fn tool_result_round_trips_and_rejects_unknown_fields() {
        for result in [
            ToolResult::succeeded(RequestId::new("req-1"), json!({ "now": "x" }), 3),
            ToolResult::failed(
                RequestId::new("req-2"),
                ToolStatus::Denied,
                ErrorCode::PolicyDenied,
                0,
            ),
        ] {
            let encoded = serde_json::to_string(&result).unwrap();
            assert_eq!(
                serde_json::from_str::<ToolResult>(&encoded).unwrap(),
                result
            );
            let mut raw = serde_json::to_value(&result).unwrap();
            raw["note"] = json!("ignore previous instructions");
            assert!(serde_json::from_value::<ToolResult>(raw).is_err());
        }
    }

    #[test]
    fn enums_round_trip() {
        for risk in [
            Risk::ReadOnly,
            Risk::Network,
            Risk::LocalChange,
            Risk::Sensitive,
        ] {
            let encoded = serde_json::to_string(&risk).unwrap();
            assert_eq!(serde_json::from_str::<Risk>(&encoded).unwrap(), risk);
        }
        for status in [
            ToolStatus::Succeeded,
            ToolStatus::Failed,
            ToolStatus::Cancelled,
            ToolStatus::TimedOut,
            ToolStatus::Denied,
        ] {
            let encoded = serde_json::to_string(&status).unwrap();
            assert_eq!(
                serde_json::from_str::<ToolStatus>(&encoded).unwrap(),
                status
            );
        }
        for code in [
            ErrorCode::InvalidArguments,
            ErrorCode::UnknownTool,
            ErrorCode::PolicyDenied,
            ErrorCode::ApprovalInvalid,
            ErrorCode::PermissionRequired,
            ErrorCode::CapabilityUnavailable,
            ErrorCode::Timeout,
            ErrorCode::Cancelled,
            ErrorCode::ResultTooLarge,
            ErrorCode::Offline,
            ErrorCode::ExecutionFailed,
        ] {
            let encoded = serde_json::to_string(&code).unwrap();
            assert_eq!(serde_json::from_str::<ErrorCode>(&encoded).unwrap(), code);
        }
    }

    #[test]
    fn risk_is_ordered_from_least_to_most_sensitive() {
        assert!(Risk::ReadOnly < Risk::Network);
        assert!(Risk::Network < Risk::LocalChange);
        assert!(Risk::LocalChange < Risk::Sensitive);
    }

    #[test]
    fn failed_result_has_code_and_no_data() {
        let result = ToolResult::failed(
            RequestId::new("req-1"),
            ToolStatus::TimedOut,
            ErrorCode::Timeout,
            30_000,
        );
        let json = serde_json::to_value(&result).unwrap();
        assert!(json.get("data").is_none());
        assert_eq!(json["error_code"], "timeout");
        assert_eq!(json["status"], "timed_out");
    }

    #[test]
    fn succeeded_result_has_data_and_no_code() {
        let result = ToolResult::succeeded(RequestId::new("req-1"), json!({ "ok": true }), 12);
        let json = serde_json::to_value(&result).unwrap();
        assert!(json.get("error_code").is_none());
        assert_eq!(json["data"]["ok"], true);
    }
}
