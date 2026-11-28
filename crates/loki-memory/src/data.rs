//! History export and deletion. Personal memory/FTS tables arrive in P7.

use loki_core::{DataExport, ExportConversation, UnixMillis};

use crate::{Database, MemoryError};

impl Database {
    /// Exports one consistent read snapshot, including stopped replies, without audit/config.
    pub fn export_data(&self, now: UnixMillis) -> Result<DataExport, MemoryError> {
        let tx = self.conn.unchecked_transaction()?;
        let history = self
            .list_conversations()?
            .into_iter()
            .map(|conversation| {
                Ok(ExportConversation {
                    messages: self.list_messages(&conversation.id)?,
                    conversation,
                })
            })
            .collect::<Result<Vec<_>, MemoryError>>()?;
        tx.commit()?;
        Ok(DataExport {
            version: 1,
            exported_at: now,
            history,
            memory_available: false,
            memories: Vec::new(),
        })
    }

    /// Deletes all history, or conversations last active strictly before `before`.
    /// Foreign keys cascade messages. Audit and permissions are separate and preserved.
    /// Compacts the database and truncates its WAL; external exports/backups are not erased.
    pub fn clear_history(&self, before: Option<UnixMillis>) -> Result<usize, MemoryError> {
        self.conn.pragma_update(None, "secure_delete", true)?;
        let count = self.conn.execute(
            "DELETE FROM conversations WHERE ?1 IS NULL OR updated_at < ?1",
            [before.map(|time| time.0)],
        )?;
        if count > 0 || before.is_none() {
            self.conn
                .execute_batch("VACUUM")
                .map_err(|error| MemoryError::HistoryCleanup(error.to_string()))?;
            let busy: i64 = self
                .conn
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
                .map_err(|error| MemoryError::HistoryCleanup(error.to_string()))?;
            if busy != 0 {
                return Err(MemoryError::HistoryCleanup(
                    "another database reader prevented WAL cleanup".into(),
                ));
            }
        }
        Ok(count)
    }

    /// Enforces the saved retention policy at startup and when the user saves settings.
    pub fn apply_retention(
        &self,
        days: Option<u32>,
        now: UnixMillis,
    ) -> Result<usize, MemoryError> {
        match days {
            None => Ok(0),
            Some(days) => self.clear_history(Some(UnixMillis(
                now.0.saturating_sub(i64::from(days) * 86_400_000),
            ))),
        }
    }
}
