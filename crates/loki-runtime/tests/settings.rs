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
    // Save the mock explicitly, so switching to Ollama below is a real change of provider.
    let mut mock = runtime.settings().values;
    mock.provider = SettingsProvider::Mock;
    let initial = runtime.update_settings(mock).await.unwrap();
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

/// A per-run provider override (`LOKI_PROVIDER=mock`, `--provider mock`, or the app's labelled
/// mock fallback before a model is chosen) is not a setting. Saving an unrelated setting must
/// not write it to disk.
#[tokio::test]
async fn mock_saving_settings_never_persists_a_session_provider_override() {
    let dir = tempfile::tempdir().unwrap();
    open(&dir).use_model("bonsai-2:4b").unwrap();

    let mut runtime = open(&dir); // session override: mock
    let mut values = runtime.settings().values;
    assert_eq!(
        values.provider,
        SettingsProvider::Ollama,
        "shows the saved provider"
    );
    assert_eq!(values.model_id.as_deref(), Some("bonsai-2:4b"));
    values.history_days = Some(30);
    runtime.update_settings(values).await.unwrap();
    assert_eq!(
        runtime.provider_name(),
        "mock",
        "the override still applies to this run"
    );
    drop(runtime);

    let saved = Runtime::open(RuntimeOptions {
        data_dir: Some(dir.path().into()),
        ..RuntimeOptions::default()
    })
    .unwrap();
    assert_eq!(saved.provider_name(), "ollama");
    assert_eq!(saved.model(), Some("bonsai-2:4b"));
    assert_eq!(saved.settings().values.history_days, Some(30));

    // Before onboarding: the app's mock fallback must not become the saved provider either.
    let fresh = tempfile::tempdir().unwrap();
    let (mut runtime, note) = Runtime::open_for_app(RuntimeOptions {
        data_dir: Some(fresh.path().into()),
        ..RuntimeOptions::default()
    })
    .unwrap();
    assert!(note.is_some());
    let mut values = runtime.settings().values;
    assert_eq!(values.provider, SettingsProvider::Ollama);
    values.history_days = Some(7);
    runtime.update_settings(values).await.unwrap();
    drop(runtime);
    let (_, note) = Runtime::open_for_app(RuntimeOptions {
        data_dir: Some(fresh.path().into()),
        ..RuntimeOptions::default()
    })
    .unwrap();
    assert!(
        note.is_some(),
        "still asks for a model; the mock wasn't saved"
    );
}

/// Retention that can't finish at startup (another Loki process holding the database, a locked
/// file) is reported as a startup warning; it never keeps the app from opening.
#[tokio::test]
async fn mock_a_retention_failure_at_startup_is_a_warning_not_a_failed_start() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = open(&dir);
    let mut settings = runtime.settings().values;
    settings.history_days = Some(1);
    runtime.update_settings(settings).await.unwrap();
    let db = loki_memory::Database::open(&runtime.paths().database_file()).unwrap();
    drop(runtime);
    db.create_conversation(
        &loki_core::ConversationId::new("old"),
        "expired",
        loki_core::UnixMillis(1),
    )
    .unwrap();
    db.connection().execute_batch("CREATE TRIGGER fail_delete BEFORE DELETE ON conversations BEGIN SELECT RAISE(ABORT, 'injected deletion failure'); END;").unwrap();

    let runtime = open(&dir);
    assert!(
        runtime
            .startup_warnings()
            .iter()
            .any(|w| w.contains("history retention didn't finish")),
        "{:?}",
        runtime.startup_warnings()
    );
    drop(runtime);
    db.connection()
        .execute_batch("DROP TRIGGER fail_delete;")
        .unwrap();
    let runtime = open(&dir);
    assert!(
        runtime.startup_warnings().is_empty(),
        "retried and finished"
    );
    assert!(runtime.list_conversations().await.unwrap().is_empty());
}
