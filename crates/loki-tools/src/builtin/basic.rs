//! `time.now`, `system.info`, `app.list` and `app.open` (P3-T06, P3-T07).

use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use loki_core::{Capability, Risk, ToolName, ToolSpec};
use loki_platform::{Platform, PlatformError};
use loki_policy::Allowed;
use serde_json::{Value, json};

use crate::tool::{Tool, ToolContext, ToolError};

fn platform_error(error: PlatformError) -> ToolError {
    match error {
        PlatformError::Unavailable(m) => ToolError::Unavailable(m),
        PlatformError::PermissionRequired(m) => ToolError::PermissionRequired(m),
        PlatformError::UnknownApp(id) => {
            ToolError::InvalidArguments(format!("unknown app_id '{id}'; use app.list"))
        }
        PlatformError::Failed(m) => ToolError::Failed(m),
    }
}

/// Runs a blocking platform call off the async runtime.
async fn blocking<T: Send + 'static>(
    platform: Arc<dyn Platform>,
    call: impl FnOnce(&dyn Platform) -> Result<T, PlatformError> + Send + 'static,
) -> Result<T, ToolError> {
    tokio::task::spawn_blocking(move || call(platform.as_ref()))
        .await
        .map_err(|error| ToolError::Failed(error.to_string()))?
        .map_err(platform_error)
}

// ---------------------------------------------------------------------------------- time.now

/// `time.now {timezone?}`: the current time as ISO-8601, in an IANA zone or the system zone.
pub struct TimeNow {
    spec: ToolSpec,
}

impl Default for TimeNow {
    fn default() -> Self {
        Self {
            spec: ToolSpec {
                name: ToolName::new("time.now"),
                description: String::from(
                    "Current date and time as ISO-8601. Optional IANA time zone, e.g. Asia/Tokyo; default is the user's zone.",
                ),
                input_schema: json!({
                    "type": "object",
                    "properties": { "timezone": { "type": "string", "maxLength": 64, "pattern": "^[A-Za-z0-9_+\\-]+(/[A-Za-z0-9_+\\-]+)*$" } },
                    "additionalProperties": false
                }),
                risk: Risk::ReadOnly,
                capability: Capability::Clock,
            },
        }
    }
}

impl Tool for TimeNow {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn execute<'a>(
        &'a self,
        arguments: Value,
        _context: ToolContext,
        _permit: &'a Allowed,
    ) -> BoxFuture<'a, Result<Value, ToolError>> {
        async move {
            let zone = match arguments.get("timezone").and_then(Value::as_str) {
                Some(name) => jiff::tz::TimeZone::get(name).map_err(|_| {
                    ToolError::InvalidArguments(format!("unknown time zone '{name}'"))
                })?,
                None => jiff::tz::TimeZone::try_system().unwrap_or(jiff::tz::TimeZone::UTC),
            };
            let now = jiff::Timestamp::now().to_zoned_datetime(zone.clone());
            Ok(json!({
                "now": now.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string(),
                "timezone": zone.iana_name().unwrap_or("system"),
                "weekday": now.strftime("%A").to_string(),
            }))
        }
        .boxed()
    }

    fn audit_summary(&self, result: &Value) -> String {
        format!("time in {}", result["timezone"].as_str().unwrap_or("?"))
    }
}

// ------------------------------------------------------------------------------- system.info

/// `system.info {}`: OS, CPU, RAM and disk. No hostname or username.
pub struct SystemInfoTool {
    spec: ToolSpec,
}

impl Default for SystemInfoTool {
    fn default() -> Self {
        Self {
            spec: ToolSpec {
                name: ToolName::new("system.info"),
                description: String::from("The computer's OS, CPU, memory and disk summary."),
                input_schema: json!({ "type": "object", "properties": {}, "additionalProperties": false }),
                risk: Risk::ReadOnly,
                capability: Capability::SystemInfo,
            },
        }
    }
}

