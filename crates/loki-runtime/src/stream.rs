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
use serde_json::Value;
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
///
/// A "yes" is only passed on when the UI sends back the **arguments it showed** and they match
/// the pending request, and the approval hasn't expired (P5-T04). That way a dialog that showed
/// something else (a stale request, a rendering bug, a number the webview rounded) can't approve
/// what actually runs. Anything else is delivered to the waiting tool call as a refusal.
#[derive(Debug, Default)]
pub struct ApprovalBroker {
    waiting: Mutex<HashMap<String, Waiting>>,
}

#[derive(Debug)]
struct Waiting {
    sender: oneshot::Sender<bool>,
    arguments: Value,
    expires_at: UnixMillis,
}

impl ApprovalBroker {
    fn register(&self, pending: &PendingApproval) -> Option<oneshot::Receiver<bool>> {
        let (sender, receiver) = oneshot::channel();
        let mut waiting = self.waiting.lock().ok()?;
        waiting.insert(
            pending.id.as_str().to_owned(),
            Waiting {
                sender,
                arguments: pending.arguments.clone(),
                expires_at: pending.expires_at,
            },
        );
        Some(receiver)
    }

    fn forget(&self, id: &str) {
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.remove(id);
        }
    }

    /// Delivers the user's answer. `shown_arguments` are the arguments the dialog displayed.
    ///
    /// Returns `true` when the answer was delivered as given. Returns `false` for an unknown or
    /// already answered ID, and for a "yes" that is expired or doesn't match the shown
    /// arguments; those are delivered as a refusal, so nothing runs. A "no" is always accepted.
    pub fn respond(&self, id: &str, approved: bool, shown_arguments: &Value) -> bool {
        self.respond_at(id, approved, shown_arguments, UnixMillis::now())
    }

    fn respond_at(&self, id: &str, approved: bool, shown: &Value, now: UnixMillis) -> bool {
        let entry = self
            .waiting
            .lock()
            .ok()
            .and_then(|mut waiting| waiting.remove(id));
        let Some(entry) = entry else {
            return false;
        };
        let valid =
            !approved || (!now.has_reached(entry.expires_at) && same_json(shown, &entry.arguments));
        let delivered = entry.sender.send(approved && valid).is_ok();
        delivered && valid
    }
}

/// JSON equality as the user saw it. Integers must match exactly (a large integer the webview
/// rounded is a different value); a float matches an integer of the same value, because
/// JavaScript prints `1.0` as `1`.
fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            if x.is_f64() || y.is_f64() {
                matches!((x.as_f64(), y.as_f64()), (Some(x), Some(y)) if x == y)
            } else {
                x == y
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same_json(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(key, x)| y.get(key).is_some_and(|y| same_json(x, y)))
        }
        _ => a == b,
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
            let Some(receiver) = self.broker.register(pending) else {
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

    fn pending(id: &str, arguments: Value, expires_at: i64) -> PendingApproval {
        PendingApproval {
            id: loki_core::ApprovalId::new(id),
            request_id: loki_core::RequestId::new("rq-1"),
            tool: loki_core::ToolName::new("app.open"),
            risk: loki_core::Risk::LocalChange,
            arguments,
            args_hash: String::from("unused-here"),
            expires_at: UnixMillis(expires_at),
        }
    }

    const NOW: UnixMillis = UnixMillis(1_000);

    #[test]
    fn approval_answers_are_delivered_once_and_unknown_ids_refused() {
        let broker = ApprovalBroker::default();
        let args = serde_json::json!({ "app_id": "com.example.calc" });
        assert!(!broker.respond_at("unknown", true, &args, NOW));
        let mut receiver = broker
            .register(&pending("ap-1", args.clone(), 2_000))
            .unwrap();
        assert!(broker.respond_at("ap-1", true, &args, NOW));
        assert_eq!(receiver.try_recv(), Ok(true));
        assert!(
            !broker.respond_at("ap-1", true, &args, NOW),
            "an answer is single use"
        );
    }

    #[test]
    fn approving_is_bound_to_the_shown_arguments() {
        let broker = ApprovalBroker::default();
        let args = serde_json::json!({ "app_id": "com.example.calc", "n": 2, "tags": ["a"] });
        let different = [
            serde_json::json!({ "app_id": "com.example.evil", "n": 2, "tags": ["a"] }),
            serde_json::json!({ "app_id": "com.example.calc", "n": 3, "tags": ["a"] }),
            serde_json::json!({ "app_id": "com.example.calc", "n": 2 }),
            serde_json::json!({ "app_id": "com.example.calc", "n": 2, "tags": ["a"], "x": 1 }),
            serde_json::json!({ "app_id": "com.example.calc", "n": 2, "tags": ["a", "b"] }),
            serde_json::Value::Null,
        ];
        for (i, shown) in different.iter().enumerate() {
            let id = format!("ap-{i}");
            let mut receiver = broker.register(&pending(&id, args.clone(), 2_000)).unwrap();
            assert!(!broker.respond_at(&id, true, shown, NOW), "{shown}");
            assert_eq!(
                receiver.try_recv(),
                Ok(false),
                "delivered as a refusal: {shown}"
            );
            assert!(!broker.respond_at(&id, true, &args, NOW), "and used up");
        }
        // Key order doesn't matter; a float the webview printed as an integer does match.
        let float = serde_json::json!({ "volume": 1.0, "app_id": "x" });
        let mut receiver = broker.register(&pending("ap-f", float, 2_000)).unwrap();
        let shown = serde_json::json!({ "app_id": "x", "volume": 1 });
        assert!(broker.respond_at("ap-f", true, &shown, NOW));
        assert_eq!(receiver.try_recv(), Ok(true));
    }

    #[test]
    fn a_large_integer_rounded_by_the_webview_does_not_match() {
        let broker = ApprovalBroker::default();
        let exact = serde_json::json!({ "n": 9_007_199_254_740_993_u64 }); // 2^53 + 1
        let rounded = serde_json::json!({ "n": 9_007_199_254_740_992_u64 }); // what JS shows
        let mut receiver = broker.register(&pending("ap-big", exact, 2_000)).unwrap();
        assert!(!broker.respond_at("ap-big", true, &rounded, NOW));
        assert_eq!(receiver.try_recv(), Ok(false));
    }

    #[test]
    fn an_expired_approval_is_refused_but_a_no_is_always_accepted() {
        let broker = ApprovalBroker::default();
        let args = serde_json::json!({});
        let mut receiver = broker
            .register(&pending("ap-old", args.clone(), 1_000))
            .unwrap();
        assert!(
            !broker.respond_at("ap-old", true, &args, NOW),
            "at the expiry instant"
        );
        assert_eq!(receiver.try_recv(), Ok(false));

        let mut receiver = broker
            .register(&pending("ap-no", args.clone(), 1_000))
            .unwrap();
        assert!(broker.respond_at("ap-no", false, &serde_json::Value::Null, NOW));
        assert_eq!(receiver.try_recv(), Ok(false));
    }
}
