//! Tauri commands. Each one only takes state and arguments, calls `loki-runtime` and returns a
//! generated contract type (`packages/contracts`). Errors cross IPC as plain strings.

use loki_core::AppStatus;
use loki_runtime::{Runtime, RuntimeOptions};
use tauri::async_runtime::RwLock;

/// Shared application state: the runtime and the startup note (e.g. "no model selected").
pub struct AppState {
    runtime: RwLock<Runtime>,
    note: Option<String>,
}

impl AppState {
    /// Opens the runtime in the per-user data dir (or `LOKI_DATA_DIR`), with file logging.
    /// Blocking: called once from Tauri's setup hook before the window loads.
    pub fn open() -> Result<Self, loki_runtime::RuntimeError> {
        let (runtime, note) = Runtime::open_for_app(RuntimeOptions {
            init_logging: true,
            ..RuntimeOptions::default()
        })?;
        Ok(Self {
            runtime: RwLock::new(runtime),
            note,
        })
    }
}

/// Provider, model, readiness and any startup note, for the status area.
#[tauri::command]
pub async fn app_status(state: tauri::State<'_, AppState>) -> Result<AppStatus, String> {
    let runtime = state.runtime.read().await;
    Ok(runtime.status(state.note.as_deref()).await)
}
