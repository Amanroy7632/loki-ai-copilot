//! Loki desktop shell (P5). The window shows the shared React UI; every command delegates to
//! `loki-runtime` (ARCHITECTURE.md: no business logic in Tauri commands).

// Release builds on Windows must not open a console window next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod lifecycle;
mod settings;
mod voice;

use tauri::Manager;

fn main() {
    let app = tauri::Builder::default()
        // First: a second process must never open or mutate the runtime/database.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            lifecycle::show(app)
        }))
        // Inert until the explicit Settings toggle. OS state is read through Loki commands.
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("Loki")
                .arg("--autostart")
                .build(),
        )
        // Registered for the Rust-side folder picker only; no capability grants `dialog:` to the
        // webview, so its own JS commands stay denied.
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = commands::AppState::open()?;
            lifecycle::install(app.handle(), &state)?;
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
            settings::get_settings,
            settings::save_settings,
            settings::clear_history,
            settings::list_audit,
            settings::export_data,
            settings::check_provider,
            lifecycle::get_autostart,
            lifecycle::set_autostart,
            voice::get_voice_status,
            voice::watch_voice,
            voice::press_talk,
            voice::release_talk,
            voice::set_voice_muted,
        ])
        .build(tauri::generate_context!());
    match app {
        Ok(app) => app.run(lifecycle::run_event),
        Err(error) => {
            eprintln!("Loki could not start: {error}");
            std::process::exit(1);
        }
    }
}
