//! The `Tool` trait and what a tool receives when it runs.

use std::sync::Arc;

use futures::future::BoxFuture;
use loki_core::{ANY_SCOPE, ErrorCode, ToolSpec, UnixMillis};
use loki_platform::Platform;
use loki_policy::Allowed;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// What a running tool gets besides its arguments.
#[derive(Clone)]
pub struct ToolContext {
    /// Fires when the user cancels. Long tools must check it.
    pub cancel: CancellationToken,
    /// The request's deadline. The executor also enforces it.
    pub deadline: UnixMillis,
    /// OS access. Tools never touch OS APIs directly.
    pub platform: Arc<dyn Platform>,
}

/// Why a tool failed. Messages may reach the model and the audit log: no file contents or secrets.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    /// Arguments passed the schema but are still invalid (e.g. an unknown `app_id`).
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),
    /// The OS needs the user to grant a permission first.
    #[error("permission required: {0}")]
    PermissionRequired(String),
    /// Not possible here.
    #[error("unavailable: {0}")]
    Unavailable(String),
    /// Refused for safety (e.g. a path escaping its root). Never retried.
    #[error("refused: {0}")]
    Refused(String),
    /// It ran, but the effect could not be confirmed. Reported honestly, never as success.
    #[error("not verified: {0}")]
    NotVerified(String),
    /// It ran and failed.
    #[error("failed: {0}")]
    Failed(String),
}

impl ToolError {
    /// The contract error code for this failure.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidArguments(_) => ErrorCode::InvalidArguments,
            Self::PermissionRequired(_) => ErrorCode::PermissionRequired,
            Self::Unavailable(_) => ErrorCode::CapabilityUnavailable,
            Self::Refused(_) => ErrorCode::PolicyDenied,
            Self::NotVerified(_) | Self::Failed(_) => ErrorCode::ExecutionFailed,
        }
    }
}

/// A typed, allowlisted capability the model may request. There is no "run command" tool.
pub trait Tool: Send + Sync {
    /// Name, description, strict input schema, risk and capability.
    fn spec(&self) -> &ToolSpec;

    /// The grant scope this call touches (e.g. the allowed-root ID). `*` for unscoped tools.
    /// Called only with schema-valid arguments.
    fn scope(&self, _arguments: &Value) -> String {
        String::from(ANY_SCOPE)
    }

    /// Runs the tool. `permit` proves policy allowed this exact request (ADR-0011); the executor
    /// checks it matches before calling. Arguments are already schema-validated.
    fn execute<'a>(
        &'a self,
        arguments: Value,
        context: ToolContext,
        permit: &'a Allowed,
    ) -> BoxFuture<'a, Result<Value, ToolError>>;

    /// A one-line audit summary of a successful result. **Never** include file contents,
    /// secrets or full output; the default records only the size.
    fn audit_summary(&self, result: &Value) -> String {
        format!("ok, {} bytes of output", result.to_string().len())
    }
}
