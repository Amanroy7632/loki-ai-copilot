//! Agent orchestrator v1 (P2-T04): one chat turn = input, context, provider stream, persist.
//!
//! Depends only on traits: the runtime injects the [`ChatProvider`], a [`ChatStore`] and an
//! [`IdSource`]. No tools yet: tools are never offered to the model, and if a model proposes a
//! call anyway it is logged and answered with [`TOOLS_UNAVAILABLE_REPLY`].

use std::sync::Arc;

use futures::future::BoxFuture;
use loki_core::{ConversationId, Message, MessageId, Role, UnixMillis};
use loki_llm::{CancellationToken, ChatProvider, ChatRequest, LlmError};

use crate::context::{BuiltContext, ContextBudget, build_context};
use crate::turn::{TurnOutcome, stream_reply};

/// The default system prompt. Short on purpose: small local models follow short prompts better.
pub const DEFAULT_SYSTEM_PROMPT: &str = "You are Loki, a helpful assistant running locally on the user's computer. \
Answer clearly and concisely. You cannot use tools or access the internet yet.";

/// Stored as the reply when the model only proposed a tool call.
pub const TOOLS_UNAVAILABLE_REPLY: &str =
    "(The model tried to use a tool, but tools are not available yet.)";

/// A storage failure, as text. The agent doesn't need to tell storage errors apart.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("storage: {0}")]
pub struct StoreError(pub String);

/// Conversation storage the agent needs. Implemented by the runtime over `loki-memory`.
pub trait ChatStore: Send + Sync {
    /// Messages of a conversation, oldest first.
    fn messages(
        &self,
        conversation: &ConversationId,
    ) -> BoxFuture<'_, Result<Vec<Message>, StoreError>>;
    /// Appends one message.
    fn append(&self, message: Message) -> BoxFuture<'_, Result<(), StoreError>>;
    /// Deletes one message (used by retry).
    fn delete(&self, id: &MessageId) -> BoxFuture<'_, Result<(), StoreError>>;
}

/// IDs and time, injected so tests are deterministic.
pub trait IdSource: Send + Sync {
    /// A new, unique message ID.
    fn message_id(&self) -> MessageId;
    /// The current time.
    fn now(&self) -> UnixMillis;
}

/// Why a turn failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentError {
    /// Storage failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The provider failed (not running, model missing, broken stream...).
    #[error(transparent)]
    Llm(#[from] LlmError),
}

/// How a turn ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnResult {
    /// The reply finished and was stored.
    Completed(Message),
    /// The user stopped it. Partial text, if any, was stored with `stopped = true`.
    Stopped(Option<Message>),
}

/// Runs chat turns. Cheap to clone the `Arc`s; build a new one to switch provider or model.
pub struct Orchestrator {
    provider: Arc<dyn ChatProvider>,
    store: Arc<dyn ChatStore>,
    ids: Arc<dyn IdSource>,
    system_prompt: String,
    reserve_tokens: u32,
}

impl std::fmt::Debug for Orchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Orchestrator")
            .field("provider", &self.provider.name())
            .finish_non_exhaustive()
    }
}

impl Orchestrator {
    /// Wires a provider and storage. `reserve_tokens` is kept free for the reply.
    pub fn new(
        provider: Arc<dyn ChatProvider>,
        store: Arc<dyn ChatStore>,
        ids: Arc<dyn IdSource>,
        system_prompt: impl Into<String>,
        reserve_tokens: u32,
    ) -> Self {
        Self {
            provider,
            store,
            ids,
            system_prompt: system_prompt.into(),
            reserve_tokens,
        }
    }

    /// The provider in use.
    pub fn provider(&self) -> &Arc<dyn ChatProvider> {
        &self.provider
    }

    fn message(&self, conversation: &ConversationId, role: Role, content: String) -> Message {
        Message::new(
            self.ids.message_id(),
            conversation.clone(),
            role,
            content,
            self.ids.now(),
        )
    }

    /// The context the next reply would see. Public so front ends can show "N turns dropped".
    pub async fn context_for(
        &self,
        conversation: &ConversationId,
    ) -> Result<BuiltContext, AgentError> {
        let history = self.store.messages(conversation).await?;
        let budget = ContextBudget::from_limits(self.provider.context_limit(), self.reserve_tokens);
        Ok(build_context(Some(&self.system_prompt), &history, budget))
    }

