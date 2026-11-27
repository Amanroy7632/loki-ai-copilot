//! Repositories for `tasks`, `tool_runs`, `approvals` and `permissions` (P3-T03, P3-T04).
//!
//! Audit tables are separate from conversations. Callers store **redacted** request JSON and
//! result summaries only; this layer never sees raw tool output.

use loki_core::{
    ApprovalId, Capability, Grant, RequestId, TaskId, TaskState, ToolStatus, UnixMillis,
};
use rusqlite::{OptionalExtension, params};

use crate::db::Database;
use crate::error::MemoryError;

/// One row of `tasks`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRow {
    /// Task ID.
    pub id: TaskId,
    /// Current state.
    pub state: TaskState,
    /// Redacted request JSON.
    pub request_json: String,
    /// Created, UTC.
    pub created_at: UnixMillis,
    /// Last state change, UTC.
    pub updated_at: UnixMillis,
}

/// One row of `tool_runs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRunRow {
    /// Run ID.
    pub id: String,
    /// The task it belongs to.
    pub task_id: TaskId,
    /// Tool name.
    pub tool: String,
    /// Final status (`ToolStatus::as_str`).
    pub status: String,
    /// Redacted summary, never raw output.
    pub redacted_result: Option<String>,
    /// Wall-clock duration.
    pub duration_ms: i64,
    /// When it finished, UTC.
    pub created_at: UnixMillis,
}

/// Filter for [`Database::list_tool_runs`].
#[derive(Debug, Clone, Default)]
pub struct ToolRunFilter {
    /// Only this tool.
    pub tool: Option<String>,
    /// Only runs at or after this instant.
    pub since: Option<UnixMillis>,
    /// At most this many rows (newest first). Defaults to 100.
    pub limit: Option<u32>,
}

/// An approval read back right after it was atomically marked used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumedApprovalRow {
    /// Approval ID.
    pub id: ApprovalId,
    /// The request it belongs to.
    pub request_id: RequestId,
    /// The bound hash.
    pub request_hash: String,
    /// Expiry.
    pub expires_at: UnixMillis,
    /// When it was consumed.
    pub used_at: UnixMillis,
}

fn task_state(value: &str) -> Result<TaskState, MemoryError> {
    TaskState::parse(value).ok_or_else(|| MemoryError::Corrupt {
        table: "tasks",
        detail: String::from("unknown task state"),
    })
}

impl Database {
    // ----------------------------------------------------------------------------- tasks

    /// Inserts a task in state `Queued`.
    pub fn create_task(
        &self,
        id: &TaskId,
        request_json: &str,
        now: UnixMillis,
    ) -> Result<(), MemoryError> {
        self.conn.execute(
            "INSERT INTO tasks (id, state, request_json, created_at, updated_at) VALUES (?1, 'queued', ?2, ?3, ?3)",
            params![id.as_str(), request_json, now.0],
        )?;
        Ok(())
    }

    /// One task.
    pub fn get_task(&self, id: &TaskId) -> Result<Option<TaskRow>, MemoryError> {
        let row = self
            .conn
            .query_row(
                "SELECT id, state, request_json, created_at, updated_at FROM tasks WHERE id = ?1",
                params![id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(id, state, request_json, created_at, updated_at)| {
            Ok(TaskRow {
                id: TaskId::new(id),
                state: task_state(&state)?,
                request_json,
                created_at: UnixMillis(created_at),
                updated_at: UnixMillis(updated_at),
            })
        })
        .transpose()
    }

    /// Moves a task to `to`, only if `TaskState::can_transition_to` allows it. The update is
    /// conditional on the state read, so a concurrent change is detected rather than overwritten.
    pub fn transition_task(
        &self,
        id: &TaskId,
        to: TaskState,
        now: UnixMillis,
    ) -> Result<(), MemoryError> {
        let current = self
            .get_task(id)?
            .ok_or_else(|| MemoryError::TaskNotFound(id.to_string()))?
            .state;
        if !current.can_transition_to(to) {
            return Err(MemoryError::IllegalTransition {
                from: current.as_str(),
                to: to.as_str(),
            });
        }
        let changed = self.conn.execute(
            "UPDATE tasks SET state = ?1, updated_at = ?2 WHERE id = ?3 AND state = ?4",
            params![to.as_str(), now.0, id.as_str(), current.as_str()],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(MemoryError::IllegalTransition {
                from: current.as_str(),
                to: to.as_str(),
            })
        }
    }

    /// Startup recovery: every non-terminal task becomes `Interrupted`. Nothing is re-run.
    /// Returns how many tasks were marked.
    pub fn recover_interrupted_tasks(&self, now: UnixMillis) -> Result<usize, MemoryError> {
        Ok(self.conn.execute(
            "UPDATE tasks SET state = 'interrupted', updated_at = ?1 \
             WHERE state IN ('queued', 'running', 'awaiting_approval')",
            params![now.0],
        )?)
    }

    // ------------------------------------------------------------------------- tool_runs

    /// Writes the audit row for a finished run.
    #[allow(clippy::too_many_arguments)]
    pub fn record_tool_run(
        &self,
        id: &str,
        task: &TaskId,
        tool: &str,
        status: ToolStatus,
        redacted_result: Option<&str>,
        duration_ms: u64,
        now: UnixMillis,
    ) -> Result<(), MemoryError> {
        self.conn.execute(
            "INSERT INTO tool_runs (id, task_id, tool, status, redacted_result, duration_ms, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                task.as_str(),
                tool,
                status.as_str(),
                redacted_result,
                i64::try_from(duration_ms).unwrap_or(i64::MAX),
                now.0
            ],
        )?;
        Ok(())
    }

