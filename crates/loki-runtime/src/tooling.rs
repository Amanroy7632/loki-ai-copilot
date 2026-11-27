//! Wiring for the P3 tool loop: the audit store over `loki-memory`, and the [`ToolHost`] built
//! from config (allowed roots, limits, network mode) and the native platform adapter.

use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use loki_agent::{AuditStore, StoreError, ToolHost};
use loki_core::{Grant, TaskId, TaskState, ToolStatus, UnixMillis};
use loki_memory::MemoryStore;
use loki_policy::{ConsumedApproval, PendingApproval};
use loki_tools::builtin::{FileRoots, FilesState};
use loki_tools::platform::Platform;
use loki_tools::{Executor, default_registry};

use crate::config::Config;
use crate::ids;
use crate::redact::Redactor;

fn store_error(error: impl std::fmt::Display) -> StoreError {
    StoreError(error.to_string())
}

/// [`AuditStore`] over SQLite. Approvals are consumed with a single conditional `UPDATE`.
pub struct MemoryAuditStore(pub MemoryStore);

impl AuditStore for MemoryAuditStore {
    fn create_task(
        &self,
        id: &TaskId,
        request_json: String,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        let id = id.clone();
        async move {
            self.0
                .run(move |db| db.create_task(&id, &request_json, UnixMillis::now()))
                .await
                .map_err(store_error)
        }
        .boxed()
    }

    fn transition(&self, id: &TaskId, to: TaskState) -> BoxFuture<'_, Result<(), StoreError>> {
        let id = id.clone();
        async move {
            self.0
                .run(move |db| db.transition_task(&id, to, UnixMillis::now()))
                .await
                .map_err(store_error)
        }
        .boxed()
    }

    fn record_run(
        &self,
        task: &TaskId,
        tool: &str,
        status: ToolStatus,
        summary: String,
        duration_ms: u64,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        let (task, tool) = (task.clone(), tool.to_owned());
        async move {
            self.0
                .run(move |db| {
                    db.record_tool_run(
                        &ids::new_run_id(),
                        &task,
                        &tool,
                        status,
                        Some(&summary),
                        duration_ms,
                        UnixMillis::now(),
                    )
                })
                .await
                .map_err(store_error)
        }
        .boxed()
    }

    fn record_approval(
        &self,
        pending: &PendingApproval,
        approved: bool,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        let pending = pending.clone();
        async move {
            self.0
                .run(move |db| {
                    db.record_approval(
                        &pending.id,
                        &pending.request_id,
                        &pending.args_hash,
                        approved,
                        pending.expires_at,
                    )
                })
                .await
                .map_err(store_error)
        }
        .boxed()
    }

    fn consume_approval(
        &self,
        pending: &PendingApproval,
    ) -> BoxFuture<'_, Result<Option<ConsumedApproval>, StoreError>> {
        let pending = pending.clone();
        async move {
            let row = self
                .0
                .run(move |db| {
                    db.consume_approval(&pending.id, &pending.args_hash, UnixMillis::now())
                })
                .await
                .map_err(store_error)?;
            Ok(row.map(|row| ConsumedApproval {
                id: row.id,
                request_id: row.request_id,
                request_hash: row.request_hash,
                expires_at: row.expires_at,
                used_at: Some(row.used_at),
            }))
        }
        .boxed()
    }

    fn grants(&self) -> BoxFuture<'_, Result<Vec<Grant>, StoreError>> {
        async move {
            self.0
                .run(|db| db.active_grants())
                .await
                .map_err(store_error)
        }
        .boxed()
    }
}

/// The allowed roots from config, canonicalized. Warnings are for the user.
pub fn file_roots(config: &Config) -> (FileRoots, Vec<String>) {
    FileRoots::new(
        config
            .allowed_roots
            .iter()
            .map(|root| (root.id.clone(), root.path.clone())),
    )
}

/// Builds the tool host, or `None` when tools are disabled in config.
pub fn build_tool_host(
    config: &Config,
    store: &MemoryStore,
    platform: Arc<dyn Platform>,
) -> Result<(Option<Arc<ToolHost>>, Vec<String>), crate::error::RuntimeError> {
    if !config.tools.enabled {
        return Ok((None, Vec::new()));
    }
    let (roots, warnings) = file_roots(config);
    let files = FilesState {
        roots: Arc::new(roots),
        index: Arc::default(),
    };
    let registry = default_registry(files)
        .map_err(|error| crate::error::RuntimeError::Tools(error.to_string()))?;
    let redactor = Redactor::new(config.allowed_roots.iter().map(|root| root.path.clone()));
    let host = ToolHost {
        registry: Arc::new(registry),
        executor: Executor::new(
            usize::try_from(config.tools.concurrency).unwrap_or(1),
            loki_tools::DEFAULT_RESULT_CAP_BYTES,
        ),
        platform,
        audit: Arc::new(MemoryAuditStore(store.clone())),
        network_mode: config.network_mode,
        max_calls_per_turn: usize::try_from(config.tools.max_calls_per_turn).unwrap_or(1),
        timeout: Duration::from_secs(u64::from(config.tools.timeout_secs.max(1))),
        redact: Arc::new(move |text: &str| redactor.redact(text)),
    };
    Ok((Some(Arc::new(host)), warnings))
}