    /// Stores `text` as a user message, then generates and stores the reply.
    pub async fn send(
        &self,
        conversation: &ConversationId,
        text: &str,
        cancel: CancellationToken,
        on_token: impl FnMut(&str),
    ) -> Result<TurnResult, AgentError> {
        let user = self.message(conversation, Role::User, text.to_owned());
        self.store.append(user).await?;
        self.reply(conversation, cancel, on_token).await
    }

    /// Removes the assistant replies after the last user message (including stopped ones),
    /// then generates a new reply. User messages are never removed.
    pub async fn retry(
        &self,
        conversation: &ConversationId,
        cancel: CancellationToken,
        on_token: impl FnMut(&str),
    ) -> Result<TurnResult, AgentError> {
        let history = self.store.messages(conversation).await?;
        for message in history
            .iter()
            .rev()
            .take_while(|m| m.role == Role::Assistant)
        {
            self.store.delete(&message.id).await?;
        }
        self.reply(conversation, cancel, on_token).await
    }

    /// Generates and stores a reply to the conversation as it is now.
    pub async fn reply(
        &self,
        conversation: &ConversationId,
        cancel: CancellationToken,
        mut on_token: impl FnMut(&str),
    ) -> Result<TurnResult, AgentError> {
        let context = self.context_for(conversation).await?;
        if context.dropped > 0 || context.truncated {
            tracing::info!(
                dropped = context.dropped,
                truncated = context.truncated,
                "context trimmed to budget"
            );
        }
        let request = ChatRequest {
            messages: context.messages,
            tools: Vec::new(),
        };
        match stream_reply(self.provider.as_ref(), request, cancel, &mut on_token).await? {
            TurnOutcome::Completed {
                mut text,
                ignored_tool_calls,
                ..
            } => {
                if ignored_tool_calls > 0 && text.trim().is_empty() {
                    on_token(TOOLS_UNAVAILABLE_REPLY);
                    text = String::from(TOOLS_UNAVAILABLE_REPLY);
                }
                let reply = self.message(conversation, Role::Assistant, text);
                self.store.append(reply.clone()).await?;
                Ok(TurnResult::Completed(reply))
            }
            TurnOutcome::Cancelled { partial } => {
                if partial.trim().is_empty() {
                    return Ok(TurnResult::Stopped(None));
                }
                let mut reply = self.message(conversation, Role::Assistant, partial);
                reply.stopped = true;
                self.store.append(reply.clone()).await?;
                Ok(TurnResult::Stopped(Some(reply)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use futures::FutureExt;
    use loki_llm::{MockProvider, MockScript, MockStep};
    use serde_json::json;

    use super::*;

    /// In-memory store for tests ("Mock" per AGENTS.md naming).
    #[derive(Default)]
    struct MockStore {
        rows: Mutex<Vec<Message>>,
    }

    impl ChatStore for MockStore {
        fn messages(
            &self,
            conversation: &ConversationId,
        ) -> BoxFuture<'_, Result<Vec<Message>, StoreError>> {
            let rows = self
                .rows
                .lock()
                .unwrap()
                .iter()
                .filter(|m| &m.conversation_id == conversation)
                .cloned()
                .collect();
            async move { Ok(rows) }.boxed()
        }
        fn append(&self, message: Message) -> BoxFuture<'_, Result<(), StoreError>> {
            self.rows.lock().unwrap().push(message);
            async { Ok(()) }.boxed()
        }
        fn delete(&self, id: &MessageId) -> BoxFuture<'_, Result<(), StoreError>> {
            self.rows.lock().unwrap().retain(|m| &m.id != id);
            async { Ok(()) }.boxed()
        }
    }

    #[derive(Default)]
    struct MockIds(AtomicU64);

    impl IdSource for MockIds {
        fn message_id(&self) -> MessageId {
            MessageId::new(format!("m{}", self.0.fetch_add(1, Ordering::SeqCst)))
        }
        fn now(&self) -> UnixMillis {
            UnixMillis(i64::try_from(self.0.load(Ordering::SeqCst)).unwrap())
        }
    }

    fn orchestrator(provider: MockProvider) -> (Orchestrator, Arc<MockStore>) {
        let store = Arc::new(MockStore::default());
        let agent = Orchestrator::new(
            Arc::new(provider),
            store.clone(),
            Arc::new(MockIds::default()),
            "sys",
            512,
        );
        (agent, store)
    }

    fn conv() -> ConversationId {
        ConversationId::new("c1")
    }

    fn rows(store: &MockStore) -> Vec<(Role, String, bool)> {
        store
            .rows
            .lock()
            .unwrap()
            .iter()
            .map(|m| (m.role, m.content.clone(), m.stopped))
            .collect()
    }

    #[tokio::test]
    async fn mock_turn_stores_user_and_assistant_messages() {
        let (agent, store) = orchestrator(MockProvider::echo());
        let mut streamed = String::new();
        let result = agent
            .send(&conv(), "hello", CancellationToken::new(), |t| {
                streamed.push_str(t)
            })
            .await
            .unwrap();
        assert!(matches!(result, TurnResult::Completed(_)));
        assert_eq!(streamed, "Mock reply: hello");
        assert_eq!(
            rows(&store),
            [
                (Role::User, "hello".into(), false),
                (Role::Assistant, "Mock reply: hello".into(), false)
            ]
        );
    }

    #[tokio::test]
    async fn mock_stopped_reply_is_stored_partial_and_marked() {
        // Each token waits 50 ms; cancel after ~120 ms, so 2 tokens arrive first.
        let (agent, store) =
            orchestrator(MockProvider::echo().with_token_delay(Duration::from_millis(50)));
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(120)).await;
            trigger.cancel();
        });
        let result = agent
            .send(&conv(), "one two three four five six", cancel, |_| {})
            .await
            .unwrap();
        let TurnResult::Stopped(Some(partial)) = result else {
            panic!("expected a stored partial, got {result:?}")
        };
        assert!(partial.stopped);
        assert!(partial.content.starts_with("Mock "), "{}", partial.content);
        assert!(partial.content.len() < "Mock reply: one two three four five six".len());
        assert!(rows(&store).last().unwrap().2);
    }

    #[tokio::test]
    async fn mock_stop_before_any_text_stores_nothing() {
        let (agent, store) =
            orchestrator(MockProvider::echo().with_token_delay(Duration::from_secs(30)));
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = agent.send(&conv(), "hi", cancel, |_| {}).await.unwrap();
        assert_eq!(result, TurnResult::Stopped(None));
        assert_eq!(rows(&store).len(), 1, "only the user message");
    }

    #[tokio::test]
    async fn mock_retry_replaces_stopped_and_complete_replies() {
        let (agent, store) = orchestrator(MockProvider::echo());
        agent
            .send(&conv(), "q", CancellationToken::new(), |_| {})
            .await
            .unwrap();
        // Simulate an extra stopped partial after the reply.
        let mut partial = Message::new(
            MessageId::new("p"),
            conv(),
            Role::Assistant,
            "Mock",
            UnixMillis(99),
        );
        partial.stopped = true;
        store.rows.lock().unwrap().push(partial);
        agent
            .retry(&conv(), CancellationToken::new(), |_| {})
            .await
            .unwrap();
        assert_eq!(
            rows(&store),
            [
                (Role::User, "q".into(), false),
                (Role::Assistant, "Mock reply: q".into(), false)
            ]
        );
    }

    #[tokio::test]
    async fn mock_tool_call_is_answered_with_tools_unavailable() {
        let script = MockScript::new(vec![
            MockStep::ToolCall {
                tool: "time.now".into(),
                arguments: json!({}),
            },
            MockStep::Done {
                prompt_tokens: None,
                completion_tokens: None,
            },
        ])
        .unwrap();
        let (agent, store) = orchestrator(MockProvider::scripted(script));
        let result = agent
            .send(&conv(), "time?", CancellationToken::new(), |_| {})
            .await
            .unwrap();
        let TurnResult::Completed(reply) = result else {
            panic!()
        };
        assert_eq!(reply.content, TOOLS_UNAVAILABLE_REPLY);
        assert_eq!(rows(&store).len(), 2);
    }

    #[tokio::test]
    async fn mock_provider_error_stores_no_reply() {
        let script = MockScript::new(vec![MockStep::Error {
            error: loki_llm::MockErrorKind::Unavailable,
            message: "down".into(),
        }])
        .unwrap();
        let (agent, store) = orchestrator(MockProvider::scripted(script));
        let error = agent
            .send(&conv(), "hi", CancellationToken::new(), |_| {})
            .await
            .unwrap_err();
        assert_eq!(error, AgentError::Llm(LlmError::Unavailable("down".into())));
        assert_eq!(
            rows(&store).len(),
            1,
            "the user's message is kept, no reply stored"
        );
    }

    #[tokio::test]
    async fn context_includes_system_prompt_first() {
        let (agent, _store) = orchestrator(MockProvider::echo());
        agent
            .send(&conv(), "hi", CancellationToken::new(), |_| {})
            .await
            .unwrap();
        let context = agent.context_for(&conv()).await.unwrap();
        assert_eq!(context.messages[0].role, Role::System);
        assert_eq!(context.messages[0].content, "sys");
    }
}
