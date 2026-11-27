//! P3-T01 (registry), P3-T04 (executor), P3-T06..T08 (built-in tools), SEC-02/03/04.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use loki_core::{
    ApprovalId, Capability, CapabilityStatus, ErrorCode, Grant, NetworkMode, RequestId, Risk,
    SessionId, ToolName, ToolRequest, ToolSpec, ToolStatus, UnixMillis,
};
use loki_platform::{AppEntry, MockPlatform, Platform};
use loki_policy::{Allowed, Authorization, PolicyInput, allow_approved, authorize};
use loki_tools::builtin::{FileRoots, FilesState};
use loki_tools::{
    Executor, Registry, RegistryError, Tool, ToolContext, ToolError, ValidationError,
    default_registry,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

// ------------------------------------------------------------------------------------ helpers

fn request(tool: &str, arguments: Value) -> ToolRequest {
    ToolRequest {
        id: RequestId::new(format!("req-{}", uuid::Uuid::now_v7().simple())),
        session_id: SessionId::new("ses"),
        tool: ToolName::new(tool),
        arguments,
        deadline: UnixMillis(UnixMillis::now().0 + 30_000),
    }
}

/// Gets a real permit the only way possible: through loki-policy. LocalChange tools go through a
/// (test-consumed) approval.
fn permit(spec: &ToolSpec, request: &ToolRequest, scope: &str) -> Allowed {
    let grants = [Grant::any(spec.capability)];
    let auth = authorize(
        &PolicyInput {
            request,
            spec: Some(spec),
            capability_status: Some(&CapabilityStatus::Supported),
            scope,
            grants: &grants,
            network_mode: NetworkMode::LocalOnly,
        },
        ApprovalId::new("ap"),
        UnixMillis::now(),
        60_000,
    );
    match auth {
        Authorization::Allowed(allowed) => allowed,
        Authorization::NeedsApproval(pending) => allow_approved(
            &loki_policy::ConsumedApproval {
                id: pending.id,
                request_id: pending.request_id,
                request_hash: pending.args_hash,
                expires_at: pending.expires_at,
                used_at: Some(UnixMillis::now()),
            },
            request,
        )
        .unwrap(),
        Authorization::Denied { reason } => panic!("denied: {reason}"),
    }
}

fn mock_platform() -> Arc<dyn Platform> {
    Arc::new(MockPlatform::default())
}

async fn run(
    registry: &Registry,
    request: &ToolRequest,
    platform: Arc<dyn Platform>,
) -> loki_tools::Execution {
    let tool = registry.validate(request).unwrap();
    let scope = tool.scope(&request.arguments);
    let allowed = permit(tool.spec(), request, &scope);
    Executor::default()
        .run(
            tool.as_ref(),
            request,
            allowed,
            platform,
            CancellationToken::new(),
        )
        .await
}

fn empty_registry_with_files(dir: &Path) -> (Registry, FilesState) {
    let (roots, warnings) = FileRoots::new([(String::from("docs"), dir.to_path_buf())]);
    assert!(warnings.is_empty(), "{warnings:?}");
    let state = FilesState {
        roots: Arc::new(roots),
        index: Arc::default(),
    };
    (default_registry(state.clone()).unwrap(), state)
}

// ---------------------------------------------------------------------------- registry (T01)

struct Fake(ToolSpec);
impl Tool for Fake {
    fn spec(&self) -> &ToolSpec {
        &self.0
    }
    fn execute<'a>(
        &'a self,
        _: Value,
        _: ToolContext,
        _: &'a Allowed,
    ) -> BoxFuture<'a, Result<Value, ToolError>> {
        async { Ok(json!({})) }.boxed()
    }
}

fn fake(name: &str, schema: Value) -> Arc<dyn Tool> {
    Arc::new(Fake(ToolSpec {
        name: ToolName::new(name),
        description: "fake".into(),
        input_schema: schema,
        risk: Risk::ReadOnly,
        capability: Capability::Clock,
    }))
}

fn closed() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

