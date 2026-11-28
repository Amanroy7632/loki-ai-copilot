//! [`Runtime`]: the single API the CLI (and from P5 the Tauri commands) call.
//!
//! It wires config, storage, the network policy and the provider together. Turn logic lives in
//! `loki-agent` ([`Orchestrator`]); this module only builds it and adapts storage to it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use loki_agent::{
    ChatStore, DEFAULT_SYSTEM_PROMPT, IdSource, Orchestrator, StoreError, ToolHost, ToolUi,
    TurnResult,
};
use loki_core::{
    AllowedRootInfo, Capability, CapabilityStatus, Conversation, ConversationId, Grant, Message,
    MessageId, TaskId, UnixMillis,
};
use loki_llm::{
    CancellationToken, ChatProvider, InstalledModel, MockProvider, OllamaApi, OllamaProvider,
    ProviderHealth, PullProgress,
};
use loki_memory::{Database, MemoryStore, TaskRow, ToolRunFilter, ToolRunRow};
use loki_tools::platform::Platform;

use crate::config::{self, Config, ProviderKind};
use crate::error::RuntimeError;
use crate::ids;
use crate::logging::{self, LoggingGuard};
use crate::models::{self, MachineProfile, Recommendation};
use crate::net::HttpClients;
use crate::paths::Paths;
use crate::redact::Redactor;
use crate::tooling::build_tool_host;

mod settings;

/// One capability as shown by `loki permissions list` and the permissions screen.
pub use loki_core::PermissionRow;

/// Longest conversation title derived from the first message, in characters.
const TITLE_MAX_CHARS: usize = 60;

/// Options for [`Runtime::open`].
#[derive(Clone, Default)]
pub struct RuntimeOptions {
    /// OS adapter override (tests pass `MockPlatform`). `None` = the native adapter.
    pub platform: Option<Arc<dyn Platform>>,
    /// `--data-dir`. When `None`, `LOKI_DATA_DIR` and then the OS default are used.
    pub data_dir: Option<PathBuf>,
    /// `--provider`: overrides the config for this run only.
    pub provider: Option<ProviderKind>,
    /// `--model`: overrides the config for this run only.
    pub model: Option<String>,
    /// Delay between mock tokens, so streaming (and Ctrl+C) is visible. Ignored by real providers.
    pub mock_token_delay: Duration,
    /// Install file logging as the global subscriber. Off in tests that open several runtimes.
    pub init_logging: bool,
    /// With `provider = mock`: play these scripts (one per model request) instead of echoing.
    /// For end-to-end and security tests (`loki --mock-script`); never a real provider.
    pub mock_scripts: Option<Vec<loki_llm::MockScript>>,
}

/// How a reply attempt ended, as seen by a front end.
pub type ReplyOutcome = TurnResult;

// ------------------------------------------------------------------------- storage adapters

/// Adapts `loki-memory` to the agent's [`ChatStore`] trait (a local newtype, so the agent never
/// sees the concrete store).
struct MemoryChatStore(MemoryStore);

impl ChatStore for MemoryChatStore {
    fn messages(
        &self,
        conversation: &ConversationId,
    ) -> BoxFuture<'_, Result<Vec<Message>, StoreError>> {
        let id = conversation.clone();
        async move {
            self.0
                .run(move |db| db.list_messages(&id))
                .await
                .map_err(|error| StoreError(error.to_string()))
        }
        .boxed()
    }

    fn append(&self, message: Message) -> BoxFuture<'_, Result<(), StoreError>> {
        async move {
            self.0
                .run(move |db| db.append_message(&message))
                .await
                .map_err(|error| StoreError(error.to_string()))
        }
        .boxed()
    }

    fn delete(&self, id: &MessageId) -> BoxFuture<'_, Result<(), StoreError>> {
        let id = id.clone();
        async move {
            self.0
                .run(move |db| db.delete_message(&id).map(|_| ()))
                .await
                .map_err(|error| StoreError(error.to_string()))
        }
        .boxed()
    }
}

/// UUID v7 IDs and the wall clock.
struct SystemIds;

impl IdSource for SystemIds {
    fn new_id(&self) -> String {
        ids::new_run_id()
    }
    fn now(&self) -> UnixMillis {
        UnixMillis::now()
    }
}

// ------------------------------------------------------------------------- runtime

/// The composed application.
pub struct Runtime {
    paths: Paths,
    config: Config,
    warnings: Vec<String>,
    store: MemoryStore,
    http: HttpClients,
    provider_kind: ProviderKind,
    model: Option<String>,
    mock: MockProvider,
    orchestrator: Orchestrator,
    platform: Arc<dyn Platform>,
    tools: Option<Arc<ToolHost>>,
    turns: Arc<crate::stream::TurnRegistry>,
    approvals: Arc<crate::stream::ApprovalBroker>,
    _logging: Option<LoggingGuard>,
}

