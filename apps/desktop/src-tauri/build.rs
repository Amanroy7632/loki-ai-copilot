//! Declares Loki's own commands in the Tauri app manifest. With a manifest, a command is
//! callable only when a capability file grants it (least privilege, P5-T09).

fn main() {
    let attributes =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "app_status",
            "send_message",
            "retry_reply",
            "stop_reply",
            "respond_approval",
            "list_conversations",
            "get_messages",
            "rename_conversation",
            "delete_conversation",
            "list_permissions",
            "list_roots",
            "grant_permission",
            "revoke_permission",
            "add_root",
            "remove_root",
            "get_settings",
            "save_settings",
            "clear_history",
            "list_audit",
            "export_data",
            "get_autostart",
            "set_autostart",
            "get_voice_status",
            "watch_voice",
            "press_talk",
            "release_talk",
            "set_voice_muted",
        ]));
    if let Err(error) = tauri_build::try_build(attributes) {
        panic!("tauri build configuration failed: {error:#}");
    }
}
