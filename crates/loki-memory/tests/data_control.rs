//! Data lifecycle acceptance tests against isolated temporary databases.

#![allow(clippy::unwrap_used)]

use loki_core::{Capability, ConversationId, Grant, Message, MessageId, Role, TaskId, UnixMillis};
use loki_memory::Database;

fn seed(db: &mut Database, id: &str, content: &str, time: i64) {
    let conversation = ConversationId::new(id);
    db.create_conversation(&conversation, content, UnixMillis(time))
        .unwrap();
    db.append_message(&Message {
        id: MessageId::new(id),
        conversation_id: conversation,
        role: Role::Assistant,
        content: content.into(),
        created_at: UnixMillis(time),
        stopped: true,
    })
    .unwrap();
}

#[test]
fn export_then_delete_removes_history_from_rows_disk_and_wal_but_preserves_audit_and_grants() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.db");
    let mut db = Database::open(&path).unwrap();
    let marker = "uniquely-private-history-content-7b6f21e3";
    seed(&mut db, "c", marker, 1);
    db.create_task(&TaskId::new("task"), "{}", UnixMillis(1))
        .unwrap();
    db.grant(
        "grant",
        &Grant {
            capability: Capability::Clock,
            scope: "*".into(),
        },
        UnixMillis(1),
    )
    .unwrap();
    let export = db.export_data(UnixMillis(2)).unwrap();
    assert_eq!(export.history[0].conversation.title, marker);
    assert_eq!(export.history[0].messages[0].content, marker);
    assert!(export.history[0].messages[0].stopped);
    assert!(!export.memory_available);
    assert!(export.memories.is_empty());

    // P7 must extend deletion and this test when it introduces any derived FTS indexes.
    let fts_tables: i64 = db.connection().query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND (name LIKE '%fts%' OR name = 'memories')", [], |row| row.get(0)).unwrap();
    assert_eq!(
        fts_tables, 0,
        "new memory/FTS tables require export/deletion support"
    );
    assert_eq!(db.clear_history(None).unwrap(), 1);
    assert!(db.export_data(UnixMillis(3)).unwrap().history.is_empty());
    assert_eq!(
        db.connection()
            .query_row("SELECT count(*) FROM messages", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(db.get_task(&TaskId::new("task")).unwrap().is_some());
    assert_eq!(db.active_grants().unwrap().len(), 1);
    for file in [&path, &path.with_extension("db-wal")] {
        let bytes = std::fs::read(file).unwrap();
        assert!(
            !bytes
                .windows(marker.len())
                .any(|window| window == marker.as_bytes())
        );
    }
    assert_eq!(db.clear_history(None).unwrap(), 0, "idempotent");
}

#[test]
fn retention_uses_last_activity_and_keeps_the_exact_cutoff() {
    let mut db = Database::open_in_memory().unwrap();
    seed(&mut db, "expired", "old", 86_400_000 - 1);
    seed(&mut db, "boundary", "boundary", 86_400_000);
    seed(&mut db, "recent", "recent", 172_800_000);
    assert_eq!(
        db.apply_retention(None, UnixMillis(172_800_000)).unwrap(),
        0
    );
    assert_eq!(
        db.apply_retention(Some(1), UnixMillis(172_800_000))
            .unwrap(),
        1
    );
    assert_eq!(db.list_conversations().unwrap().len(), 2);
    assert!(
        db.list_messages(&ConversationId::new("expired"))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.list_messages(&ConversationId::new("boundary"))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn another_reader_preventing_wal_cleanup_is_reported_after_logical_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("busy.db");
    let mut db = Database::open(&path).unwrap();
    seed(&mut db, "c", "private", 1);
    let reader = rusqlite::Connection::open(&path).unwrap();
    reader
        .execute_batch("BEGIN; SELECT * FROM messages;")
        .unwrap();
    db.connection()
        .busy_timeout(std::time::Duration::ZERO)
        .unwrap();
    let error = db.clear_history(None).unwrap_err();
    assert!(error.to_string().contains("history was deleted"));
    assert!(db.list_conversations().unwrap().is_empty());
    reader.execute_batch("ROLLBACK").unwrap();
    db.clear_history(None).unwrap();
}
