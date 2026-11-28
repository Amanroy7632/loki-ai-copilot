//! Thin voice commands (P5-T08). The state machine and its rules live in `loki-runtime::voice`;
//! these only forward. They use the runtime's `VoiceControl` handle, not the runtime lock, so
//! mute works even while a reply is streaming.

use loki_core::VoiceStatus;
use tauri::ipc::Channel;

use crate::commands::AppState;

/// The current voice status.
#[tauri::command]
pub fn get_voice_status(state: tauri::State<'_, AppState>) -> VoiceStatus {
    state.voice.status()
}

/// Streams every voice status change to the window (starting with the current one), so the
/// listening indicator follows the runtime, including changes made from the tray.
#[tauri::command]
pub fn watch_voice(
    state: tauri::State<'_, AppState>,
    on_status: Channel<VoiceStatus>,
) -> VoiceStatus {
    state
        .voice
        .subscribe(std::sync::Arc::new(move |status: &VoiceStatus| {
            on_status.send(status.clone()).is_ok()
        }))
}

/// Push-to-talk pressed. Refused (with the reason) while muted or unavailable.
#[tauri::command]
pub fn press_talk(state: tauri::State<'_, AppState>) -> Result<VoiceStatus, String> {
    state.voice.press_talk().map_err(|error| error.to_string())
}

/// Push-to-talk released.
#[tauri::command]
pub fn release_talk(state: tauri::State<'_, AppState>) -> Result<VoiceStatus, String> {
    state
        .voice
        .release_talk()
        .map_err(|error| error.to_string())
}

/// Mute or unmute voice. Muting always succeeds and turns the microphone off at once.
#[tauri::command]
pub fn set_voice_muted(state: tauri::State<'_, AppState>, muted: bool) -> VoiceStatus {
    state.voice.set_muted(muted)
}