#[test]
fn every_builtin_spec_is_strict_and_well_named() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = empty_registry_with_files(dir.path());
    let specs = registry.specs();
    assert_eq!(
        specs.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        [
            "app.list",
            "app.open",
            "files.read",
            "files.search",
            "system.info",
            "time.now"
        ]
    );
    for spec in specs {
        assert!(loki_tools::is_valid_tool_name(spec.name.as_str()));
        loki_tools::check_strict_schema(&spec.input_schema)
            .unwrap_or_else(|e| panic!("{}: {e:?}", spec.name));
        assert_eq!(spec.input_schema["additionalProperties"], false);
    }
}

#[test]
fn duplicate_names_are_rejected() {
    let mut registry = Registry::new();
    registry.register(fake("time.now", closed())).unwrap();
    assert_eq!(
        registry.register(fake("time.now", closed())),
        Err(RegistryError::Duplicate("time.now".into()))
    );
}

#[test]
fn bad_names_are_rejected() {
    for name in [
        "time",
        "Time.now",
        "time.Now",
        "time..now",
        ".now",
        "time.now.",
        "time.now2",
        "time-now",
        "shell.exec;rm",
    ] {
        let mut registry = Registry::new();
        assert!(
            matches!(
                registry.register(fake(name, closed())),
                Err(RegistryError::BadName(_))
            ),
            "{name}"
        );
    }
    let mut registry = Registry::new();
    registry.register(fake("files.read_all", closed())).unwrap();
}

#[test]
fn loose_schemas_are_rejected() {
    for schema in [
        json!({ "type": "object", "properties": {} }),
        json!({ "type": "object", "properties": {}, "additionalProperties": true }),
        json!({ "type": "object", "properties": { "s": { "type": "string" } }, "additionalProperties": false }),
        json!({ "type": "object", "properties": { "a": { "type": "array", "items": { "type": "boolean" } } }, "additionalProperties": false }),
        json!({ "type": "object", "properties": { "n": { "type": "integer", "minimum": 0 } }, "additionalProperties": false }),
        json!({ "type": "object", "properties": { "o": { "type": "object", "properties": {} } }, "additionalProperties": false }),
        json!({ "type": "object", "properties": { "r": { "$ref": "https://example.com/s.json" } }, "additionalProperties": false }),
        json!({ "type": "object", "properties": { "x": { "type": ["string", "null"] } }, "additionalProperties": false }),
        json!({ "type": "string", "maxLength": 3 }),
    ] {
        let mut registry = Registry::new();
        assert!(
            matches!(
                registry.register(fake("x.y", schema.clone())),
                Err(RegistryError::LooseSchema { .. })
            ),
            "{schema}"
        );
    }
}

/// SEC-04: malformed arguments, unknown fields, wrong types and oversize strings are refused
/// before policy runs.
#[test]
fn sec_04_invalid_arguments_are_refused_before_policy() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = empty_registry_with_files(dir.path());
    for (tool, arguments) in [
        (
            "time.now",
            json!({ "timezone": "Asia/Tokyo", "shell": "rm -rf /" }),
        ),
        ("time.now", json!({ "timezone": 42 })),
        ("time.now", json!({ "timezone": "x".repeat(65) })),
        ("time.now", json!("not an object")),
        ("app.open", json!({})),
        (
            "app.open",
            json!({ "app_id": "com.apple.calculator; rm -rf ~" }),
        ),
        (
            "files.search",
            json!({ "root_id": "docs", "query": "a", "limit": 51 }),
        ),
        ("files.search", json!({ "root_id": "../etc", "query": "a" })),
        (
            "files.read",
            json!({ "file_id": "docs/../../etc/passwd", "max_bytes": 10 }),
        ),
        (
            "files.read",
            json!({ "file_id": "docs/0123", "max_bytes": 262_145 }),
        ),
    ] {
        let result = registry.validate(&request(tool, arguments.clone()));
        assert!(
            matches!(result, Err(ValidationError::InvalidArguments(_))),
            "{tool} {arguments} -> {:?}",
            result.err()
        );
    }
    assert!(matches!(
        registry.validate(&request("shell.exec", json!({}))),
        Err(ValidationError::UnknownTool(_))
    ));
}

