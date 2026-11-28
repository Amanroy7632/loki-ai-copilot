//! Loki desktop shell (P5). The window shows the shared React UI; every command delegates to
//! `loki-runtime` (ARCHITECTURE.md: no business logic in Tauri commands).

// Release builds on Windows must not open a console window next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use tauri::Manager;

fn main() {
    let app = tauri::Builder::default()
        // Registered for the Rust-side folder picker only; no capability grants `dialog:` to the
        // webview, so its own JS commands stay denied.
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = commands::AppState::open()?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::send_message,
            commands::retry_reply,
            commands::stop_reply,
            commands::respond_approval,
            commands::list_conversations,
            commands::get_messages,
            commands::rename_conversation,
            commands::delete_conversation,
            commands::list_permissions,
            commands::list_roots,
            commands::grant_permission,
            commands::revoke_permission,
            commands::add_root,
            commands::remove_root,
        ])
        .run(tauri::generate_context!());
    if let Err(error) = app {
        eprintln!("Loki could not start: {error}");
        std::process::exit(1);
    }
}