impl std::fmt::Debug for RuntimeOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeOptions")
            .field("data_dir", &self.data_dir)
            .field("provider", &self.provider)
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("paths", &self.paths)
            .field("provider", &self.provider_kind)
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

impl Runtime {
    /// Resolves paths, loads (or repairs) config, starts logging, opens the database and builds
    /// the provider. It does **not** contact the provider; call [`Runtime::provider_health`].
    ///
    /// Blocking: call it before entering the async runtime, or from `spawn_blocking`.
    pub fn open(options: RuntimeOptions) -> Result<Self, RuntimeError> {
        let paths = Paths::resolve(options.data_dir)?;
        paths.ensure()?;
        let loaded = config::load_or_init(&paths.config_file())?;
        let config = loaded.config;
        // Fail on a missing model *before* installing the global logger, so a caller can retry
        // (`open_for_app` falls back to the mock) without initialising logging twice.
        if options.provider.unwrap_or(config.provider) == ProviderKind::Ollama
            && options.model.is_none()
            && config.model_id.is_none()
        {
            return Err(RuntimeError::NoModelSelected);
        }

        let logging = if options.init_logging {
            let roots = config.allowed_roots.iter().map(|root| root.path.clone());
            Some(logging::init(&paths.log_dir, Redactor::new(roots))?)
        } else {
            None
        };
        for warning in &loaded.warnings {
            tracing::warn!("{warning}");
        }

        let db = Database::open(&paths.database_file())?;
        // Retention runs on every start. A failure (another Loki process holding the database,
        // a locked file) must not keep the app from starting: it is reported and retried on the
        // next start or save. History it couldn't delete stays until then.
        let retention_warning = db
            .apply_retention(config.retention.history_days, UnixMillis::now())
            .err()
            .map(|error| format!("history retention didn't finish and will retry: {error}"));
        // Startup recovery (P3-T04): unfinished tasks become Interrupted and are never re-run.
        // `open` is blocking by contract, so this runs synchronously before the store exists.
        let interrupted = db.recover_interrupted_tasks(UnixMillis::now())?;
        let store = MemoryStore::new(db);
        let mut warnings = loaded.warnings;
        if let Some(warning) = retention_warning {
            tracing::warn!("{warning}");
            warnings.push(warning);
        }
        if interrupted > 0 {
            tracing::warn!(
                interrupted,
                "marked unfinished tool tasks as interrupted; none were re-run"
            );
        }
        let platform = options
            .platform
            .unwrap_or_else(loki_tools::platform::native);
        let (tools, tool_warnings) = build_tool_host(&config, &store, Arc::clone(&platform))?;
        warnings.extend(tool_warnings);
        let http = HttpClients::new(config.network_mode);
        let provider_kind = options.provider.unwrap_or(config.provider);
        let model = options.model.or_else(|| config.model_id.clone());
        let mock = mock_provider(options.mock_scripts.clone(), options.mock_token_delay);
        let orchestrator = build_orchestrator(
            &config,
            &store,
            http,
            provider_kind,
            model.as_deref(),
            mock.clone(),
            tools.clone(),
        )?;
        tracing::info!(
            provider = orchestrator.provider().name(),
            model = model.as_deref().unwrap_or("-"),
            "runtime opened"
        );
        Ok(Self {
            paths,
            config,
            warnings,
            store,
            http,
            provider_kind,
            model,
            mock,
            orchestrator,
            platform,
            tools,
            turns: Arc::default(),
            approvals: Arc::default(),
            _logging: logging,
        })
    }

    /// The resolved directories.
    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// The effective configuration (as stored; per-run overrides are not written back).
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Warnings from startup (e.g. a corrupt config was replaced) for the front end to show.
    pub fn startup_warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Test hook: what the **mock** provider was sent (empty for real providers). Used by the
    /// security suite to check that nothing secret reaches the model context (SEC-10).
    pub fn recorded_mock_requests(&self) -> Vec<loki_llm::ChatRequest> {
        match self.provider_kind {
            ProviderKind::Mock => self.mock.recorded_requests(),
            ProviderKind::Ollama => Vec::new(),
        }
    }

    /// The provider's display name (`mock` or `ollama`), so front ends can label replies honestly.
    pub fn provider_name(&self) -> &str {
        self.orchestrator.provider().name()
    }

