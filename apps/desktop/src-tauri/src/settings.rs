//! Thin settings commands. File destinations come only from the native save dialog.

use loki_core::{AuditEntry, DataControlError, DesktopSettings, SettingsSnapshot};
use tauri_plugin_dialog::DialogExt;

use crate::commands::AppState;

const BUSY: &str = "Loki is busy. Stop the reply and try again.";

fn busy() -> DataControlError {
    DataControlError {
        message: BUSY.into(),
        state_changed: false,
    }
}

#[tauri::command]
pub async fn get_settings(state: tauri::State<'_, AppState>) -> Result<SettingsSnapshot, String> {
    Ok(state.runtime.read().await.settings())
}

#[tauri::command]
pub async fn save_settings(
    state: tauri::State<'_, AppState>,
    settings: DesktopSettings,
) -> Result<SettingsSnapshot, DataControlError> {
    let mut runtime = state.runtime.try_write().map_err(|_| busy())?;
    let result = runtime
        .update_settings(settings)
        .await
        .map_err(|error| error.data_control_error());
    // On a retention cleanup error the configuration may already be saved; startup notes
    // would be misleading. The frontend refreshes the effective settings even on failure.
    if result
        .as_ref()
        .err()
        .is_none_or(|error| error.state_changed)
    {
        *state.note.write().await = None;
    }
    result
}

#[tauri::command]
pub async fn clear_history(state: tauri::State<'_, AppState>) -> Result<usize, DataControlError> {
    state
        .runtime
        .try_write()
        .map_err(|_| busy())?
        .clear_history()
        .await
        .map_err(|error| error.data_control_error())
}

#[tauri::command]
pub async fn list_audit(state: tauri::State<'_, AppState>) -> Result<Vec<AuditEntry>, String> {
    state
        .runtime
        .read()
        .await
        .audit_entries()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn export_data(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let picker = app
        .dialog()
        .file()
        .set_title("Export Loki history to a new JSON file")
        .set_file_name("loki-history.json")
        .add_filter("JSON", &["json"]);
    let picked = tauri::async_runtime::spawn_blocking(move || picker.blocking_save_file())
        .await
        .map_err(|error| error.to_string())?;
    let Some(picked) = picked else {
        return Ok(false);
    };
    let path = picked.into_path().map_err(|error| error.to_string())?;
    state
        .runtime
        .read()
        .await
        .export_data_to(path)
        .await
        .map_err(|error| error.to_string())?;
    Ok(true)
}

/// Checks the local model server: reachable, installed models (no cloud models) and the model
/// to preselect. Talks only to the configured loopback Ollama; never downloads.
#[tauri::command]
pub async fn check_provider(
    state: tauri::State<'_, AppState>,
) -> Result<loki_core::ProviderCheck, String> {
    Ok(state.runtime.read().await.check_provider().await)
}
