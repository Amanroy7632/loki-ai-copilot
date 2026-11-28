//! Settings composition and data control, shared by every front end.

use std::io::Write;
use std::path::PathBuf;

use loki_core::{
    AuditEntry, CapabilityStatus, DataExport, DesktopSettings, SettingsProvider, SettingsSnapshot,
};

use super::*;

impl Runtime {
    /// Effective editable settings. Unsupported features carry reasons, never pretend success.
    pub fn settings(&self) -> SettingsSnapshot {
        let unavailable = |reason: &str| CapabilityStatus::Unavailable {
            reason: reason.into(),
        };
        SettingsSnapshot {
            values: DesktopSettings {
                // The saved choice, not this run's override (`LOKI_PROVIDER`, `--provider`, or
                // the labelled mock before onboarding): saving must never persist an override.
                provider: match self.config.provider {
                    ProviderKind::Mock => SettingsProvider::Mock,
                    ProviderKind::Ollama => SettingsProvider::Ollama,
                },
                model_id: self.config.model_id.clone(),
                network_mode: self.config.network_mode,
                history_days: self.config.retention.history_days,
            },
            memory: unavailable(
                "Personal memory is not available yet; chat history is stored separately.",
            ),
            autostart: unavailable("Start at login is not available in this build."),
            wake_word: unavailable("Wake-word listening is not available in this build."),
        }
    }

    /// Validates and prepares the provider and policy before saving. Exclusive access prevents
    /// changing policy or deleting retained history under an active turn. No network request
    /// or model download occurs. A later health check reports a missing local model/server.
    pub async fn update_settings(
        &mut self,
        settings: DesktopSettings,
    ) -> Result<SettingsSnapshot, RuntimeError> {
        if settings
            .history_days
            .is_some_and(|days| !(1..=36500).contains(&days))
        {
            return Err(RuntimeError::InvalidInput(
                "history retention must be 1–36500 days, or keep forever".into(),
            ));
        }
        let model = settings
            .model_id
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty());
        if let Some(id) = &model {
            if id.len() > 200
                || id.chars().any(char::is_whitespace)
                || id.chars().any(char::is_control)
            {
                return Err(RuntimeError::InvalidInput(
                    "model tag must be at most 200 bytes without whitespace or control characters"
                        .into(),
                ));
            }
            refuse_cloud(id)?;
        }
        let mut candidate = self.config.clone();
        candidate.provider = match settings.provider {
            SettingsProvider::Mock => ProviderKind::Mock,
            SettingsProvider::Ollama => ProviderKind::Ollama,
        };
        candidate.model_id = model;
        candidate.network_mode = settings.network_mode;
        candidate.retention.history_days = settings.history_days;
        // Switch the active provider only when the user changed provider or model. Otherwise
        // this run keeps its override (e.g. the mock before a model is chosen).
        let provider_changed = candidate.provider != self.config.provider
            || candidate.model_id != self.config.model_id;
        let (kind, model) = if provider_changed {
            (candidate.provider, candidate.model_id.clone())
        } else {
            (self.provider_kind, self.model.clone())
        };
        let http = HttpClients::new(candidate.network_mode);
        let (tools, _) = build_tool_host(&candidate, &self.store, Arc::clone(&self.platform))?;
        let orchestrator = build_orchestrator(
            &candidate,
            &self.store,
            http,
            kind,
            model.as_deref(),
            self.mock.clone(),
            tools.clone(),
        )?;
        let path = self.paths.config_file();
        let saved = candidate.clone();
        tokio::task::spawn_blocking(move || config::save(&path, &saved))
            .await
            .map_err(|error| RuntimeError::InvalidInput(error.to_string()))??;
        self.provider_kind = kind;
        self.model = model;
        self.http = http;
        self.tools = tools;
        self.orchestrator = orchestrator;
        self.config = candidate;
        // Saving retention is explicit consent to delete expired history. If cleanup fails,
        // return the error: the persisted policy remains in force and startup retries it.
        self.store
            .run(move |db| db.apply_retention(settings.history_days, UnixMillis::now()))
            .await
            .map_err(|error| RuntimeError::SettingsApplied(error.to_string()))?;
        Ok(self.settings())
    }

    /// A consistent JSON-ready snapshot; audit/config/credentials are excluded.
    pub async fn export_data(&self) -> Result<DataExport, RuntimeError> {
        Ok(self
            .store
            .run(|db| db.export_data(UnixMillis::now()))
            .await?)
    }

    /// Writes directly to a path chosen by the native save dialog. Refuses to replace any
    /// existing file (including Loki's DB/config); no exported temporary copy is created.
    pub async fn export_data_to(&self, path: PathBuf) -> Result<(), RuntimeError> {
        let data = self.export_data().await?;
        tokio::task::spawn_blocking(move || {
            let io_error = |source| RuntimeError::Io {
                action: "export history to",
                path: path.clone(),
                source,
            };
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(io_error)?;
            let result = serde_json::to_writer_pretty(&mut file, &data)
                .map_err(std::io::Error::other)
                .and_then(|()| file.write_all(b"\n"))
                .and_then(|()| file.sync_all());
            if let Err(error) = result {
                drop(file);
                let _ = std::fs::remove_file(&path);
                return Err(io_error(error));
            }
            Ok(())
        })
        .await
        .map_err(|error| RuntimeError::InvalidInput(error.to_string()))?
    }

    /// Clears history after user confirmation. Exclusive access keeps active turns from
    /// recreating messages. Callers must discard any cached conversation text after success.
    pub async fn clear_history(&mut self) -> Result<usize, RuntimeError> {
        Ok(self.store.run(|db| db.clear_history(None)).await?)
    }

    /// At most the latest 100 tool runs, with redacted summaries only.
    pub async fn audit_entries(&self) -> Result<Vec<AuditEntry>, RuntimeError> {
        Ok(self
            .audit_runs(ToolRunFilter::default())
            .await?
            .into_iter()
            .map(|row| AuditEntry {
                id: row.id,
                tool: row.tool,
                status: row.status,
                summary: row.redacted_result,
                created_at: row.created_at,
            })
            .collect())
    }
}