    /// The model in use, if the provider has one.
    pub fn model(&self) -> Option<&str> {
        match self.provider_kind {
            ProviderKind::Mock => None,
            ProviderKind::Ollama => self.model.as_deref(),
        }
    }

    /// Asks the provider whether it can serve requests (Ollama: server up and model installed).
    pub async fn provider_health(&self) -> ProviderHealth {
        self.orchestrator.provider().health().await
    }

    /// Opens like [`Runtime::open`], but if Ollama is configured without a model (first run,
    /// before onboarding), opens with the **mock** provider instead and returns a note saying
    /// so. Front ends must show the note; the mock is always labelled `mock`, never a model.
    pub fn open_for_app(options: RuntimeOptions) -> Result<(Self, Option<String>), RuntimeError> {
        match Self::open(options.clone()) {
            Ok(runtime) => Ok((runtime, None)),
            Err(RuntimeError::NoModelSelected) => {
                let runtime = Self::open(RuntimeOptions {
                    provider: Some(ProviderKind::Mock),
                    ..options
                })?;
                Ok((
                    runtime,
                    Some(String::from(
                        "No model selected yet: replies come from the built-in mock. Choose a model in Settings.",
                    )),
                ))
            }
            Err(error) => Err(error),
        }
    }

    /// The status a front end shows: provider, model, readiness and an optional note.
    pub async fn status(&self, note: Option<&str>) -> loki_core::AppStatus {
        let health = self.provider_health().await;
        let (ready, reason) = match health {
            ProviderHealth::Ready => (true, None),
            ProviderHealth::Unavailable { reason } => (false, Some(reason)),
        };
        loki_core::AppStatus {
            version: String::from(env!("CARGO_PKG_VERSION")),
            provider: self.provider_name().to_owned(),
            model: self.model().map(str::to_owned),
            ready,
            detail: reason.or_else(|| note.map(str::to_owned)),
            tools_enabled: self.tools_enabled(),
        }
    }

    /// Switches the model for this session (`/model <id>`). Not saved; use [`Runtime::use_model`].
    pub fn switch_model(&mut self, model: &str) -> Result<(), RuntimeError> {
        let orchestrator = build_orchestrator(
            &self.config,
            &self.store,
            self.http,
            ProviderKind::Ollama,
            Some(model),
            self.mock.clone(),
            self.tools.clone(),
        )?;
        self.orchestrator = orchestrator;
        self.provider_kind = ProviderKind::Ollama;
        self.model = Some(model.to_owned());
        Ok(())
    }

    // --------------------------------------------------------------------- conversations

    /// All conversations, most recently updated first.
    pub async fn list_conversations(&self) -> Result<Vec<Conversation>, RuntimeError> {
        Ok(self.store.run(|db| db.list_conversations()).await?)
    }

    /// One conversation, or [`RuntimeError::ConversationNotFound`].
    pub async fn conversation(&self, id: &ConversationId) -> Result<Conversation, RuntimeError> {
        let lookup = id.clone();
        self.store
            .run(move |db| db.get_conversation(&lookup))
            .await?
            .ok_or_else(|| RuntimeError::ConversationNotFound(id.to_string()))
    }

    /// Messages of a conversation, oldest first.
    pub async fn messages(&self, id: &ConversationId) -> Result<Vec<Message>, RuntimeError> {
        self.conversation(id).await?;
        let id = id.clone();
        Ok(self.store.run(move |db| db.list_messages(&id)).await?)
    }

    /// Renames a conversation. The title is trimmed, must not be empty, and is capped at
    /// 200 characters (it is display text only and is never sent to tools).
    pub async fn rename_conversation(
        &self,
        id: &ConversationId,
        title: &str,
    ) -> Result<Conversation, RuntimeError> {
        let title: String = title.trim().chars().take(200).collect();
        if title.is_empty() {
            return Err(RuntimeError::InvalidInput(String::from(
                "a title can't be empty",
            )));
        }
        let lookup = id.clone();
        self.store
            .run(move |db| db.rename_conversation(&lookup, &title))
            .await?
            .ok_or_else(|| RuntimeError::ConversationNotFound(id.to_string()))
    }

    /// Deletes a conversation and all its messages. Audit rows (tool runs) are kept: they are a
    /// separate record and contain no conversation text.
    pub async fn delete_conversation(&self, id: &ConversationId) -> Result<(), RuntimeError> {
        let lookup = id.clone();
        let deleted = self
            .store
            .run(move |db| db.delete_conversation(&lookup))
            .await?;
        if deleted {
            Ok(())
        } else {
            Err(RuntimeError::ConversationNotFound(id.to_string()))
        }
    }

