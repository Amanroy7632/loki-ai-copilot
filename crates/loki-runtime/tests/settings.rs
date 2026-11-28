//! Settings and data control through the real runtime with the labelled mock provider.

#![allow(clippy::unwrap_used)]

use loki_core::{CapabilityStatus, DataExport, NetworkMode, SettingsProvider};
use loki_runtime::{
    CancellationToken, Config, DenyAllApprovals, ProviderKind, Runtime, RuntimeOptions, config,
};

fn open(dir: &tempfile::TempDir) -> Runtime {
    Runtime::open(RuntimeOptions {
        data_dir: Some(dir.path().into()),
        provider: Some(ProviderKind::Mock),
        ..RuntimeOptions::default()
    })
    .unwrap()
}

#[tokio::test]
async fn mock_settings_apply_immediately_persist_and_report_unavailable_features() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = open(&dir);
    let mut settings = runtime.settings().values;
    settings.provider = SettingsProvider::Ollama;
    settings.model_id = Some(" qwen3:4b ".into());
    settings.network_mode = NetworkMode::ToolsAllowed;
    settings.history_days = Some(30);
    let snapshot = runtime.update_settings(settings).await.unwrap();
    assert_eq!(runtime.provider_name(), "ollama");
    assert_eq!(runtime.model(), Some("qwen3:4b"));
    assert!(matches!(
        snapshot.memory,
        CapabilityStatus::Unavailable { .. }
    ));
    assert!(matches!(
        snapshot.autostart,
        CapabilityStatus::Unavailable { .. }
    ));
    assert!(matches!(
        snapshot.wake_word,
        CapabilityStatus::Unavailable { .. }
    ));
    drop(runtime);
    let runtime = Runtime::open(RuntimeOptions {
        data_dir: Some(dir.path().into()),
        ..RuntimeOptions::default()
    })
    .unwrap();
    assert_eq!(runtime.settings(), snapshot);
}

#[tokio::test]
async fn mock_invalid_settings_leave_disk_and_active_provider_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = open(&dir);
    let initial = runtime.settings();
    let config_before = std::fs::read(runtime.paths().config_file()).unwrap();
    for model in [None, Some("gpt-oss:20b-cloud"), Some("bad\nmodel")] {
        let mut value = initial.values.clone();
        value.provider = SettingsProvider::Ollama;
        value.model_id = model.map(str::to_owned);
        assert!(runtime.update_settings(value).await.is_err());
        assert_eq!(runtime.settings(), initial);
        assert_eq!(
            std::fs::read(runtime.paths().config_file()).unwrap(),
            config_before
        );
    }
    for days in [0, 36501] {
        let mut value = initial.values.clone();
        value.history_days = Some(days);
        assert!(runtime.update_settings(value).await.is_err());
        assert_eq!(runtime.settings(), initial);
    }
}

#[tokio::test]
async fn mock_export_file_is_json_and_never_overwrites_existing_files_then_clear_persists() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = open(&dir);
    let conversation = runtime.start_conversation("private message").await.unwrap();
    runtime
        .send(
            &conversation.id,
            "private message",
            CancellationToken::new(),
            &DenyAllApprovals,
            |_| {},
        )
        .await
        .unwrap();
    let path = dir.path().join("export.json");
    runtime.export_data_to(path.clone()).await.unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let export: DataExport = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(export.history[0].messages.len(), 2);
    assert_eq!(
        export.history[0].messages[1].content,
        "Mock reply: private message"
    );
    assert!(runtime.export_data_to(path.clone()).await.is_err());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    let config = runtime.paths().config_file();
    let before = std::fs::read(&config).unwrap();
    assert!(runtime.export_data_to(config.clone()).await.is_err());
    assert_eq!(std::fs::read(config).unwrap(), before);
    assert_eq!(runtime.clear_history().await.unwrap(), 1);
    assert!(runtime.messages(&conversation.id).await.is_err());
    assert!(runtime.export_data().await.unwrap().history.is_empty());
    drop(runtime);
    assert!(open(&dir).list_conversations().await.unwrap().is_empty());
}

#[tokio::test]
async fn mock_retention_runs_on_save_and_on_startup() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = open(&dir);
    let path = runtime.paths().database_file();
    let db = loki_memory::Database::open(&path).unwrap();
    let old = loki_core::ConversationId::new("old");
    db.create_conversation(&old, "expired", loki_core::UnixMillis(1))
        .unwrap();
    let mut settings = runtime.settings().values;
    settings.history_days = Some(1);
    runtime.update_settings(settings).await.unwrap();
    assert!(runtime.list_conversations().await.unwrap().is_empty());
    db.create_conversation(&old, "expired", loki_core::UnixMillis(1))
        .unwrap();
    drop(db);
    drop(runtime);
    assert!(open(&dir).list_conversations().await.unwrap().is_empty());
    let saved: Config = config::load_or_init(&open(&dir).paths().config_file())
        .unwrap()
        .config;
    assert_eq!(saved.retention.history_days, Some(1));
}

#[tokio::test]
async fn mock_retention_failure_reports_that_settings_were_already_saved() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = open(&dir);
    let db = loki_memory::Database::open(&runtime.paths().database_file()).unwrap();
    db.create_conversation(
        &loki_core::ConversationId::new("old"),
        "expired",
        loki_core::UnixMillis(1),
    )
    .unwrap();
    db.connection().execute_batch("CREATE TRIGGER fail_delete BEFORE DELETE ON conversations BEGIN SELECT RAISE(ABORT, 'injected deletion failure'); END;").unwrap();
    let mut settings = runtime.settings().values;
    settings.history_days = Some(1);
    settings.network_mode = NetworkMode::ToolsAllowed;
    let error = runtime
        .update_settings(settings.clone())
        .await
        .unwrap_err()
        .data_control_error();
    assert!(error.state_changed);
    assert!(error.message.contains("settings were saved"));
    assert_eq!(runtime.settings().values, settings);
    assert_eq!(
        config::load_or_init(&runtime.paths().config_file())
            .unwrap()
            .config
            .retention
            .history_days,
        Some(1)
    );
    // An ordinary validation failure never tells the UI to discard a running conversation.
    settings.history_days = Some(0);
    assert!(
        !runtime
            .update_settings(settings)
            .await
            .unwrap_err()
            .data_control_error()
            .state_changed
    );
}