    /// Audit rows, newest first.
    pub fn list_tool_runs(&self, filter: &ToolRunFilter) -> Result<Vec<ToolRunRow>, MemoryError> {
        let mut statement = self.conn.prepare(
            "SELECT id, task_id, tool, status, redacted_result, duration_ms, created_at FROM tool_runs \
             WHERE (?1 IS NULL OR tool = ?1) AND (?2 IS NULL OR created_at >= ?2) \
             ORDER BY created_at DESC, rowid DESC LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![
                filter.tool,
                filter.since.map(|s| s.0),
                filter.limit.unwrap_or(100)
            ],
            tool_run_from_row,
        )?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Audit rows of one task, oldest first.
    pub fn tool_runs_for_task(&self, task: &TaskId) -> Result<Vec<ToolRunRow>, MemoryError> {
        let mut statement = self.conn.prepare(
            "SELECT id, task_id, tool, status, redacted_result, duration_ms, created_at FROM tool_runs \
             WHERE task_id = ?1 ORDER BY created_at ASC, rowid ASC",
        )?;
        let rows = statement.query_map(params![task.as_str()], tool_run_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // ------------------------------------------------------------------------- approvals

    /// Records the user's answer. Only `approved` rows can ever be consumed.
    pub fn record_approval(
        &self,
        id: &ApprovalId,
        request_id: &RequestId,
        request_hash: &str,
        approved: bool,
        expires_at: UnixMillis,
    ) -> Result<(), MemoryError> {
        self.conn.execute(
            "INSERT INTO approvals (id, request_id, request_hash, decision, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id.as_str(),
                request_id.as_str(),
                request_hash,
                if approved { "approved" } else { "rejected" },
                expires_at.0
            ],
        )?;
        Ok(())
    }

    /// Consumes an approval **once**: only if it is approved, unused, unexpired and bound to
    /// `request_hash`. The single conditional `UPDATE` makes reuse impossible even under races.
    /// Returns `None` if any condition fails (SEC-05).
    pub fn consume_approval(
        &self,
        id: &ApprovalId,
        request_hash: &str,
        now: UnixMillis,
    ) -> Result<Option<ConsumedApprovalRow>, MemoryError> {
        Ok(self
            .conn
            .query_row(
                "UPDATE approvals SET used_at = ?1 \
                 WHERE id = ?2 AND request_hash = ?3 AND decision = 'approved' AND used_at IS NULL AND expires_at > ?1 \
                 RETURNING id, request_id, request_hash, expires_at, used_at",
                params![now.0, id.as_str(), request_hash],
                |row| {
                    Ok(ConsumedApprovalRow {
                        id: ApprovalId::new(row.get::<_, String>(0)?),
                        request_id: RequestId::new(row.get::<_, String>(1)?),
                        request_hash: row.get(2)?,
                        expires_at: UnixMillis(row.get(3)?),
                        used_at: UnixMillis(row.get(4)?),
                    })
                },
            )
            .optional()?)
    }

    // ----------------------------------------------------------------------- permissions

    /// Grants `capability` in `scope` (idempotent: an active identical grant is kept).
    pub fn grant(&self, id: &str, grant: &Grant, now: UnixMillis) -> Result<(), MemoryError> {
        self.conn.execute(
            "INSERT INTO permissions (id, capability, scope, granted_at) \
             SELECT ?1, ?2, ?3, ?4 WHERE NOT EXISTS ( \
               SELECT 1 FROM permissions WHERE capability = ?2 AND scope = ?3 AND revoked_at IS NULL)",
            params![id, grant.capability.as_str(), grant.scope, now.0],
        )?;
        Ok(())
    }

    /// Revokes active grants matching exactly. Returns how many were revoked.
    pub fn revoke(&self, grant: &Grant, now: UnixMillis) -> Result<usize, MemoryError> {
        Ok(self.conn.execute(
            "UPDATE permissions SET revoked_at = ?1 WHERE capability = ?2 AND scope = ?3 AND revoked_at IS NULL",
            params![now.0, grant.capability.as_str(), grant.scope],
        )?)
    }

    /// Active grants. Unknown capability names (from a newer build) are skipped, never widened.
    pub fn active_grants(&self) -> Result<Vec<Grant>, MemoryError> {
        let mut statement = self
            .conn
            .prepare("SELECT capability, scope FROM permissions WHERE revoked_at IS NULL ORDER BY capability, scope")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut grants = Vec::new();
        for row in rows {
            let (capability, scope) = row?;
            if let Some(capability) = Capability::parse(&capability) {
                grants.push(Grant { capability, scope });
            }
        }
        Ok(grants)
    }
}

fn tool_run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ToolRunRow> {
    Ok(ToolRunRow {
        id: row.get(0)?,
        task_id: TaskId::new(row.get::<_, String>(1)?),
        tool: row.get(2)?,
        status: row.get(3)?,
        redacted_result: row.get(4)?,
        duration_ms: row.get(5)?,
        created_at: UnixMillis(row.get(6)?),
    })
}
