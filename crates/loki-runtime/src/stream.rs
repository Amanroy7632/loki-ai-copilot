//! Streaming chat turns for UI front ends (P5-T02): one `TurnEvent` stream per turn, a way to
//! stop a running turn by ID, and approvals answered asynchronously by the UI.
//!
//! This is front-end plumbing shared by the desktop app (and later mobile): it decides nothing.
//! Policy, approval binding and single use stay in `loki-policy` and the audit store; the broker
//! only carries the user's yes/no back to the waiting tool call.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use loki_agent::{ToolEvent, ToolUi};
use loki_core::{ApprovalPreview, TurnEvent, UnixMillis};
use loki_llm::CancellationToken;
use loki_policy::PendingApproval;
use tokio::sync::oneshot;

/// Sink for one turn's events. Must not block (it typically forwards to an IPC channel).
pub type EventSink = Arc<dyn Fn(TurnEvent) + Send + Sync>;

/// Longest accepted turn ID (front ends usually send a UUID).
const MAX_TURN_ID_LEN: usize = 64;

/// Running turns, by front-end-chosen ID, so `stop_turn` can cancel one.
#[derive(Debug, Default)]
pub struct TurnRegistry {
    turns: Mutex<HashMap<String, CancellationToken>>,
}

impl TurnRegistry {
    /// Registers a new turn. Refuses empty, oversized or already running IDs.
    pub fn begin(&self, turn_id: &str) -> Option<CancellationToken> {
        if turn_id.is_empty() || turn_id.len() > MAX_TURN_ID_LEN {
            return None;
        }
        let mut turns = self.turns.lock().ok()?;
        if turns.contains_key(turn_id) {
            return None;
        }
        let token = CancellationToken::new();
        turns.insert(turn_id.to_owned(), token.clone());
        Some(token)
    }

    /// Removes a finished turn.
    pub fn end(&self, turn_id: &str) {
        if let Ok(mut turns) = self.turns.lock() {
            turns.remove(turn_id);
        }
    }

    /// Cancels a running turn. Returns `false` if there is no such turn.
    pub fn stop(&self, turn_id: &str) -> bool {
        let token = self
            .turns
            .lock()
            .ok()
            .and_then(|turns| turns.get(turn_id).cloned());
        match token {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }
}

/// Waiting approvals, by approval ID. Each answer is delivered once; unknown or already answered
/// IDs are refused.
#[derive(Debug, Default)]
pub struct ApprovalBroker {
    waiting: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl ApprovalBroker {
    fn register(&self, id: &str) -> Option<oneshot::Receiver<bool>> {
        let (sender, receiver) = oneshot::channel();
        let mut waiting = self.waiting.lock().ok()?;
        waiting.insert(id.to_owned(), sender);
        Some(receiver)
    }

    fn forget(&self, id: &str) {
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.remove(id);
        }
    }

    /// Delivers the user's answer. Returns `false` for an unknown, expired or already answered ID.
    pub fn respond(&self, id: &str, approved: bool) -> bool {
        let sender = self
            .waiting
            .lock()
            .ok()
            .and_then(|mut waiting| waiting.remove(id));
        sender.is_some_and(|sender| sender.send(approved).is_ok())
    }
}

/// `ToolUi` for streaming front ends: approvals become `ApprovalRequested` events and wait for
/// `ApprovalBroker::respond`; tool results become `ToolFinished` events.
pub struct StreamUi {
    /// Where events go.
    pub sink: EventSink,
    /// Where answers come from.
    pub broker: Arc<ApprovalBroker>,
}

fn preview(pending: &PendingApproval) -> ApprovalPreview {
    ApprovalPreview {
        id: pending.id.clone(),
        request_id: pending.request_id.clone(),
        tool: pending.tool.clone(),
        risk: pending.risk,
        arguments: pending.arguments.clone(),
        expires_at: pending.expires_at,
    }
}

impl ToolUi for StreamUi {
    fn approve<'a>(&'a self, pending: &'a PendingApproval) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let id = pending.id.as_str().to_owned();
            let Some(receiver) = self.broker.register(&id) else {
                return false;
            };
            (self.sink)(TurnEvent::ApprovalRequested {
                approval: preview(pending),
            });
            // Wait at most until the approval expires; an expired "yes" would be refused anyway.
            let remaining = pending.expires_at.0.saturating_sub(UnixMillis::now().0);
            let wait = Duration::from_millis(u64::try_from(remaining).unwrap_or(0));
            let answer = tokio::time::timeout(wait, receiver).await;
            self.broker.forget(&id);
            matches!(answer, Ok(Ok(true)))
        })
    }

    fn notify(&self, event: &ToolEvent) {
        let ToolEvent::Finished {
            tool,
            status,
            detail,
        } = event;
        (self.sink)(TurnEvent::ToolFinished {
            tool: tool.clone(),
            status: *status,
            detail: detail.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_ids_are_validated_and_unique_while_running() {
        let turns = TurnRegistry::default();
        assert!(turns.begin("").is_none());
        assert!(turns.begin(&"x".repeat(65)).is_none());
        let token = turns.begin("t1").unwrap();
        assert!(turns.begin("t1").is_none(), "duplicate while running");
        assert!(turns.stop("t1"));
        assert!(token.is_cancelled());
        assert!(!turns.stop("nope"));
        turns.end("t1");
        assert!(turns.begin("t1").is_some(), "reusable after it ended");
    }

    #[test]
    fn approval_answers_are_delivered_once_and_unknown_ids_refused() {
        let broker = ApprovalBroker::default();
        assert!(!broker.respond("unknown", true));
        let mut receiver = broker.register("ap-1").unwrap();
        assert!(broker.respond("ap-1", true));
        assert_eq!(receiver.try_recv(), Ok(true));
        assert!(!broker.respond("ap-1", true), "an answer is single use");
    }
}
