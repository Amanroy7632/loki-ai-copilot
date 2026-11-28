//! Loki desktop shell (P5). The window shows the shared React UI; every command delegates to
//! `loki-runtime` (ARCHITECTURE.md: no business logic in Tauri commands).

// Release builds on Windows must not open a console window next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use tauri::Manager;

fn main() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let state = commands::AppState::open()?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![commands::app_status])
        .run(tauri::generate_context!());
    if let Err(error) = app {
        eprintln!("Loki could not start: {error}");
        std::process::exit(1);
    }
}