    /// Creates a conversation titled after its first message.
    pub async fn start_conversation(
        &self,
        first_message: &str,
    ) -> Result<Conversation, RuntimeError> {
        let id = ids::new_conversation_id();
        let title = title_from(first_message);
        let now = UnixMillis::now();
        Ok(self
            .store
            .run(move |db| db.create_conversation(&id, &title, now))
            .await?)
    }

    /// Stores the user's message, then generates and stores the reply, streaming fragments to
    /// `on_token`. A stopped reply keeps its partial text with `stopped = true`.
    pub async fn send(
        &self,
        conversation: &ConversationId,
        text: &str,
        cancel: CancellationToken,
        ui: &dyn ToolUi,
        on_token: impl FnMut(&str),
    ) -> Result<ReplyOutcome, RuntimeError> {
        self.conversation(conversation).await?;
        Ok(self
            .orchestrator
            .send(conversation, text, cancel, ui, on_token)
            .await?)
    }

    /// `/retry`: replaces the replies after the last user message with a new one.
    pub async fn retry_reply(
        &self,
        conversation: &ConversationId,
        cancel: CancellationToken,
        ui: &dyn ToolUi,
        on_token: impl FnMut(&str),
    ) -> Result<ReplyOutcome, RuntimeError> {
        self.conversation(conversation).await?;
        Ok(self
            .orchestrator
            .retry(conversation, cancel, ui, on_token)
            .await?)
    }

    // --------------------------------------------------------------------- streaming (P5-T02)

    /// Runs one chat turn for a streaming front end, sending every event to `sink`.
    ///
    /// - `conversation: None` with `text` starts a new conversation (`Started` comes first).
    /// - `text: None` retries the last reply of `conversation`.
    ///
    /// The turn always ends with exactly one `Completed`, `Stopped` or `Failed` event; errors are
    /// reported that way and also returned. `turn_id` is chosen by the caller so `stop_turn` can
    /// cancel it; an ID already running is refused.
    pub async fn chat_turn(
        &self,
        turn_id: &str,
        conversation: Option<ConversationId>,
        text: Option<String>,
        sink: crate::stream::EventSink,
    ) -> Result<(), RuntimeError> {
        let lease = self.turns.enter(turn_id).ok_or_else(|| {
            RuntimeError::InvalidInput(String::from(
                "invalid/duplicate turn id or Loki is shutting down",
            ))
        })?;
        let result = self
            .run_turn(conversation, text, lease.cancel.clone(), &sink)
            .await;
        match result {
            Ok(TurnResult::Completed(message)) => sink(loki_core::TurnEvent::Completed { message }),
            Ok(TurnResult::Stopped(message)) => sink(loki_core::TurnEvent::Stopped { message }),
            Err(error) => {
                sink(loki_core::TurnEvent::Failed {
                    error: error.to_string(),
                });
                return Err(error);
            }
        }
        Ok(())
    }

    async fn run_turn(
        &self,
        conversation: Option<ConversationId>,
        text: Option<String>,
        cancel: CancellationToken,
        sink: &crate::stream::EventSink,
    ) -> Result<TurnResult, RuntimeError> {
        let ui = crate::stream::StreamUi {
            sink: Arc::clone(sink),
            broker: Arc::clone(&self.approvals),
        };
        let on_token = |token: &str| {
            sink(loki_core::TurnEvent::Token {
                text: token.to_owned(),
            });
        };
        match (conversation, text) {
            (None, Some(text)) => {
                let created = self.start_conversation(&text).await?;
                sink(loki_core::TurnEvent::Started {
                    conversation: created.clone(),
                });
                self.send(&created.id, &text, cancel, &ui, on_token).await
            }
            (Some(id), Some(text)) => self.send(&id, &text, cancel, &ui, on_token).await,
            (Some(id), None) => self.retry_reply(&id, cancel, &ui, on_token).await,
            (None, None) => Err(RuntimeError::InvalidInput(String::from(
                "a new conversation needs a message",
            ))),
        }
    }

    /// Independent control handle, usable even while front ends hold a runtime write lock.
    pub fn lifecycle_control(&self) -> crate::LifecycleControl {
        crate::LifecycleControl {
            turns: Arc::clone(&self.turns),
        }
    }

    /// Stops a running turn (the UI's Stop button). Returns `false` if it isn't running.
    pub fn stop_turn(&self, turn_id: &str) -> bool {
        self.turns.stop(turn_id)
    }

