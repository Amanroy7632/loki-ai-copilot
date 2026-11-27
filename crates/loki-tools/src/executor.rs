//! The executor (P3-T04): runs one permitted request with a concurrency limit, a deadline, a
//! cancellation token and a result-size cap.
//!
//! [`Executor::run`] takes the [`Allowed`] permit **by value**. The permit isn't `Clone`, so it
//! is used up by one run. It is checked against the request before the tool is touched.

use std::sync::Arc;
use std::time::{Duration, Instant};

use loki_core::{ErrorCode, ToolRequest, ToolResult, ToolStatus, UnixMillis};
use loki_platform::Platform;
use loki_policy::Allowed;
use serde_json::Value;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::tool::{Tool, ToolContext, ToolError};

/// Default result cap (CONTRACTS.md): 64 KiB of JSON.
pub const DEFAULT_RESULT_CAP_BYTES: usize = 64 * 1024;
/// Default concurrent tool jobs.
pub const DEFAULT_CONCURRENCY: usize = 2;

/// The full outcome of a run: the contract result plus what the audit log needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Execution {
    /// Returned to the model (data only on success).
    pub result: ToolResult,
    /// Redacted one-line summary for `tool_runs.redacted_result`. Never contains output data.
    pub audit_summary: String,
}

/// Runs tools. Cheap to share (`Arc` inside).
#[derive(Debug, Clone)]
pub struct Executor {
    semaphore: Arc<Semaphore>,
    result_cap: usize,
}

impl Default for Executor {
    fn default() -> Self {
        Self::new(DEFAULT_CONCURRENCY, DEFAULT_RESULT_CAP_BYTES)
    }
}

fn millis_since(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

impl Executor {
    /// `concurrency` simultaneous tools (at least 1); results over `result_cap` bytes are refused.
    pub fn new(concurrency: usize, result_cap: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(concurrency.max(1))),
            result_cap,
        }
    }

    /// Runs `request` with `tool`, if and only if `permit` is for this exact request.
    pub async fn run(
        &self,
        tool: &dyn Tool,
        request: &ToolRequest,
        permit: Allowed,
        platform: Arc<dyn Platform>,
        cancel: CancellationToken,
    ) -> Execution {
        let started = Instant::now();
        let fail = |status: ToolStatus, code: ErrorCode, summary: String| Execution {
            result: ToolResult::failed(request.id.clone(), status, code, millis_since(started)),
            audit_summary: summary,
        };

        if !permit.matches(request) || tool.spec().name != request.tool {
            return fail(
                ToolStatus::Denied,
                ErrorCode::ApprovalInvalid,
                String::from("permit does not match the request"),
            );
        }
        let now = UnixMillis::now();
        if now.has_reached(request.deadline) {
            return fail(
                ToolStatus::TimedOut,
                ErrorCode::Timeout,
                String::from("deadline passed before start"),
            );
        }
        let remaining =
            Duration::from_millis(u64::try_from(request.deadline.0 - now.0).unwrap_or(0));
        let deadline = tokio::time::Instant::now() + remaining;

        let permit_slot = tokio::select! {
            biased;
            () = cancel.cancelled() => return fail(ToolStatus::Cancelled, ErrorCode::Cancelled, String::from("cancelled while queued")),
            () = tokio::time::sleep_until(deadline) => return fail(ToolStatus::TimedOut, ErrorCode::Timeout, String::from("timed out while queued")),
            slot = self.semaphore.acquire() => slot,
        };
        let Ok(_slot) = permit_slot else {
            return fail(
                ToolStatus::Failed,
                ErrorCode::ExecutionFailed,
                String::from("executor closed"),
            );
        };

        let context = ToolContext {
            cancel: cancel.clone(),
            deadline: request.deadline,
            platform,
        };
        let outcome: Result<Value, ToolError> = tokio::select! {
            biased;
            () = cancel.cancelled() => return fail(ToolStatus::Cancelled, ErrorCode::Cancelled, String::from("cancelled")),
            () = tokio::time::sleep_until(deadline) => return fail(ToolStatus::TimedOut, ErrorCode::Timeout, String::from("timed out")),
            outcome = tool.execute(request.arguments.clone(), context, &permit) => outcome,
        };

        match outcome {
            Ok(value) => {
                let size = serde_json::to_vec(&value).map_or(usize::MAX, |bytes| bytes.len());
                if size > self.result_cap {
                    return fail(
                        ToolStatus::Failed,
                        ErrorCode::ResultTooLarge,
                        format!(
                            "result of {size} bytes exceeds the {} byte cap",
                            self.result_cap
                        ),
                    );
                }
                let audit_summary = tool.audit_summary(&value);
                Execution {
                    result: ToolResult::succeeded(request.id.clone(), value, millis_since(started)),
                    audit_summary,
                }
            }
            Err(error) => {
                let code = error.code();
                let status = if code == ErrorCode::PolicyDenied {
                    ToolStatus::Denied
                } else {
                    ToolStatus::Failed
                };
                fail(status, code, error.to_string())
            }
        }
    }
}