impl Tool for SystemInfoTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn execute<'a>(
        &'a self,
        _arguments: Value,
        context: ToolContext,
        _permit: &'a Allowed,
    ) -> BoxFuture<'a, Result<Value, ToolError>> {
        async move {
            let info = blocking(context.platform, |p| p.system_info()).await?;
            serde_json::to_value(info).map_err(|error| ToolError::Failed(error.to_string()))
        }
        .boxed()
    }

    fn audit_summary(&self, _result: &Value) -> String {
        String::from("system summary")
    }
}

// ---------------------------------------------------------------------------------- app.list

/// `app.list {query?, limit?}`: discovered apps, so the model can find a valid `app_id`.
pub struct AppList {
    spec: ToolSpec,
}

impl Default for AppList {
    fn default() -> Self {
        Self {
            spec: ToolSpec {
                name: ToolName::new("app.list"),
                description: String::from(
                    "Installed apps (app_id and name). Optional case-insensitive name filter. Use it to find the app_id for app.open.",
                ),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "maxLength": 64 },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
                    },
                    "additionalProperties": false
                }),
                risk: Risk::ReadOnly,
                capability: Capability::AppLaunch,
            },
        }
    }
}

impl Tool for AppList {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn execute<'a>(
        &'a self,
        arguments: Value,
        context: ToolContext,
        _permit: &'a Allowed,
    ) -> BoxFuture<'a, Result<Value, ToolError>> {
        async move {
            let query = arguments.get("query").and_then(Value::as_str).unwrap_or("").to_lowercase();
            let limit = arguments.get("limit").and_then(Value::as_u64).unwrap_or(20);
            let limit = usize::try_from(limit).unwrap_or(20);
            let apps = blocking(context.platform, |p| p.list_apps()).await?;
            let matching: Vec<_> = apps
                .into_iter()
                .filter(|app| query.is_empty() || app.display_name.to_lowercase().contains(&query) || app.app_id.to_lowercase().contains(&query))
                .collect();
            let total = matching.len();
            Ok(json!({ "apps": matching.into_iter().take(limit).collect::<Vec<_>>(), "total_matches": total }))
        }
        .boxed()
    }

    fn audit_summary(&self, result: &Value) -> String {
        format!("{} matching apps", result["total_matches"])
    }
}

// ---------------------------------------------------------------------------------- app.open

/// `app.open {app_id}`: launches a **discovered** app and verifies it started. LocalChange, so it
/// always needs the user's approval.
pub struct AppOpen {
    spec: ToolSpec,
}

impl Default for AppOpen {
    fn default() -> Self {
        Self {
            spec: ToolSpec {
                name: ToolName::new("app.open"),
                description: String::from(
                    "Open an installed app by its app_id (from app.list). The user must approve.",
                ),
                input_schema: json!({
                    "type": "object",
                    "properties": { "app_id": { "type": "string", "minLength": 1, "maxLength": 255, "pattern": "^[A-Za-z0-9][A-Za-z0-9._-]*$" } },
                    "required": ["app_id"],
                    "additionalProperties": false
                }),
                risk: Risk::LocalChange,
                capability: Capability::AppLaunch,
            },
        }
    }
}

impl Tool for AppOpen {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn execute<'a>(
        &'a self,
        arguments: Value,
        context: ToolContext,
        _permit: &'a Allowed,
    ) -> BoxFuture<'a, Result<Value, ToolError>> {
        async move {
            let app_id = arguments
                .get("app_id")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::InvalidArguments(String::from("app_id is required")))?
                .to_owned();
            let target = app_id.clone();
            let outcome = blocking(context.platform, move |p| p.launch_app(&target)).await?;
            if !outcome.verified {
                return Err(ToolError::NotVerified(outcome.detail));
            }
            Ok(json!({ "app_id": app_id, "verified": true, "already_running": outcome.already_running, "detail": outcome.detail }))
        }
        .boxed()
    }

    fn audit_summary(&self, result: &Value) -> String {
        format!(
            "launched {} (verified)",
            result["app_id"].as_str().unwrap_or("?")
        )
    }
}
