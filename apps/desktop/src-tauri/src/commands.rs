//! Tauri commands. Each one only takes state and arguments, calls `loki-runtime` and returns a
//! generated contract type (`packages/contracts`). Errors cross IPC as plain strings.

use std::sync::Arc;

use loki_core::{AppStatus, Conversation, ConversationId, Message, TurnEvent};
use loki_runtime::{EventSink, Runtime, RuntimeOptions};
use tauri::async_runtime::RwLock;
use tauri::ipc::Channel;

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

/// Wraps an IPC channel as the runtime's event sink. Send errors mean the window went away;
/// the turn carries on and is stored, which is what a closed window should do.
fn sink(channel: Channel<TurnEvent>) -> EventSink {
    Arc::new(move |event| {
        let _ = channel.send(event);
    })
}

/// Sends a message (a new conversation when `conversation_id` is null) and streams the turn.
#[tauri::command]
pub async fn send_message(
    state: tauri::State<'_, AppState>,
    turn_id: String,
    conversation_id: Option<ConversationId>,
    text: String,
    on_event: Channel<TurnEvent>,
) -> Result<(), String> {
    let runtime = state.runtime.read().await;
    runtime
        .chat_turn(&turn_id, conversation_id, Some(text), sink(on_event))
        .await
        .map_err(|error| error.to_string())
}

/// Regenerates the last reply of a conversation and streams it.
#[tauri::command]
pub async fn retry_reply(
    state: tauri::State<'_, AppState>,
    turn_id: String,
    conversation_id: ConversationId,
    on_event: Channel<TurnEvent>,
) -> Result<(), String> {
    let runtime = state.runtime.read().await;
    runtime
        .chat_turn(&turn_id, Some(conversation_id), None, sink(on_event))
        .await
        .map_err(|error| error.to_string())
}

/// Stops a running turn. `false` if it isn't running.
#[tauri::command]
pub async fn stop_reply(
    state: tauri::State<'_, AppState>,
    turn_id: String,
) -> Result<bool, String> {
    Ok(state.runtime.read().await.stop_turn(&turn_id))
}

/// The user's answer to an approval dialog. `false` for unknown, expired or answered IDs.
#[tauri::command]
pub async fn respond_approval(
    state: tauri::State<'_, AppState>,
    approval_id: String,
    approved: bool,
) -> Result<bool, String> {
    Ok(state
        .runtime
        .read()
        .await
        .respond_approval(&approval_id, approved))
}

/// Conversations, most recent first.
#[tauri::command]
pub async fn list_conversations(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Conversation>, String> {
    state
        .runtime
        .read()
        .await
        .list_conversations()
        .await
        .map_err(|error| error.to_string())
}

/// Messages of one conversation, oldest first.
#[tauri::command]
pub async fn get_messages(
    state: tauri::State<'_, AppState>,
    conversation_id: ConversationId,
) -> Result<Vec<Message>, String> {
    state
        .runtime
        .read()
        .await
        .messages(&conversation_id)
        .await
        .map_err(|error| error.to_string())
}
