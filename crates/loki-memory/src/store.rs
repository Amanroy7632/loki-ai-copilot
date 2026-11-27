//! Async access to the [`Database`] without blocking the async runtime.

use std::sync::{Arc, Mutex};

use crate::db::Database;
use crate::error::MemoryError;

/// A shareable handle that runs database work on Tokio's blocking pool.
///
/// The lock is taken **inside** the blocking closure and released before it returns, so no
/// guard or connection is ever held across an `.await` (ARCHITECTURE.md concurrency rules).
#[derive(Debug, Clone)]
pub struct MemoryStore {
    db: Arc<Mutex<Database>>,
}

impl MemoryStore {
    /// Wraps an open database.
    pub fn new(db: Database) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
        }
    }

    /// Runs `work` against the database on a blocking thread and returns its result.
    ///
    /// Must be called from within a Tokio runtime.
    pub async fn run<T, F>(&self, work: F) -> Result<T, MemoryError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Database) -> Result<T, MemoryError> + Send + 'static,
    {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            let mut guard = db.lock().map_err(|_| MemoryError::Poisoned)?;
            work(&mut guard)
        })
        .await
        .map_err(|error| MemoryError::Task(error.to_string()))?
    }
}
