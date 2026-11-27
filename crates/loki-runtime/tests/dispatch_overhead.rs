//! NFR-03 / P3 gate: warm tool-dispatch overhead (validation, policy, task + audit writes in
//! SQLite on disk, executor), measured with `time.now`, whose own work is microseconds.
//! Target: under 250 ms. Prints the numbers; asserts a generous bound so CI noise can't flake it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use loki_agent::{DenyAllApprovals, IdSource, ToolHost};
use loki_core::{Capability, Grant, NetworkMode, SessionId, ToolStatus, UnixMillis};
use loki_llm::{CancellationToken, ToolCallDraft};
use loki_memory::{Database, MemoryStore};
use loki_runtime::tooling::MemoryAuditStore;
use loki_tools::builtin::FilesState;
use loki_tools::platform::{MockPlatform, Platform};
use loki_tools::{Executor, default_registry};
use serde_json::json;

struct Ids;
impl IdSource for Ids {
    fn new_id(&self) -> String {
        loki_runtime::ids::new_run_id()
    }
    fn now(&self) -> UnixMillis {
        UnixMillis::now()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn warm_dispatch_overhead_is_well_under_250ms() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join("loki.db")).unwrap();
    db.grant("g", &Grant::any(Capability::Clock), UnixMillis::now())
        .unwrap();
    let store = MemoryStore::new(db);
    let host = ToolHost {
        registry: Arc::new(default_registry(FilesState::default()).unwrap()),
        executor: Executor::default(),
        platform: Arc::new(MockPlatform::default()) as Arc<dyn Platform>,
        audit: Arc::new(MemoryAuditStore(store)),
        network_mode: NetworkMode::LocalOnly,
        max_calls_per_turn: 6,
        timeout: Duration::from_secs(10),
        redact: Arc::new(|t: &str| loki_runtime::redact(t)),
    };
    let draft = ToolCallDraft {
        tool: loki_core::ToolName::new("time.now"),
        arguments: json!({ "timezone": "Asia/Tokyo" }),
    };
    let session = SessionId::new("bench");
    let cancel = CancellationToken::new();

    // Warm up (first SQLite statements, tz database load).
    for _ in 0..5 {
        host.dispatch(&draft, &session, &Ids, &DenyAllApprovals, &cancel)
            .await
            .unwrap();
    }
    let mut samples = Vec::new();
    for _ in 0..50 {
        let started = Instant::now();
        let dispatched = host
            .dispatch(&draft, &session, &Ids, &DenyAllApprovals, &cancel)
            .await
            .unwrap();
        samples.push(started.elapsed());
        assert_eq!(dispatched.result.status, ToolStatus::Succeeded);
    }
    samples.sort();
    let median = samples[samples.len() / 2];
    let p95 = samples[samples.len() * 95 / 100];
    let max = samples[samples.len() - 1];
    println!(
        "NFR-03 warm dispatch (time.now, 50 runs): median {median:?}, p95 {p95:?}, max {max:?}"
    );
    assert!(
        p95 < Duration::from_millis(250),
        "p95 {p95:?} exceeds the 250 ms target"
    );
}