    /// Delivers the user's answer to a pending approval, with the arguments the dialog showed.
    /// Returns `false` for unknown or already answered IDs, and for a "yes" that has expired or
    /// doesn't match the shown arguments (delivered as a refusal). A "yes" still has to pass the
    /// store's single-use, unexpired, same-arguments check before anything runs.
    pub fn respond_approval(
        &self,
        approval_id: &str,
        approved: bool,
        shown_arguments: &serde_json::Value,
    ) -> bool {
        self.approvals
            .respond(approval_id, approved, shown_arguments)
    }

    // --------------------------------------------------------------------- tools (P3)

    /// Whether the tool loop is enabled.
    pub fn tools_enabled(&self) -> bool {
        self.tools.is_some()
    }

    /// Every capability with its platform status and whether (and where) it is granted.
    pub async fn permissions(&self) -> Result<Vec<PermissionRow>, RuntimeError> {
        let grants = self.store.run(|db| db.active_grants()).await?;
        let statuses = self.platform.capabilities();
        Ok(Capability::ALL
            .into_iter()
            .map(|capability| PermissionRow {
                capability,
                status: statuses.get(&capability).cloned().unwrap_or(
                    CapabilityStatus::Unavailable {
                        reason: String::from("not reported by this platform"),
                    },
                ),
                scopes: grants
                    .iter()
                    .filter(|g| g.capability == capability)
                    .map(|g| g.scope.clone())
                    .collect(),
            })
            .collect())
    }

    /// Grants a capability (`scope` = `*` or an allowed-root ID for `file_read`). Granting a
    /// capability the platform can't support is refused, so the UI never shows it as working.
    pub async fn grant(&self, capability: &str, scope: &str) -> Result<Grant, RuntimeError> {
        let grant = self.parse_grant(capability, scope)?;
        if let Some(CapabilityStatus::Unavailable { reason }) =
            self.platform.capabilities().get(&grant.capability)
        {
            return Err(RuntimeError::InvalidInput(format!(
                "{capability} is unavailable here: {reason}"
            )));
        }
        let stored = grant.clone();
        self.store
            .run(move |db| db.grant(&ids::new_run_id(), &stored, UnixMillis::now()))
            .await?;
        Ok(grant)
    }

    /// Revokes an exact grant. Returns how many were revoked.
    pub async fn revoke(&self, capability: &str, scope: &str) -> Result<usize, RuntimeError> {
        let grant = self.parse_grant(capability, scope)?;
        Ok(self
            .store
            .run(move |db| db.revoke(&grant, UnixMillis::now()))
            .await?)
    }

    fn parse_grant(&self, capability: &str, scope: &str) -> Result<Grant, RuntimeError> {
        let parsed = Capability::parse(capability).ok_or_else(|| {
            let known: Vec<&str> = Capability::ALL.iter().map(|c| c.as_str()).collect();
            RuntimeError::InvalidInput(format!(
                "unknown capability '{capability}'. Known: {}",
                known.join(", ")
            ))
        })?;
        if scope != loki_core::ANY_SCOPE
            && !self
                .config
                .allowed_roots
                .iter()
                .any(|root| root.id == scope)
        {
            return Err(RuntimeError::InvalidInput(format!(
                "scope must be '*' or an allowed root ID (see `loki roots list`), not '{scope}'"
            )));
        }
        Ok(Grant {
            capability: parsed,
            scope: scope.to_owned(),
        })
    }

    /// The allowed roots, for display (the model only sees the IDs).
    pub fn roots(&self) -> Vec<AllowedRootInfo> {
        self.config
            .allowed_roots
            .iter()
            .map(|root| AllowedRootInfo {
                id: root.id.clone(),
                path: root.path.display().to_string(),
            })
            .collect()
    }

    /// Checks a root ID before anything else happens (the desktop app calls this before it
    /// opens the folder picker, so the user isn't asked to pick a folder for an invalid name).
    pub fn validate_root_id(id: &str) -> Result<(), RuntimeError> {
        let valid = (1..=32).contains(&id.len())
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
        if valid {
            Ok(())
        } else {
            Err(RuntimeError::InvalidInput(String::from(
                "folder names are 1-32 of a-z, 0-9, _ or -",
            )))
        }
    }

