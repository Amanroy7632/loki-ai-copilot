//! [`Runtime`]: the single API the CLI (and from P5 the Tauri commands) call.
//!
//! It wires config, storage and the provider together and persists results. Decisions about
//! how a reply stream is consumed live in `loki-agent`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use loki_agent::{TurnOutcome, stream_reply};
use loki_core::{Conversation, ConversationId, Message, Role, UnixMillis};
use loki_llm::{CancellationToken, ChatMessage, ChatProvider, ChatRequest, MockProvider};
use loki_memory::{Database, MemoryStore};

use crate::config::{self, Config, ProviderKind};
use crate::error::RuntimeError;
use crate::ids;
use crate::logging::{self, LoggingGuard};
use crate::paths::Paths;
use crate::redact::Redactor;

/// Longest conversation title derived from the first message, in characters.
const TITLE_MAX_CHARS: usize = 60;

/// Options for [`Runtime::open`].
#[derive(Debug, Clone, Default)]
pub struct RuntimeOptions {
    /// `--data-dir`. When `None`, `LOKI_DATA_DIR` and then the OS default are used.
    pub data_dir: Option<PathBuf>,
    /// Delay between mock tokens, so streaming (and Ctrl+C) is visible. Ignored by real providers.
    pub mock_token_delay: Duration,
    /// Install file logging as the global subscriber. Off in tests that open several runtimes.
    pub init_logging: bool,
}

/// How a reply attempt ended, as seen by a front end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyOutcome {
    /// The reply finished and was stored.
    Completed(Message),
    /// The user cancelled. Nothing was stored; `/retry` regenerates.
    Cancelled {
        /// Text that streamed before cancellation (already shown to the user).
        partial: String,
    },
}

/// The composed application.
pub struct Runtime {
    paths: Paths,
    config: Config,
    warnings: Vec<String>,
    store: MemoryStore,
    provider: Arc<dyn ChatProvider>,
    _logging: Option<LoggingGuard>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("paths", &self.paths)
            .field("provider", &self.provider.name())
            .finish_non_exhaustive()
    }
}

impl Runtime {
    /// Resolves paths, loads (or repairs) config, starts logging and opens the database.
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

        let provider: Arc<dyn ChatProvider> = match config.provider {
            ProviderKind::Mock => {
                Arc::new(MockProvider::echo().with_token_delay(options.mock_token_delay))
            }
            // P2 adds the Ollama adapter. Until then, refuse rather than fall back to anything.
            ProviderKind::Ollama => {
                return Err(RuntimeError::ProviderUnavailable(String::from("ollama")));
            }
        };

