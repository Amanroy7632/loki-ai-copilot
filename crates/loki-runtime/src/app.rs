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
    ChatStore, DEFAULT_SYSTEM_PROMPT, IdSource, Orchestrator, StoreError, TurnResult,
};
use loki_core::{Conversation, ConversationId, Message, MessageId, UnixMillis};
use loki_llm::{
    CancellationToken, ChatProvider, InstalledModel, MockProvider, OllamaApi, OllamaProvider,
    ProviderHealth, PullProgress,
};
use loki_memory::{Database, MemoryStore};

use crate::config::{self, Config, ProviderKind};
use crate::error::RuntimeError;
use crate::ids;
use crate::logging::{self, LoggingGuard};
use crate::models::{self, MachineProfile, Recommendation};
use crate::net::HttpClients;
use crate::paths::Paths;
use crate::redact::Redactor;

/// Longest conversation title derived from the first message, in characters.
const TITLE_MAX_CHARS: usize = 60;

/// Options for [`Runtime::open`].
#[derive(Debug, Clone, Default)]
pub struct RuntimeOptions {
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
    fn message_id(&self) -> MessageId {
        ids::new_message_id()
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
    mock_token_delay: Duration,
    orchestrator: Orchestrator,
    _logging: Option<LoggingGuard>,
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

        let logging = if options.init_logging {
            let roots = config.allowed_roots.iter().map(|root| root.path.clone());
            Some(logging::init(&paths.log_dir, Redactor::new(roots))?)
        } else {
            None
        };
        for warning in &loaded.warnings {
            tracing::warn!("{warning}");
        }

        let store = MemoryStore::new(Database::open(&paths.database_file())?);
        let http = HttpClients::new(config.network_mode);
        let provider_kind = options.provider.unwrap_or(config.provider);
        let model = options.model.or_else(|| config.model_id.clone());
        let orchestrator = build_orchestrator(
            &config,
            &store,
            http,
            provider_kind,
            model.as_deref(),
            options.mock_token_delay,
        )?;
        tracing::info!(
            provider = orchestrator.provider().name(),
            model = model.as_deref().unwrap_or("-"),
            "runtime opened"
        );
        Ok(Self {
            paths,
            config,
            warnings: loaded.warnings,
            store,
            http,
            provider_kind,
            model,
            mock_token_delay: options.mock_token_delay,
            orchestrator,
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

    /// Switches the model for this session (`/model <id>`). Not saved; use [`Runtime::use_model`].
    pub fn switch_model(&mut self, model: &str) -> Result<(), RuntimeError> {
        let orchestrator = build_orchestrator(
            &self.config,
            &self.store,
            self.http,
            ProviderKind::Ollama,
            Some(model),
            self.mock_token_delay,
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
        on_token: impl FnMut(&str),
    ) -> Result<ReplyOutcome, RuntimeError> {
        self.conversation(conversation).await?;
        Ok(self
            .orchestrator
            .send(conversation, text, cancel, on_token)
            .await?)
    }

    /// `/retry`: replaces the replies after the last user message with a new one.
    pub async fn retry_reply(
        &self,
        conversation: &ConversationId,
        cancel: CancellationToken,
        on_token: impl FnMut(&str),
    ) -> Result<ReplyOutcome, RuntimeError> {
        self.conversation(conversation).await?;
        Ok(self
            .orchestrator
            .retry(conversation, cancel, on_token)
            .await?)
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

fn build_orchestrator(
    config: &Config,
    store: &MemoryStore,
    http: HttpClients,
    kind: ProviderKind,
    model: Option<&str>,
    mock_token_delay: Duration,
) -> Result<Orchestrator, RuntimeError> {
    let provider: Arc<dyn ChatProvider> = match kind {
        ProviderKind::Mock => Arc::new(MockProvider::echo().with_token_delay(mock_token_delay)),
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
    Ok(Orchestrator::new(
        provider,
        Arc::new(MemoryChatStore(store.clone())),
        Arc::new(SystemIds),
        DEFAULT_SYSTEM_PROMPT,
        config.context.reserve_tokens,
    ))
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
            .send(&conversation.id, "q", cancel, |_| {})
            .await
            .unwrap();
        assert_eq!(outcome, ReplyOutcome::Stopped(None));
        assert_eq!(runtime.messages(&conversation.id).await.unwrap().len(), 1);

        for _ in 0..2 {
            runtime
                .retry_reply(&conversation.id, CancellationToken::new(), |_| {})
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
