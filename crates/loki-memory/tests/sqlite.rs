//! P1-T05 acceptance tests: migrations, pragmas, foreign keys, repositories, parameterized SQL.
//! Every test uses its own temp directory, never real user data.

// Test helpers outside #[test] fns aren't covered by clippy.toml's allow-*-in-tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use loki_core::{ConversationId, Message, MessageId, Role, UnixMillis};
use loki_memory::{Database, MemoryError, MemoryStore, SCHEMA_VERSION};
use rusqlite::params;

fn db_path(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("loki.db")
}

fn message(id: &str, conversation: &str, role: Role, content: &str, at: i64) -> Message {
    Message {
        id: MessageId::new(id),
        conversation_id: ConversationId::new(conversation),
        role,
        content: content.into(),
        created_at: UnixMillis(at),
    }
}

fn table_names(db: &Database) -> Vec<String> {
    let mut statement = db
        .connection()
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn migrations_apply_to_an_empty_db() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&db_path(&dir)).unwrap();
    assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(SCHEMA_VERSION, 1);
    let tables = table_names(&db);
    for expected in ["conversations", "messages", "settings"] {
        assert!(
            tables.iter().any(|t| t == expected),
            "missing {expected}: {tables:?}"
        );
    }
}

#[test]
fn reopen_is_idempotent_and_keeps_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = db_path(&dir);
    {
        let mut db = Database::open(&path).unwrap();
        let id = ConversationId::new("c1");
        db.create_conversation(&id, "first", UnixMillis(10))
            .unwrap();
        db.append_message(&message("m1", "c1", Role::User, "hi", 11))
            .unwrap();
    }
    for _ in 0..2 {
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(db.list_conversations().unwrap().len(), 1);
        assert_eq!(
            db.list_messages(&ConversationId::new("c1")).unwrap().len(),
            1
        );
    }
}

#[test]
fn connection_pragmas_are_set() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&db_path(&dir)).unwrap();
    let conn = db.connection();
    let foreign_keys: i64 = conn
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .unwrap();
    let journal_mode: String = conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    let busy_timeout: i64 = conn
        .pragma_query_value(None, "busy_timeout", |row| row.get(0))
        .unwrap();
    assert_eq!(foreign_keys, 1);
    assert_eq!(journal_mode.to_lowercase(), "wal");
    assert_eq!(busy_timeout, 5_000);
}

#[test]
fn deleting_a_conversation_cascades_to_its_messages() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::open(&db_path(&dir)).unwrap();
    let keep = ConversationId::new("keep");
    let drop = ConversationId::new("drop");
    db.create_conversation(&keep, "keep", UnixMillis(1))
        .unwrap();
    db.create_conversation(&drop, "drop", UnixMillis(1))
        .unwrap();
    db.append_message(&message("k1", "keep", Role::User, "a", 2))
        .unwrap();
    db.append_message(&message("d1", "drop", Role::User, "b", 2))
        .unwrap();
    db.append_message(&message("d2", "drop", Role::Assistant, "c", 3))
        .unwrap();

    assert!(db.delete_conversation(&drop).unwrap());
    let orphans: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1",
            params!["drop"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(orphans, 0);
    assert_eq!(db.list_messages(&keep).unwrap().len(), 1);
    assert!(!db.delete_conversation(&drop).unwrap());
}

#[test]
fn foreign_key_is_enforced_for_raw_inserts() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&db_path(&dir)).unwrap();
    let result = db.connection().execute(
        "INSERT INTO messages (id, conversation_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params!["m1", "missing", "user", "x", 1],
    );
    assert!(result.is_err(), "orphan message must be refused");
}

#[test]
fn append_to_missing_conversation_is_refused() {
    let mut db = Database::open_in_memory().unwrap();
    let error = db
        .append_message(&message("m1", "nope", Role::User, "x", 1))
        .unwrap_err();
    assert!(
        matches!(error, MemoryError::ConversationNotFound(_)),
        "{error:?}"
    );
}

#[test]
fn role_check_constraint_rejects_unknown_roles() {
    let db = Database::open_in_memory().unwrap();
    db.create_conversation(&ConversationId::new("c1"), "t", UnixMillis(1))
        .unwrap();
    let result = db.connection().execute(
        "INSERT INTO messages (id, conversation_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params!["m1", "c1", "admin", "x", 1],
    );
    assert!(result.is_err());
}

