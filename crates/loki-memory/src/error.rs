//! Storage errors.

/// Why a storage operation failed.
///
/// Messages never include stored content, only identifiers and SQLite's own error text.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    /// SQLite reported an error.
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The database was written by a newer Loki. We refuse to touch it rather than risk damage.
    #[error("database schema version {found} is newer than this build supports ({supported})")]
    SchemaTooNew {
        /// `PRAGMA user_version` found on disk.
        found: i64,
        /// The highest version this build knows.
        supported: i64,
    },
    /// The referenced conversation does not exist.
    #[error("conversation {0} not found")]
    ConversationNotFound(String),
    /// A stored value is not valid for its column (e.g. an unknown role). Indicates corruption.
    #[error("corrupt row in {table}: {detail}")]
    Corrupt {
        /// The table the bad row is in.
        table: &'static str,
        /// What was wrong.
        detail: String,
    },
    /// The connection lock was poisoned by a panic in another thread.
    #[error("database connection is unusable after a panic")]
    Poisoned,
    /// The blocking task running the query could not be joined (e.g. the runtime shut down).
    #[error("database task failed: {0}")]
    Task(String),
}