// ---------------------------------------------------------------------------- executor (T04)

struct Slow;
impl Tool for Slow {
    fn spec(&self) -> &ToolSpec {
        static SPEC: std::sync::OnceLock<ToolSpec> = std::sync::OnceLock::new();
        SPEC.get_or_init(|| ToolSpec {
            name: ToolName::new("test.slow"),
            description: "sleeps".into(),
            input_schema: json!({ "type": "object", "properties": { "bytes": { "type": "integer", "minimum": 0, "maximum": 1_000_000 } }, "additionalProperties": false }),
            risk: Risk::ReadOnly,
            capability: Capability::Clock,
        })
    }
    fn execute<'a>(
        &'a self,
        arguments: Value,
        _: ToolContext,
        _: &'a Allowed,
    ) -> BoxFuture<'a, Result<Value, ToolError>> {
        async move {
            match arguments.get("bytes").and_then(Value::as_u64) {
                Some(n) => Ok(json!({ "blob": "x".repeat(usize::try_from(n).unwrap()) })),
                None => {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    Ok(json!({}))
                }
            }
        }
        .boxed()
    }
}

#[tokio::test]
async fn executor_enforces_the_deadline() {
    let mut req = request("test.slow", json!({}));
    req.deadline = UnixMillis(UnixMillis::now().0 + 100);
    let allowed = permit(Slow.spec(), &req, "*");
    let execution = Executor::default()
        .run(
            &Slow,
            &req,
            allowed,
            mock_platform(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(execution.result.status, ToolStatus::TimedOut);
    assert_eq!(execution.result.error_code, Some(ErrorCode::Timeout));
}

#[tokio::test]
async fn executor_cancels_promptly() {
    let req = request("test.slow", json!({}));
    let allowed = permit(Slow.spec(), &req, "*");
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        trigger.cancel();
    });
    let started = std::time::Instant::now();
    let execution = Executor::default()
        .run(&Slow, &req, allowed, mock_platform(), cancel)
        .await;
    assert_eq!(execution.result.status, ToolStatus::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn executor_refuses_oversize_results() {
    let req = request("test.slow", json!({ "bytes": 70_000 }));
    let allowed = permit(Slow.spec(), &req, "*");
    let execution = Executor::default()
        .run(
            &Slow,
            &req,
            allowed,
            mock_platform(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(execution.result.error_code, Some(ErrorCode::ResultTooLarge));
    assert!(
        execution.result.data.is_none(),
        "oversize data never reaches the model"
    );
}

#[tokio::test]
async fn executor_refuses_a_permit_for_another_request() {
    let issued_for = request("test.slow", json!({ "bytes": 1 }));
    let allowed = permit(Slow.spec(), &issued_for, "*");
    let other = request("test.slow", json!({ "bytes": 2 }));
    let execution = Executor::default()
        .run(
            &Slow,
            &other,
            allowed,
            mock_platform(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(execution.result.status, ToolStatus::Denied);
    assert_eq!(
        execution.result.error_code,
        Some(ErrorCode::ApprovalInvalid)
    );
}

// ------------------------------------------------------------------------- time/system (T06)

#[tokio::test]
async fn time_now_in_a_named_zone() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = empty_registry_with_files(dir.path());
    let execution = run(
        &registry,
        &request("time.now", json!({ "timezone": "Asia/Tokyo" })),
        mock_platform(),
    )
    .await;
    let data = execution.result.data.unwrap();
    assert_eq!(data["timezone"], "Asia/Tokyo");
    assert!(data["now"].as_str().unwrap().ends_with("+09:00"), "{data}");
}

#[tokio::test]
async fn time_now_rejects_unknown_zones() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = empty_registry_with_files(dir.path());
    let execution = run(
        &registry,
        &request("time.now", json!({ "timezone": "Mars/Olympus" })),
        mock_platform(),
    )
    .await;
    assert_eq!(
        execution.result.error_code,
        Some(ErrorCode::InvalidArguments)
    );
}

#[tokio::test]
async fn system_info_has_no_identifying_fields() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = empty_registry_with_files(dir.path());
    let execution = run(
        &registry,
        &request("system.info", json!({})),
        mock_platform(),
    )
    .await;
    let text = execution.result.data.unwrap().to_string().to_lowercase();
    assert!(text.contains("ram_total_bytes"));
    for forbidden in ["host", "user", "serial"] {
        assert!(!text.contains(forbidden), "{text}");
    }
}

// --------------------------------------------------------------------------------- apps (T07)

#[tokio::test]
async fn mock_app_open_launches_a_discovered_app_and_verifies_it() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = empty_registry_with_files(dir.path());
    let platform = Arc::new(MockPlatform::default());
    let execution = run(
        &registry,
        &request("app.open", json!({ "app_id": "com.example.calculator" })),
        platform.clone(),
    )
    .await;
    assert_eq!(
        execution.result.status,
        ToolStatus::Succeeded,
        "{execution:?}"
    );
    assert_eq!(platform.launches(), ["com.example.calculator"]);
    assert!(execution.audit_summary.contains("verified"));
}

#[tokio::test]
async fn mock_app_open_rejects_unknown_ids_without_launching() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = empty_registry_with_files(dir.path());
    let platform = Arc::new(MockPlatform::default());
    let execution = run(
        &registry,
        &request("app.open", json!({ "app_id": "com.evil.app" })),
        platform.clone(),
    )
    .await;
    assert_eq!(
        execution.result.error_code,
        Some(ErrorCode::InvalidArguments)
    );
    assert!(platform.launches().is_empty());
}

#[tokio::test]
async fn mock_unverified_launch_is_not_reported_as_success() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = empty_registry_with_files(dir.path());
    let platform = Arc::new(MockPlatform::default().with_unverified_launches());
    let execution = run(
        &registry,
        &request("app.open", json!({ "app_id": "com.example.calculator" })),
        platform,
    )
    .await;
    assert_eq!(execution.result.status, ToolStatus::Failed);
    assert!(
        execution.audit_summary.contains("not verified"),
        "{}",
        execution.audit_summary
    );
}