        let db = Database::open(&paths.database_file())?;
        tracing::info!(provider = provider.name(), "runtime opened");
        Ok(Self {
            paths,
            config,
            warnings: loaded.warnings,
            store: MemoryStore::new(db),
            provider,
            _logging: logging,
        })
    }

    /// The resolved directories.
    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// The effective configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Warnings from startup (e.g. a corrupt config was replaced) for the front end to show.
    pub fn startup_warnings(&self) -> &[String] {
        &self.warnings
    }

    /// The provider's display name (`mock` in P1), so front ends can label replies honestly.
    pub fn provider_name(&self) -> &str {
        self.provider.name()
    }

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

    /// Stores a user message.
    pub async fn add_user_message(
        &self,
        conversation: &ConversationId,
        text: &str,
    ) -> Result<Message, RuntimeError> {
        let message = Message {
            id: ids::new_message_id(),
            conversation_id: conversation.clone(),
            role: Role::User,
            content: text.to_owned(),
            created_at: UnixMillis::now(),
        };
        let stored = message.clone();
        self.store.run(move |db| db.append_message(&stored)).await?;
        Ok(message)
    }

    /// Generates and stores the assistant's reply to the conversation so far, streaming
    /// fragments to `on_token`. A cancelled reply is **not** stored.
    pub async fn generate_reply(
        &self,
        conversation: &ConversationId,
        cancel: CancellationToken,
        on_token: impl FnMut(&str),
    ) -> Result<ReplyOutcome, RuntimeError> {
        let history = self.messages(conversation).await?;
        let request = ChatRequest {
            messages: history
                .into_iter()
                .map(|message| ChatMessage::new(message.role, message.content))
                .collect(),
            tools: Vec::new(),
        };
        match stream_reply(self.provider.as_ref(), request, cancel, on_token).await? {
            TurnOutcome::Cancelled { partial } => {
                tracing::info!("reply cancelled by user");
                Ok(ReplyOutcome::Cancelled { partial })
            }
            TurnOutcome::Completed { text, .. } => {
                let message = Message {
                    id: ids::new_message_id(),
                    conversation_id: conversation.clone(),
                    role: Role::Assistant,
                    content: text,
                    created_at: UnixMillis::now(),
                };
                let stored = message.clone();
                self.store.run(move |db| db.append_message(&stored)).await?;
                Ok(ReplyOutcome::Completed(message))
            }
        }
    }

    /// `/retry`: removes the assistant replies after the last user message, then generates a
    /// new one. User messages are never removed.
    pub async fn retry_reply(
        &self,
        conversation: &ConversationId,
        cancel: CancellationToken,
        on_token: impl FnMut(&str),
    ) -> Result<ReplyOutcome, RuntimeError> {
        let messages = self.messages(conversation).await?;
        let trailing: Vec<_> = messages
            .iter()
            .rev()
            .take_while(|message| message.role == Role::Assistant)
            .map(|message| message.id.clone())
            .collect();
        if !trailing.is_empty() {
            self.store
                .run(move |db| {
                    for id in &trailing {
                        db.delete_message(id)?;
                    }
                    Ok(())
                })
                .await?;
        }
        self.generate_reply(conversation, cancel, on_token).await
    }
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
    use super::*;

    fn open(dir: &tempfile::TempDir) -> Runtime {
        Runtime::open(RuntimeOptions {
            data_dir: Some(dir.path().to_path_buf()),
            ..RuntimeOptions::default()
        })
        .unwrap()
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
            let runtime = open(&dir);
            assert_eq!(runtime.provider_name(), "mock");
            let conversation = runtime.start_conversation("hi there").await.unwrap();
            runtime
                .add_user_message(&conversation.id, "hi there")
                .await
                .unwrap();
            let outcome = runtime
                .generate_reply(&conversation.id, CancellationToken::new(), |_| {})
                .await
                .unwrap();
            assert!(matches!(outcome, ReplyOutcome::Completed(_)));
            conversation.id
        };
        let runtime = open(&dir);
        let messages = runtime.messages(&id).await.unwrap();
        let roles: Vec<Role> = messages.iter().map(|m| m.role).collect();
        assert_eq!(roles, [Role::User, Role::Assistant]);
        assert_eq!(messages[1].content, "Mock reply: hi there");
    }

    #[tokio::test]
    async fn cancelled_reply_is_not_stored_and_retry_replaces_replies() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = open(&dir);
        let conversation = runtime.start_conversation("q").await.unwrap();
        runtime
            .add_user_message(&conversation.id, "q")
            .await
            .unwrap();

        let cancel = CancellationToken::new();
        cancel.cancel();
        let outcome = runtime
            .generate_reply(&conversation.id, cancel, |_| {})
            .await
            .unwrap();
        assert!(matches!(outcome, ReplyOutcome::Cancelled { .. }));
        assert_eq!(runtime.messages(&conversation.id).await.unwrap().len(), 1);

        for _ in 0..2 {
            runtime
                .retry_reply(&conversation.id, CancellationToken::new(), |_| {})
                .await
                .unwrap();
        }
        let messages = runtime.messages(&conversation.id).await.unwrap();
        assert_eq!(messages.len(), 2, "retry must replace, not append");
    }

    #[tokio::test]
    async fn unknown_conversation_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = open(&dir);
        let error = runtime
            .messages(&ConversationId::new("missing"))
            .await
            .unwrap_err();
        assert!(matches!(error, RuntimeError::ConversationNotFound(_)));
    }

    #[test]
    fn ollama_provider_is_refused_until_p2() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        paths.ensure().unwrap();
        let config = Config {
            provider: ProviderKind::Ollama,
            ..Config::default()
        };
        config::save(&paths.config_file(), &config).unwrap();
        let error = Runtime::open(RuntimeOptions {
            data_dir: Some(dir.path().to_path_buf()),
            ..RuntimeOptions::default()
        })
        .unwrap_err();
        assert!(
            matches!(error, RuntimeError::ProviderUnavailable(_)),
            "{error:?}"
        );
    }
}