    /// Adds (or repoints) an allowed root. The path must be an existing directory.
    ///
    /// If the ID already named a **different** folder, its grants are revoked first: a grant
    /// was given for the folder the user saw, so it must not silently carry over to another
    /// one. The file tools see the change immediately (no restart).
    pub async fn add_root(
        &mut self,
        id: &str,
        path: &std::path::Path,
    ) -> Result<AllowedRootInfo, RuntimeError> {
        Self::validate_root_id(id)?;
        let canonical = path
            .canonicalize()
            .ok()
            .filter(|p| p.is_dir())
            .ok_or_else(|| {
                RuntimeError::InvalidInput(format!(
                    "{} is not an existing directory",
                    path.display()
                ))
            })?;
        let repointed = self
            .config
            .allowed_roots
            .iter()
            .any(|root| root.id == id && root.path != canonical);
        if repointed {
            self.revoke_scope(id).await?;
        }
        let mut config = self.config.clone();
        config.allowed_roots.retain(|root| root.id != id);
        config.allowed_roots.push(crate::config::AllowedRoot {
            id: id.to_owned(),
            path: canonical.clone(),
        });
        config::save(&self.paths.config_file(), &config)?;
        self.config = config;
        self.reload_tools()?;
        Ok(AllowedRootInfo {
            id: id.to_owned(),
            path: canonical.display().to_string(),
        })
    }

    /// Removes an allowed root and revokes every grant scoped to it, so a later root with the
    /// same ID starts with no access. Returns whether it existed.
    pub async fn remove_root(&mut self, id: &str) -> Result<bool, RuntimeError> {
        if !self.config.allowed_roots.iter().any(|root| root.id == id) {
            return Ok(false);
        }
        // Revoke first: if that fails, the root stays and nothing widens.
        self.revoke_scope(id).await?;
        let mut config = self.config.clone();
        config.allowed_roots.retain(|root| root.id != id);
        config::save(&self.paths.config_file(), &config)?;
        self.config = config;
        self.reload_tools()?;
        Ok(true)
    }

    /// Revokes every active grant with this exact scope.
    async fn revoke_scope(&self, scope: &str) -> Result<usize, RuntimeError> {
        let scope = scope.to_owned();
        Ok(self
            .store
            .run(move |db| {
                let now = UnixMillis::now();
                let mut revoked = 0;
                for grant in db.active_grants()?.iter().filter(|g| g.scope == scope) {
                    revoked += db.revoke(grant, now)?;
                }
                Ok(revoked)
            })
            .await?)
    }

    /// Rebuilds the tool host (and the orchestrator that holds it) from the current config, so
    /// a long-running front end sees root changes without a restart.
    fn reload_tools(&mut self) -> Result<(), RuntimeError> {
        let (tools, warnings) =
            build_tool_host(&self.config, &self.store, Arc::clone(&self.platform))?;
        for warning in &warnings {
            tracing::warn!("{warning}");
        }
        self.orchestrator = build_orchestrator(
            &self.config,
            &self.store,
            self.http,
            self.provider_kind,
            self.model.as_deref(),
            self.mock.clone(),
            tools.clone(),
        )?;
        self.tools = tools;
        Ok(())
    }

    /// Audit rows, newest first (redacted summaries only).
    pub async fn audit_runs(&self, filter: ToolRunFilter) -> Result<Vec<ToolRunRow>, RuntimeError> {
        Ok(self.store.run(move |db| db.list_tool_runs(&filter)).await?)
    }

    /// One task with its audit rows.
    pub async fn audit_task(
        &self,
        id: &TaskId,
    ) -> Result<(TaskRow, Vec<ToolRunRow>), RuntimeError> {
        let lookup = id.clone();
        let (task, runs) = self
            .store
            .run(move |db| Ok((db.get_task(&lookup)?, db.tool_runs_for_task(&lookup)?)))
            .await?;
        let task = task.ok_or_else(|| RuntimeError::InvalidInput(format!("no task {id}")))?;
        Ok((task, runs))
    }

    /// Apps the platform can launch (for `loki apps list`).
    pub async fn apps(&self) -> Result<Vec<loki_tools::platform::AppEntry>, RuntimeError> {
        let platform = Arc::clone(&self.platform);
        tokio::task::spawn_blocking(move || platform.list_apps())
            .await
            .map_err(|error| RuntimeError::Tools(error.to_string()))?
            .map_err(|error| RuntimeError::InvalidInput(error.to_string()))
    }

    // --------------------------------------------------------------------- models (P2-T05)

    /// Detects this machine and recommends from the allowlist.
    pub fn recommend_models(&self) -> (MachineProfile, Recommendation) {
        let profile = models::detect(&self.paths.data_dir);
        let recommendation = models::recommend(&profile, &models::allowlist());
        (profile, recommendation)
    }

    fn ollama_api(&self) -> Result<OllamaApi, RuntimeError> {
        let (client, base) = self.http.local_service(&self.config.ollama.base_url)?;
        Ok(OllamaApi::new(client, base))
    }

    /// Models installed in Ollama.
    pub async fn installed_models(&self) -> Result<Vec<InstalledModel>, RuntimeError> {
        Ok(self.ollama_api()?.installed_models().await?)
    }

