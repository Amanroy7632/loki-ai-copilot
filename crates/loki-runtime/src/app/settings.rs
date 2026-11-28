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

impl Runtime {
    /// Checks the local model server for Settings: reachable or not (with how to fix it), the
    /// installed models, and the one to preselect. Only talks to the configured loopback Ollama
    /// (network policy), never downloads, and never lists cloud models.
    pub async fn check_provider(&self) -> loki_core::ProviderCheck {
        match self.installed_models().await {
            Ok(installed) => {
                let (_, recommendation) = self.recommend_models();
                let preferred: Vec<String> = recommendation
                    .primary
                    .iter()
                    .chain(&recommendation.alternatives)
                    .map(|entry| entry.id.clone())
                    .collect();
                provider_check_from(&installed, &preferred, self.config.model_id.as_deref())
            }
            Err(error) => loki_core::ProviderCheck {
                reachable: false,
                detail: Some(format!(
                    "Can't reach Ollama at {} ({error}). Start it with ./scripts/ollama-serve.sh or the Ollama app, then check again.",
                    self.config.ollama.base_url
                )),
                models: Vec::new(),
                hidden_cloud_models: 0,
                suggested: None,
            },
        }
    }
}

/// Builds the Settings view of installed models. Order: models recommended for this machine,
/// then other reviewed models (allowlist order), then unreviewed ones by name. The suggestion is
/// the saved model if it is installed, else the first in that order.
pub fn provider_check_from(
    installed: &[loki_llm::InstalledModel],
    preferred: &[String],
    saved: Option<&str>,
) -> loki_core::ProviderCheck {
    let allowlist = models::allowlist();
    let rank = |id: &str| {
        preferred
            .iter()
            .position(|p| p == id)
            .map(|i| (0, i))
            .or_else(|| allowlist.iter().position(|e| e.id == id).map(|i| (1, i)))
            .unwrap_or((2, 0))
    };
    let hidden_cloud_models = installed
        .iter()
        .filter(|m| models::is_cloud_model(&m.name))
        .count();
    let mut models: Vec<loki_core::LocalModel> = installed
        .iter()
        .filter(|m| !models::is_cloud_model(&m.name))
        .map(|m| {
            let entry = models::find(&m.name);
            loki_core::LocalModel {
                id: m.name.clone(),
                size_bytes: m.size,
                reviewed: entry.is_some(),
                display_name: entry.as_ref().map(|e| e.display_name.clone()),
                license: entry.map(|e| e.license),
            }
        })
        .collect();
    models.sort_by(|a, b| rank(&a.id).cmp(&rank(&b.id)).then_with(|| a.id.cmp(&b.id)));
    let suggested = saved
        .filter(|id| models.iter().any(|m| m.id == *id))
        .map(str::to_owned)
        .or_else(|| models.first().map(|m| m.id.clone()));
    let detail = models.is_empty().then(|| {
        String::from(
            "Ollama is running but has no local models. Download a reviewed one with `loki models pull bonsai-2:4b` (the license is shown first).",
        )
    });
    loki_core::ProviderCheck {
        reachable: true,
        detail,
        models,
        hidden_cloud_models: u32::try_from(hidden_cloud_models).unwrap_or(u32::MAX),
        suggested,
    }
}
