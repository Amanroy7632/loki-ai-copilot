//! Native desktop shell wiring. Cancellation and autostart policy live in loki-runtime.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use loki_core::AutostartStatus;
use loki_runtime::autostart::{AutostartBackend, AutostartService};
use tauri::{
    Manager,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
use tauri_plugin_autostart::ManagerExt;

use crate::commands::AppState;

struct NativeAutostart(tauri::AppHandle);

impl AutostartBackend for NativeAutostart {
    fn is_enabled(&self) -> Result<bool, String> {
        self.0
            .autolaunch()
            .is_enabled()
            .map_err(|error| error.to_string())
    }
    fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        let manager = self.0.autolaunch();
        if enabled {
            manager.enable()
        } else {
            manager.disable()
        }
        .map_err(|error| error.to_string())
    }
}

pub struct DesktopLifecycle {
    pub autostart: AutostartService,
    control: loki_runtime::LifecycleControl,
    voice: loki_runtime::voice::VoiceControl,
    quitting: AtomicBool,
    exit_ready: AtomicBool,
}

/// Construct controls without changing OS login registration or opening the microphone.
pub fn install(app: &tauri::AppHandle, state: &AppState) -> Result<(), Box<dyn std::error::Error>> {
    let unavailable = if cfg!(debug_assertions) {
        Some(
            "Start at login is available in installed release builds, not development builds."
                .into(),
        )
    } else if std::env::var_os(loki_runtime::DATA_DIR_ENV).is_some()
        || std::env::var_os(loki_runtime::config::PROVIDER_ENV).is_some()
    {
        Some("Start at login is unavailable with session data/provider overrides. Launch the installed app normally.".into())
    } else {
        None
    };
    app.manage(DesktopLifecycle {
        autostart: AutostartService::new(NativeAutostart(app.clone()), unavailable),
        control: state.control.clone(),
        voice: state.voice.clone(),
        quitting: AtomicBool::new(false),
        exit_ready: AtomicBool::new(false),
    });
    let toggle = MenuItem::with_id(app, "toggle", "Show / hide Loki", true, None::<&str>)?;
    let mute = MenuItem::with_id(app, "mute", "Mute voice", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Emergency stop", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Loki", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &mute, &stop, &quit])?;
    let icon = app
        .default_window_icon()
        .ok_or("missing tray icon")?
        .clone();
    let tray = TrayIconBuilder::with_id("loki-tray")
        .icon(icon)
        .tooltip("Loki — microphone off")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => {
                if let Some(window) = app.get_webview_window("main") {
                    if window.is_visible().unwrap_or(false) {
                        report(window.hide());
                    } else {
                        show(app);
                    }
                }
            }
            "mute" => {
                let voice = &app.state::<DesktopLifecycle>().voice;
                voice.set_muted(!voice.status().muted);
            }
            "stop" => app.state::<DesktopLifecycle>().control.emergency_stop(),
            "quit" => request_quit(app),
            _ => {}
        })
        .build(app)?;
    // The tray follows the runtime's voice state: its label and tooltip always say whether the
    // microphone is on, including changes made from the window (P5-T08).
    state.voice.subscribe(std::sync::Arc::new(move |status| {
        let label = if status.muted {
            "Unmute voice"
        } else {
            "Mute voice"
        };
        let tooltip = if status.mic_open {
            "Loki — microphone ON"
        } else if status.muted {
            "Loki — voice muted"
        } else {
            "Loki — microphone off"
        };
        report(mute.set_text(label));
        report(tray.set_tooltip(Some(tooltip)));
        true
    }));
    if !std::env::args_os().any(|arg| arg == "--autostart") {
        show(app);
    }
    Ok(())
}

pub fn show(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        report(window.show());
        report(window.unminimize());
        report(window.set_focus());
    }
}

fn report(result: tauri::Result<()>) {
    if let Err(error) = result {
        eprintln!("Loki window operation failed: {error}");
    }
}

pub fn request_quit(app: &tauri::AppHandle) {
    let state = app.state::<DesktopLifecycle>();
    if state.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    state.control.begin_shutdown();
    let control = state.control.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if !control.wait_for_idle(Duration::from_secs(2)).await {
            eprintln!(
                "Loki shutdown cleanup timed out; unfinished tasks will be recovered on startup"
            );
        }
        app.state::<DesktopLifecycle>()
            .exit_ready
            .store(true, Ordering::SeqCst);
        app.exit(0);
    });
}

pub fn run_event(app: &tauri::AppHandle, event: tauri::RunEvent) {
    match event {
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } if label == "main" => {
            api.prevent_close();
            if let Some(window) = app.get_webview_window("main") {
                report(window.hide());
            }
        }
        tauri::RunEvent::ExitRequested { api, .. } => {
            if let Some(state) = app.try_state::<DesktopLifecycle>()
                && !state.exit_ready.load(Ordering::SeqCst)
            {
                api.prevent_exit();
                request_quit(app);
            }
        }
        // Tauri exposes this event only on macOS; it is shell wiring, not an OS service adapter.
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => show(app),
        _ => {}
    }
}

#[tauri::command]
pub async fn get_autostart(
    state: tauri::State<'_, DesktopLifecycle>,
) -> Result<AutostartStatus, String> {
    Ok(state.autostart.status().await)
}

#[tauri::command]
pub async fn set_autostart(
    state: tauri::State<'_, DesktopLifecycle>,
    enabled: bool,
) -> Result<AutostartStatus, String> {
    state.autostart.set_enabled(enabled).await
}