#[tokio::test]
async fn mock_app_list_filters_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = empty_registry_with_files(dir.path());
    let platform: Arc<dyn Platform> = Arc::new(MockPlatform::default().with_apps(vec![
        AppEntry {
            app_id: "com.example.calculator".into(),
            display_name: "Calculator".into(),
        },
        AppEntry {
            app_id: "com.example.notes".into(),
            display_name: "Notes".into(),
        },
    ]));
    let execution = run(
        &registry,
        &request("app.list", json!({ "query": "calc" })),
        platform,
    )
    .await;
    let data = execution.result.data.unwrap();
    assert_eq!(data["total_matches"], 1);
    assert_eq!(data["apps"][0]["app_id"], "com.example.calculator");
}

// -------------------------------------------------------------------------------- files (T08)

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

async fn search(registry: &Registry, query: &str, limit: Option<u64>) -> Value {
    let mut arguments = json!({ "root_id": "docs", "query": query });
    if let Some(limit) = limit {
        arguments["limit"] = json!(limit);
    }
    run(
        registry,
        &request("files.search", arguments),
        mock_platform(),
    )
    .await
    .result
    .data
    .unwrap()
}

async fn read(registry: &Registry, file_id: &str, max_bytes: u64) -> loki_tools::Execution {
    run(
        registry,
        &request(
            "files.read",
            json!({ "file_id": file_id, "max_bytes": max_bytes }),
        ),
        mock_platform(),
    )
    .await
}

#[tokio::test]
async fn search_then_read_a_text_file() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("notes/invoice-2026.md"),
        "# Invoice\nTotal: 42",
    );
    let (registry, _) = empty_registry_with_files(dir.path());
    let found = search(&registry, "INVOICE", None).await;
    let file = &found["files"][0];
    assert_eq!(file["path"], "notes/invoice-2026.md");
    assert!(
        !file.to_string().contains(dir.path().to_str().unwrap()),
        "absolute paths never reach the model"
    );
    let execution = read(&registry, file["file_id"].as_str().unwrap(), 1024).await;
    assert_eq!(
        execution.result.data.as_ref().unwrap()["content"],
        "# Invoice\nTotal: 42"
    );
    assert!(
        !execution.audit_summary.contains("Invoice"),
        "audit never holds content: {}",
        execution.audit_summary
    );
}