    /// Downloads an **allowlisted** model through Ollama, streaming progress. The caller must
    /// show the license (see [`models::find`]) and get consent first. Cancel with the token.
    pub fn pull_model(
        &self,
        id: &str,
        cancel: CancellationToken,
    ) -> Result<BoxStream<'static, Result<PullProgress, loki_llm::LlmError>>, RuntimeError> {
        if models::find(id).is_none() {
            return Err(RuntimeError::ModelNotAllowlisted(id.to_owned()));
        }
        Ok(self.ollama_api()?.pull(id, cancel))
    }

    /// Saves `id` as the model (and Ollama as the provider) in the config file.
    pub fn use_model(&mut self, id: &str) -> Result<(), RuntimeError> {
        refuse_cloud(id)?;
        let mut config = self.config.clone();
        config.provider = ProviderKind::Ollama;
        config.model_id = Some(id.to_owned());
        config::save(&self.paths.config_file(), &config)?;
        self.config = config;
        Ok(())
    }
}

fn refuse_cloud(model: &str) -> Result<(), RuntimeError> {
    if models::is_cloud_model(model) {
        return Err(RuntimeError::CloudModelRefused(model.to_owned()));
    }
    Ok(())
}

/// The mock provider for `provider = mock`: scripted when scripts are given, else echo.
fn mock_provider(scripts: Option<Vec<loki_llm::MockScript>>, delay: Duration) -> MockProvider {
    match scripts {
        Some(scripts) if !scripts.is_empty() => MockProvider::sequence(scripts),
        _ => MockProvider::echo(),
    }
    .with_token_delay(delay)
}

fn build_orchestrator(
    config: &Config,
    store: &MemoryStore,
    http: HttpClients,
    kind: ProviderKind,
    model: Option<&str>,
    mock: MockProvider,
    tools: Option<Arc<ToolHost>>,
) -> Result<Orchestrator, RuntimeError> {
    let provider: Arc<dyn ChatProvider> = match kind {
        ProviderKind::Mock => Arc::new(mock),
        ProviderKind::Ollama => {
            let model = model.ok_or(RuntimeError::NoModelSelected)?;
            refuse_cloud(model)?;
            let (client, base) = http.local_service(&config.ollama.base_url)?;
            // Thinking mode comes from the reviewed allowlist; unreviewed models run without it.
            let think = models::find(model).is_some_and(|entry| entry.think);
            Arc::new(
                OllamaProvider::new(OllamaApi::new(client, base), model, config.context.tokens)
                    .with_think(think),
            )
        }
    };
    let orchestrator = Orchestrator::new(
        provider,
        Arc::new(MemoryChatStore(store.clone())),
        Arc::new(SystemIds),
        DEFAULT_SYSTEM_PROMPT,
        config.context.reserve_tokens,
    );
    Ok(match tools {
        Some(host) => orchestrator.with_tools(host),
        None => orchestrator,
    })
}

/// First line of the message, trimmed and cut to [`TITLE_MAX_CHARS`] characters.
fn title_from(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return String::from("Untitled");
    }
    let mut title: String = line.chars().take(TITLE_MAX_CHARS).collect();
    if line.chars().count() > TITLE_MAX_CHARS {
        title.push('…');
    }
    title
}

#[cfg(test)]
mod tests {
    use loki_core::Role;

    use super::*;

    fn open_with(
        dir: &tempfile::TempDir,
        provider: Option<ProviderKind>,
        model: Option<&str>,
    ) -> Result<Runtime, RuntimeError> {
        Runtime::open(RuntimeOptions {
            data_dir: Some(dir.path().to_path_buf()),
            provider,
            model: model.map(str::to_owned),
            ..RuntimeOptions::default()
        })
    }

    fn open_mock(dir: &tempfile::TempDir) -> Runtime {
        open_with(dir, Some(ProviderKind::Mock), None).unwrap()
    }

    #[test]
    fn title_is_first_line_truncated() {
        assert_eq!(title_from("  hello\nworld"), "hello");
        assert_eq!(title_from(""), "Untitled");
        let long = "x".repeat(100);
        assert_eq!(title_from(&long).chars().count(), TITLE_MAX_CHARS + 1);
    }

