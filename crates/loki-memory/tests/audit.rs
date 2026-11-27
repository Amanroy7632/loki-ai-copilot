//! P3-T03/T04 storage: task transitions, startup recovery, single-use approvals (SEC-05), grants.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use loki_core::{
    ApprovalId, Capability, Grant, RequestId, TaskId, TaskState, ToolStatus, UnixMillis,
};
use loki_memory::{Database, MemoryError, ToolRunFilter};

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

#[test]
fn legal_transitions_are_persisted_and_illegal_ones_rejected() {
    let db = db();
    let id = TaskId::new("t1");
    db.create_task(&id, "{}", UnixMillis(1)).unwrap();
    assert_eq!(db.get_task(&id).unwrap().unwrap().state, TaskState::Queued);
    // Skipping straight to Succeeded is illegal.
    let error = db
        .transition_task(&id, TaskState::Succeeded, UnixMillis(2))
        .unwrap_err();
    assert!(
        matches!(
            error,
            MemoryError::IllegalTransition {
                from: "queued",
                to: "succeeded"
            }
        ),
        "{error:?}"
    );
    db.transition_task(&id, TaskState::AwaitingApproval, UnixMillis(3))
        .unwrap();
    db.transition_task(&id, TaskState::Running, UnixMillis(4))
        .unwrap();
    db.transition_task(&id, TaskState::Succeeded, UnixMillis(5))
        .unwrap();
    let task = db.get_task(&id).unwrap().unwrap();
    assert_eq!(
        (task.state, task.updated_at),
        (TaskState::Succeeded, UnixMillis(5))
    );
    // Terminal: nothing further.
    assert!(
        db.transition_task(&id, TaskState::Running, UnixMillis(6))
            .is_err()
    );
    assert!(matches!(
        db.transition_task(&TaskId::new("nope"), TaskState::Running, UnixMillis(7)),
        Err(MemoryError::TaskNotFound(_))
    ));
}

#[test]
fn startup_recovery_marks_unfinished_tasks_interrupted_and_never_replays() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("loki.db");
    {
        let db = Database::open(&path).unwrap();
        for (id, steps) in [
            ("queued", vec![]),
            ("waiting", vec![TaskState::AwaitingApproval]),
            ("running", vec![TaskState::Running]),
            ("done", vec![TaskState::Running, TaskState::Succeeded]),
        ] {
            db.create_task(&TaskId::new(id), "{}", UnixMillis(1))
                .unwrap();
            for step in steps {
                db.transition_task(&TaskId::new(id), step, UnixMillis(2))
                    .unwrap();
            }
        }
    }
    let db = Database::open(&path).unwrap();
    assert_eq!(db.recover_interrupted_tasks(UnixMillis(10)).unwrap(), 3);
    for id in ["queued", "waiting", "running"] {
        assert_eq!(
            db.get_task(&TaskId::new(id)).unwrap().unwrap().state,
            TaskState::Interrupted,
            "{id}"
        );
    }
    assert_eq!(
        db.get_task(&TaskId::new("done")).unwrap().unwrap().state,
        TaskState::Succeeded
    );
    // Interrupted is terminal: it can't be resumed.
    assert!(
        db.transition_task(&TaskId::new("running"), TaskState::Running, UnixMillis(11))
            .is_err()
    );
    assert_eq!(db.recover_interrupted_tasks(UnixMillis(12)).unwrap(), 0);
}

#[test]
fn tool_runs_are_listed_newest_first_with_filters() {
    let db = db();
    let task = TaskId::new("t1");
    db.create_task(&task, "{}", UnixMillis(1)).unwrap();
    db.record_tool_run(
        "r1",
        &task,
        "time.now",
        ToolStatus::Succeeded,
        Some("time in UTC"),
        3,
        UnixMillis(10),
    )
    .unwrap();
    db.record_tool_run(
        "r2",
        &task,
        "app.open",
        ToolStatus::Denied,
        Some("no grant"),
        0,
        UnixMillis(20),
    )
    .unwrap();
    let all = db.list_tool_runs(&ToolRunFilter::default()).unwrap();
    assert_eq!(
        all.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        ["r2", "r1"]
    );
    let only = db
        .list_tool_runs(&ToolRunFilter {
            tool: Some("time.now".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(only.len(), 1);
    let recent = db
        .list_tool_runs(&ToolRunFilter {
            since: Some(UnixMillis(15)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(db.tool_runs_for_task(&task).unwrap().len(), 2);
}

#[test]
fn sec_05_approval_is_single_use_bound_and_expiring() {
    let db = db();
    let id = ApprovalId::new("ap-1");
    db.record_approval(
        &id,
        &RequestId::new("req-1"),
        "hash-a",
        true,
        UnixMillis(1_000),
    )
    .unwrap();
    // Wrong hash: arguments changed.
    assert!(
        db.consume_approval(&id, "hash-b", UnixMillis(500))
            .unwrap()
            .is_none()
    );
    // First valid use works.
    let consumed = db
        .consume_approval(&id, "hash-a", UnixMillis(500))
        .unwrap()
        .unwrap();
    assert_eq!(consumed.used_at, UnixMillis(500));
    assert_eq!(consumed.request_id, RequestId::new("req-1"));
    // Reuse fails.
    assert!(
        db.consume_approval(&id, "hash-a", UnixMillis(600))
            .unwrap()
            .is_none()
    );

    // Expired approvals can't be consumed.
    let late = ApprovalId::new("ap-2");
    db.record_approval(
        &late,
        &RequestId::new("req-2"),
        "hash-c",
        true,
        UnixMillis(1_000),
    )
    .unwrap();
    assert!(
        db.consume_approval(&late, "hash-c", UnixMillis(1_000))
            .unwrap()
            .is_none()
    );

    // Rejected approvals can never be consumed.
    let rejected = ApprovalId::new("ap-3");
    db.record_approval(
        &rejected,
        &RequestId::new("req-3"),
        "hash-d",
        false,
        UnixMillis(9_000),
    )
    .unwrap();
    assert!(
        db.consume_approval(&rejected, "hash-d", UnixMillis(10))
            .unwrap()
            .is_none()
    );
}

#[test]
fn grants_are_idempotent_revocable_and_exact() {
    let db = db();
    let docs = Grant {
        capability: Capability::FileRead,
        scope: "docs".into(),
    };
    db.grant("p1", &docs, UnixMillis(1)).unwrap();
    db.grant("p2", &docs, UnixMillis(2)).unwrap();
    db.grant("p3", &Grant::any(Capability::Clock), UnixMillis(3))
        .unwrap();
    assert_eq!(
        db.active_grants().unwrap(),
        vec![Grant::any(Capability::Clock), docs.clone()]
    );
    assert_eq!(db.revoke(&docs, UnixMillis(4)).unwrap(), 1);
    assert_eq!(
        db.active_grants().unwrap(),
        vec![Grant::any(Capability::Clock)]
    );
    assert_eq!(db.revoke(&docs, UnixMillis(5)).unwrap(), 0);
    // Unknown capability names in the table are ignored, not widened.
    db.connection()
        .execute("INSERT INTO permissions (id, capability, scope, granted_at) VALUES ('p9', 'root_shell', '*', 1)", [])
        .unwrap();
    assert_eq!(
        db.active_grants().unwrap(),
        vec![Grant::any(Capability::Clock)]
    );
}
