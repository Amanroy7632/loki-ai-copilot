//! P5-T05: the permissions screen's runtime API. Acceptance: revoking a grant immediately denies
//! the next call. Also: roots added at runtime reach the file tools without a restart, and a
//! removed or repointed root never keeps (or passes on) its grants.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use loki_core::{Capability, CapabilityStatus, ToolStatus, TurnEvent};
use loki_llm::{MockScript, MockStep};
use loki_runtime::{EventSink, MockPlatform, ProviderKind, Runtime, RuntimeOptions};
use loki_tools::platform::Platform;
use serde_json::{Value, json};

fn call(tool: &str, arguments: Value) -> MockScript {
    MockScript::new(vec![
        MockStep::ToolCall {
            tool: tool.into(),
            arguments,
        },
        MockStep::Done {
            prompt_tokens: None,
            completion_tokens: None,
        },
    ])
    .unwrap()
}

fn reply(text: &str) -> MockScript {
    MockScript::new(vec![
        MockStep::Token { text: text.into() },
        MockStep::Done {
            prompt_tokens: None,
            completion_tokens: None,
        },
    ])
    .unwrap()
}

fn runtime(dir: &tempfile::TempDir, scripts: Vec<MockScript>) -> Runtime {
    Runtime::open(RuntimeOptions {
        data_dir: Some(dir.path().to_path_buf()),
        provider: Some(ProviderKind::Mock),
        platform: Some(Arc::new(MockPlatform::default()) as Arc<dyn Platform>),
        mock_scripts: Some(scripts),
        mock_token_delay: Duration::from_millis(1),
        ..RuntimeOptions::default()
    })
    .unwrap()
}

/// Runs one turn and returns the status of its tool call.
async fn tool_status(rt: &Runtime, turn: &str, text: &str) -> Option<ToolStatus> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let store = Arc::clone(&events);
    let sink: EventSink = Arc::new(move |event| store.lock().unwrap().push(event));
    rt.chat_turn(turn, None, Some(text.to_owned()), sink)
        .await
        .unwrap();
    let events = events.lock().unwrap();
    events.iter().find_map(|event| match event {
        TurnEvent::ToolFinished { status, .. } => Some(*status),
        _ => None,
    })
}

fn scopes(rows: &[loki_runtime::PermissionRow], capability: Capability) -> Vec<String> {
    rows.iter()
        .find(|row| row.capability == capability)
        .map(|row| row.scopes.clone())
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn mock_revoking_a_grant_immediately_denies_the_next_call() {
    let dir = tempfile::tempdir().unwrap();
    let time = || call("time.now", json!({ "timezone": "Asia/Tokyo" }));
    let rt = runtime(
        &dir,
        vec![time(), reply("It's noon."), time(), reply("I can't.")],
    );
    rt.grant("clock", "*").await.unwrap();
    assert_eq!(
        tool_status(&rt, "t1", "time?").await,
        Some(ToolStatus::Succeeded)
    );

    assert_eq!(rt.revoke("clock", "*").await.unwrap(), 1);
    assert!(scopes(&rt.permissions().await.unwrap(), Capability::Clock).is_empty());
    // Same runtime, no restart: the very next call is denied.
    assert_eq!(
        tool_status(&rt, "t2", "time again?").await,
        Some(ToolStatus::Denied)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn permissions_report_unsupported_capabilities_with_a_reason_and_refuse_granting_them() {
    let dir = tempfile::tempdir().unwrap();
    let rt = runtime(&dir, Vec::new());
    let rows = rt.permissions().await.unwrap();
    assert_eq!(rows.len(), Capability::ALL.len());
    let unavailable: Vec<_> = rows
        .iter()
        .filter_map(|row| match &row.status {
            CapabilityStatus::Unavailable { reason } => Some((row.capability, reason.clone())),
            _ => None,
        })
        .collect();
    assert!(!unavailable.is_empty());
    for (capability, reason) in unavailable {
        assert!(!reason.trim().is_empty(), "{capability:?} has a reason");
        assert!(
            rt.grant(capability.as_str(), "*").await.is_err(),
            "{capability:?} can't be granted"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn mock_a_root_added_at_runtime_reaches_the_file_tools_without_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let docs = tempfile::tempdir().unwrap();
    std::fs::write(docs.path().join("invoice.txt"), "total 12").unwrap();
    let search = call(
        "files.search",
        json!({ "root_id": "docs", "query": "invoice" }),
    );
    let mut rt = runtime(&dir, vec![search, reply("Found it.")]);
    let added = rt.add_root("docs", docs.path()).await.unwrap();
    assert_eq!(added.id, "docs");
    assert_eq!(rt.roots(), [added]);
    rt.grant("file_read", "docs").await.unwrap();
    assert_eq!(
        tool_status(&rt, "t1", "find my invoice").await,
        Some(ToolStatus::Succeeded)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_or_repointing_a_root_revokes_its_grants() {
    let dir = tempfile::tempdir().unwrap();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let mut rt = runtime(&dir, Vec::new());

    // Removing a root revokes its grant; re-adding the same ID starts without access.
    rt.add_root("docs", first.path()).await.unwrap();
    rt.grant("file_read", "docs").await.unwrap();
    assert!(rt.remove_root("docs").await.unwrap());
    assert!(!rt.remove_root("docs").await.unwrap(), "already gone");
    assert!(rt.roots().is_empty());
    rt.add_root("docs", first.path()).await.unwrap();
    let rows = rt.permissions().await.unwrap();
    assert!(scopes(&rows, Capability::FileRead).is_empty());

    // Re-adding the same folder keeps the grant; pointing the ID elsewhere revokes it.
    rt.grant("file_read", "docs").await.unwrap();
    rt.add_root("docs", first.path()).await.unwrap();
    let rows = rt.permissions().await.unwrap();
    assert_eq!(scopes(&rows, Capability::FileRead), ["docs"]);
    rt.add_root("docs", second.path()).await.unwrap();
    let rows = rt.permissions().await.unwrap();
    assert!(scopes(&rows, Capability::FileRead).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_roots_are_refused_before_anything_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut rt = runtime(&dir, Vec::new());
    for id in ["", "Docs", "my docs", "../x", &"x".repeat(33)] {
        assert!(Runtime::validate_root_id(id).is_err(), "{id:?}");
    }
    let file = dir.path().join("not-a-dir.txt");
    std::fs::write(&file, "x").unwrap();
    assert!(rt.add_root("docs", &file).await.is_err());
    assert!(
        rt.add_root("docs", &dir.path().join("missing"))
            .await
            .is_err()
    );
    assert!(rt.roots().is_empty());
}