#[test]
fn conversations_list_newest_first_and_messages_oldest_first() {
    let mut db = Database::open_in_memory().unwrap();
    db.create_conversation(&ConversationId::new("old"), "old", UnixMillis(1))
        .unwrap();
    db.create_conversation(&ConversationId::new("new"), "new", UnixMillis(5))
        .unwrap();
    // A message makes "old" the most recently updated.
    db.append_message(&message("m2", "old", Role::Assistant, "second", 20))
        .unwrap();
    db.append_message(&message("m1", "old", Role::User, "first", 10))
        .unwrap();

    let listed: Vec<String> = db
        .list_conversations()
        .unwrap()
        .into_iter()
        .map(|c| c.id.to_string())
        .collect();
    assert_eq!(listed, ["old", "new"]);
    let old = db
        .get_conversation(&ConversationId::new("old"))
        .unwrap()
        .unwrap();
    assert_eq!(
        old.updated_at,
        UnixMillis(20),
        "updated_at never moves backwards"
    );

    let contents: Vec<String> = db
        .list_messages(&ConversationId::new("old"))
        .unwrap()
        .into_iter()
        .map(|m| m.content)
        .collect();
    assert_eq!(contents, ["first", "second"]);
}

#[test]
fn stored_content_is_data_not_sql() {
    let mut db = Database::open_in_memory().unwrap();
    db.create_conversation(
        &ConversationId::new("c1"),
        "x'); DROP TABLE messages; --",
        UnixMillis(1),
    )
    .unwrap();
    let hostile = "'); DELETE FROM conversations; --";
    db.append_message(&message("m1", "c1", Role::User, hostile, 2))
        .unwrap();
    assert_eq!(db.list_conversations().unwrap().len(), 1);
    assert_eq!(
        db.list_messages(&ConversationId::new("c1")).unwrap()[0].content,
        hostile
    );
}

#[test]
fn delete_message_removes_only_that_message() {
    let mut db = Database::open_in_memory().unwrap();
    db.create_conversation(&ConversationId::new("c1"), "t", UnixMillis(1))
        .unwrap();
    db.append_message(&message("m1", "c1", Role::User, "q", 2))
        .unwrap();
    db.append_message(&message("m2", "c1", Role::Assistant, "a", 3))
        .unwrap();
    assert!(db.delete_message(&MessageId::new("m2")).unwrap());
    assert!(!db.delete_message(&MessageId::new("m2")).unwrap());
    assert_eq!(
        db.list_messages(&ConversationId::new("c1")).unwrap().len(),
        1
    );
}

#[test]
fn newer_schema_version_is_refused_and_left_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = db_path(&dir);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
    }
    let error = Database::open(&path).unwrap_err();
    assert!(
        matches!(error, MemoryError::SchemaTooNew { .. }),
        "{error:?}"
    );
    let conn = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION + 1);
}

#[tokio::test]
async fn store_runs_queries_off_the_async_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new(Database::open(&db_path(&dir)).unwrap());
    store
        .run(|db| {
            db.create_conversation(&ConversationId::new("c1"), "t", UnixMillis(1))
                .map(|_| ())
        })
        .await
        .unwrap();
    let count = store
        .run(|db| Ok(db.list_conversations()?.len()))
        .await
        .unwrap();
    assert_eq!(count, 1);
}

/// Acceptance: "No `format!` of SQL values". The whole crate bans `format!` so SQL can only be
/// built from constant strings with parameters.
#[test]
fn source_never_formats_sql() {
    fn visit(dir: &Path, offenders: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, offenders);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let source = std::fs::read_to_string(&path).unwrap();
                for (number, line) in source.lines().enumerate() {
                    let code = line.split("//").next().unwrap_or("");
                    if code.contains("format!") || code.contains("write!(") {
                        offenders.push(format!("{}:{}", path.display(), number + 1));
                    }
                }
            }
        }
    }
    let mut offenders = Vec::new();
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut offenders,
    );
    assert!(
        offenders.is_empty(),
        "string formatting in loki-memory: {offenders:?}"
    );
}