#[tokio::test]
async fn search_limit_is_enforced_and_hidden_files_skipped() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..10 {
        write(&dir.path().join(format!("report-{i}.txt")), "x");
    }
    write(&dir.path().join(".report-hidden.txt"), "x");
    write(&dir.path().join(".git/report-in-hidden-dir.txt"), "x");
    let (registry, _) = empty_registry_with_files(dir.path());
    let limited = search(&registry, "report", Some(3)).await;
    assert_eq!(limited["files"].as_array().unwrap().len(), 3);
    assert_eq!(limited["truncated"], true);
    let all = search(&registry, "report", Some(50)).await;
    let paths: Vec<&str> = all["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths.len(), 10);
    assert!(paths.iter().all(|p| !p.contains("hidden")), "{paths:?}");
}

#[tokio::test]
async fn read_truncates_at_max_bytes_on_a_char_boundary() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("long.txt"), &"é".repeat(100));
    let (registry, _) = empty_registry_with_files(dir.path());
    let id = search(&registry, "long", None).await["files"][0]["file_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let data = read(&registry, &id, 5).await.result.data.unwrap();
    assert_eq!(data["content"], "éé");
    assert_eq!(data["truncated"], true);
}

#[tokio::test]
async fn binary_and_unsupported_files_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data.txt"), b"abc\0def").unwrap();
    std::fs::write(dir.path().join("photo.jpg"), b"jpg").unwrap();
    let (registry, _) = empty_registry_with_files(dir.path());
    for name in ["data", "photo"] {
        let id = search(&registry, name, None).await["files"][0]["file_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let execution = read(&registry, &id, 100).await;
        assert_eq!(
            execution.result.status,
            ToolStatus::Denied,
            "{name}: {execution:?}"
        );
    }
}

#[tokio::test]
async fn unknown_or_forged_file_ids_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = empty_registry_with_files(dir.path());
    let execution = read(&registry, "docs/0123456789abcdef0123456789abcdef", 100).await;
    assert_eq!(
        execution.result.error_code,
        Some(ErrorCode::InvalidArguments)
    );
}

/// SEC-03: a symlink inside the root that points outside it is never followed by search, and a
/// symlink swapped in after search is refused at read time.
#[cfg(unix)]
#[tokio::test]
async fn sec_03_symlinks_out_of_the_root_are_refused() {
    let outside = tempfile::tempdir().unwrap();
    write(&outside.path().join("secret.txt"), "TOP SECRET");
    let dir = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("secret.txt"),
        dir.path().join("secret-link.txt"),
    )
    .unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("outside-dir")).unwrap();
    let (registry, _) = empty_registry_with_files(dir.path());
    assert!(
        search(&registry, "secret", None).await["files"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // Swap: a real file is found, then replaced by a symlink to the secret.
    write(&dir.path().join("swap.txt"), "harmless");
    let id = search(&registry, "swap", None).await["files"][0]["file_id"]
        .as_str()
        .unwrap()
        .to_owned();
    std::fs::remove_file(dir.path().join("swap.txt")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("secret.txt"),
        dir.path().join("swap.txt"),
    )
    .unwrap();
    let execution = read(&registry, &id, 100).await;
    assert_eq!(execution.result.status, ToolStatus::Denied, "{execution:?}");
    assert!(!format!("{execution:?}").contains("TOP SECRET"));
}

#[test]
fn roots_that_do_not_exist_are_skipped_with_a_warning() {
    let (roots, warnings) = FileRoots::new([
        (
            String::from("gone"),
            std::path::PathBuf::from("/definitely/not/here"),
        ),
        (String::from("Bad ID"), std::env::temp_dir()),
    ]);
    assert!(roots.ids().is_empty());
    assert_eq!(warnings.len(), 2);
}