    #[tokio::test]
    async fn mock_chat_persists_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let id = {
            let runtime = open_mock(&dir);
            assert_eq!(runtime.provider_name(), "mock");
            let conversation = runtime.start_conversation("hi there").await.unwrap();
            let outcome = runtime
                .send(
                    &conversation.id,
                    "hi there",
                    CancellationToken::new(),
                    &loki_agent::DenyAllApprovals,
                    |_| {},
                )
                .await
                .unwrap();
            assert!(matches!(outcome, ReplyOutcome::Completed(_)));
            conversation.id
        };
        let runtime = open_mock(&dir);
        let messages = runtime.messages(&id).await.unwrap();
        let roles: Vec<Role> = messages.iter().map(|m| m.role).collect();
        assert_eq!(roles, [Role::User, Role::Assistant]);
        assert_eq!(messages[1].content, "Mock reply: hi there");
    }

    #[tokio::test]
    async fn mock_cancelled_before_text_stores_no_reply_and_retry_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = open_mock(&dir);
        let conversation = runtime.start_conversation("q").await.unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let outcome = runtime
            .send(
                &conversation.id,
                "q",
                cancel,
                &loki_agent::DenyAllApprovals,
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(outcome, ReplyOutcome::Stopped(None));
        assert_eq!(runtime.messages(&conversation.id).await.unwrap().len(), 1);

        for _ in 0..2 {
            runtime
                .retry_reply(
                    &conversation.id,
                    CancellationToken::new(),
                    &loki_agent::DenyAllApprovals,
                    |_| {},
                )
                .await
                .unwrap();
        }
        assert_eq!(
            runtime.messages(&conversation.id).await.unwrap().len(),
            2,
            "retry replaces"
        );
    }

    #[tokio::test]
    async fn unknown_conversation_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = open_mock(&dir);
        let error = runtime
            .messages(&ConversationId::new("missing"))
            .await
            .unwrap_err();
        assert!(matches!(error, RuntimeError::ConversationNotFound(_)));
    }

    #[tokio::test]
    async fn app_open_without_a_model_falls_back_to_a_labelled_mock() {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, note) = Runtime::open_for_app(RuntimeOptions {
            data_dir: Some(dir.path().to_path_buf()),
            ..RuntimeOptions::default()
        })
        .unwrap();
        assert_eq!(runtime.provider_name(), "mock");
        let note = note.expect("the fallback must be explained");
        let status = runtime.status(Some(&note)).await;
        assert_eq!(status.provider, "mock");
        assert!(status.ready);
        assert!(status.detail.unwrap().contains("No model selected"));
    }

    #[test]
    fn ollama_without_a_model_asks_for_one() {
        let dir = tempfile::tempdir().unwrap();
        let error = open_with(&dir, Some(ProviderKind::Ollama), None).unwrap_err();
        assert!(matches!(error, RuntimeError::NoModelSelected), "{error:?}");
    }

    #[test]
    fn cloud_models_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let error =
            open_with(&dir, Some(ProviderKind::Ollama), Some("gpt-oss:20b-cloud")).unwrap_err();
        assert!(
            matches!(error, RuntimeError::CloudModelRefused(_)),
            "{error:?}"
        );
    }

    #[test]
    fn non_loopback_ollama_url_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        paths.ensure().unwrap();
        let mut config = Config::default();
        config.ollama.base_url = String::from("http://192.168.1.50:11434");
        config::save(&paths.config_file(), &config).unwrap();
        let error = open_with(&dir, Some(ProviderKind::Ollama), Some("qwen3:4b")).unwrap_err();
        assert!(matches!(error, RuntimeError::Network(_)), "{error:?}");
    }

    #[tokio::test]
    async fn ollama_down_is_reported_by_health_not_at_open() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        paths.ensure().unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut config = Config::default();
        config.ollama.base_url = format!("http://127.0.0.1:{port}");
        config::save(&paths.config_file(), &config).unwrap();
        let runtime = open_with(&dir, None, Some("qwen3:4b")).unwrap();
        assert_eq!(runtime.provider_name(), "ollama");
        let health = runtime.provider_health().await;
        assert!(
            matches!(&health, ProviderHealth::Unavailable { reason } if reason.contains("not running")),
            "{health:?}"
        );
    }

    #[test]
    fn use_model_saves_provider_and_model() {
        let dir = tempfile::tempdir().unwrap();
        let mut runtime = open_mock(&dir);
        runtime.use_model("qwen3:4b").unwrap();
        let reloaded = config::load_or_init(&runtime.paths().config_file())
            .unwrap()
            .config;
        assert_eq!(reloaded.provider, ProviderKind::Ollama);
        assert_eq!(reloaded.model_id.as_deref(), Some("qwen3:4b"));
        assert!(runtime.use_model("glm-5.3:cloud").is_err());
    }

    #[test]
    fn pull_refuses_models_outside_the_allowlist() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = open_mock(&dir);
        let error = runtime
            .pull_model("some-random:7b", CancellationToken::new())
            .err()
            .unwrap();
        assert!(matches!(error, RuntimeError::ModelNotAllowlisted(_)));
    }
}
