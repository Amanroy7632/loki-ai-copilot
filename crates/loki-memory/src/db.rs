//! Opening the database and applying migrations (ADR-0007).
//!
//! Migrations are the numbered files in `/migrations`, embedded at compile time. The applied
//! version is `PRAGMA user_version`. Each pending migration runs in its own transaction together
//! with the `user_version` bump, so a crash leaves the DB at a consistent version.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, TransactionBehavior};

use crate::error::MemoryError;

/// The embedded migrations, in order. Index `i` holds migration number `i + 1`.
///
/// **Forward-only:** append new files; never edit or reorder existing entries.
const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_init",
        include_str!("../../../migrations/0001_init.sql"),
    ),
    (
        "0002_message_stopped",
        include_str!("../../../migrations/0002_message_stopped.sql"),
    ),
];

/// The schema version this build produces (the number of migrations).
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

/// How long a writer waits for a lock held by another connection before failing.
const BUSY_TIMEOUT: Duration = Duration::from_millis(5_000);

/// An open, migrated SQLite database.
///
/// Synchronous by design: call it from a blocking thread. Async code uses
/// [`crate::MemoryStore`], which does that for you.
#[derive(Debug)]
pub struct Database {
    pub(crate) conn: Connection,
}

impl Database {
    /// Opens (or creates) the database file and brings the schema up to [`SCHEMA_VERSION`].
    ///
    /// The parent directory must exist. Opening is idempotent: re-opening a current DB
    /// applies nothing.
    ///
    /// Before applying migrations to an **existing** database, it is copied to
    /// `<file>.bak-v<old version>` with `VACUUM INTO` (DATA_MODEL "Recovery"). If that backup
    /// already exists (an earlier attempt), it is kept as is.
    pub fn open(path: &Path) -> Result<Self, MemoryError> {
        let conn = Connection::open(path)?;
        Self::from_connection(conn, true, Some(path))
    }

    /// An in-memory database, for tests. WAL doesn't apply to in-memory databases.
    pub fn open_in_memory() -> Result<Self, MemoryError> {
        let conn = Connection::open_in_memory()?;
        Self::from_connection(conn, false, None)
    }

    fn from_connection(
        mut conn: Connection,
        wal: bool,
        file: Option<&Path>,
    ) -> Result<Self, MemoryError> {
        configure(&conn, wal)?;
        if let Some(file) = file {
            backup_before_migrating(&conn, file)?;
        }
        migrate(&mut conn)?;
        Ok(Self { conn })
    }

    /// The applied schema version (`PRAGMA user_version`).
    pub fn schema_version(&self) -> Result<i64, MemoryError> {
        user_version(&self.conn)
    }

    /// Read access to the raw connection, for diagnostics and tests. Use the repository
    /// methods for data access.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

/// Connection settings from `docs/DATA_MODEL.md`.
fn configure(conn: &Connection, wal: bool) -> Result<(), MemoryError> {
    conn.pragma_update(None, "foreign_keys", true)?;
    if wal {
        // journal_mode returns the resulting mode as a row, so it must be queried, not executed.
        let mode: String =
            conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(MemoryError::Corrupt {
                table: "pragma",
                detail: String::from("could not enable WAL journal mode"),
            });
        }
    }
    conn.busy_timeout(BUSY_TIMEOUT)?;
    Ok(())
}

fn user_version(conn: &Connection) -> Result<i64, MemoryError> {
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

/// Applies every migration newer than the DB's `user_version`.
fn migrate(conn: &mut Connection) -> Result<(), MemoryError> {
    let current = user_version(conn)?;
    if current > SCHEMA_VERSION {
        return Err(MemoryError::SchemaTooNew {
            found: current,
            supported: SCHEMA_VERSION,
        });
    }
    for (index, (_name, sql)) in MIGRATIONS.iter().enumerate() {
        let version = i64::try_from(index).unwrap_or(i64::MAX).saturating_add(1);
        if version <= current {
            continue;
        }
        // IMMEDIATE takes the write lock up front, so two processes can't both apply it.
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

/// The backup path for a database at `version`: `<file>.bak-v<version>`.
pub fn backup_path(file: &Path, version: i64) -> PathBuf {
    let mut name: OsString = file.as_os_str().to_owned();
    name.push(".bak-v");
    name.push(version.to_string());
    PathBuf::from(name)
}

/// Copies an existing, older database aside before any migration touches it.
///
/// Nothing is copied for a brand-new file (version 0) or a current one. `VACUUM INTO` writes a
/// consistent, compacted snapshot even in WAL mode. The target path is a bound parameter.
fn backup_before_migrating(conn: &Connection, file: &Path) -> Result<(), MemoryError> {
    let current = user_version(conn)?;
    if current == 0 || current >= SCHEMA_VERSION {
        return Ok(());
    }
    let target = backup_path(file, current);
    if target.exists() {
        return Ok(());
    }
    let target_text = target.to_str().ok_or(MemoryError::Corrupt {
        table: "backup",
        detail: String::from("database path is not valid UTF-8"),
    })?;
    conn.execute("VACUUM INTO ?1", [target_text])?;
    Ok(())
}
