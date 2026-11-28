//! Payloads the runtime API returns to front ends (Tauri commands from P5). Generated into
//! TypeScript like every other contract type, so the UI never hand-writes IPC types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ids::{ApprovalId, RequestId, ToolName};
use crate::message::{Conversation, Message};
use crate::time::UnixMillis;
use crate::tool::{Risk, ToolStatus};

/// What the desktop shell shows at startup and in its status area.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct AppStatus {
    /// Loki version, e.g. `0.0.1`.
    pub version: String,
    /// Provider name, `ollama` or `mock`. A mock is always labelled as such.
    pub provider: String,
    /// The model in use, if the provider has one.
    pub model: Option<String>,
    /// `true` only when the provider can serve a chat right now.
    pub ready: bool,
    /// Why it isn't ready, or a startup note (e.g. "no model selected"). Shown to the user.
    pub detail: Option<String>,
    /// Whether tools are offered to the model.
    pub tools_enabled: bool,
}

/// An approval request as the UI shows it: the tool, its risk, **every** argument and the expiry.
/// The UI answers with the `id`; the answer is bound to this exact request and is single use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct ApprovalPreview {
    /// Approval ID to answer with.
    pub id: ApprovalId,
    /// The tool request it is bound to.
    pub request_id: RequestId,
    /// Tool name.
    pub tool: ToolName,
    /// Risk class, for the plain-language explanation.
    pub risk: Risk,
    /// Every argument, exactly as it will run.
    pub arguments: Value,
    /// After this instant the approval is refused, even if the user says yes.
    pub expires_at: UnixMillis,
}

/// One event of a streamed chat turn. Each turn ends with exactly one of `completed`, `stopped`
/// or `failed`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TurnEvent {
    /// A new conversation was created for this turn (first message).
    Started {
        /// The new conversation.
        conversation: Conversation,
    },
    /// A piece of reply text.
    Token {
        /// The text fragment.
        text: String,
    },
    /// A tool needs the user's approval; answer with `respond_approval`.
    ApprovalRequested {
        /// What to show.
        approval: ApprovalPreview,
    },
    /// A tool call finished (any status).
    ToolFinished {
        /// Tool name.
        tool: String,
        /// Final status.
        status: ToolStatus,
        /// Short reason for a non-success.
        detail: Option<String>,
    },
    /// The reply finished and was stored.
    Completed {
        /// The stored reply.
        message: Message,
    },
    /// The user stopped it; partial text, if any, was stored marked `stopped`.
    Stopped {
        /// The stored partial reply, if there was text.
        message: Option<Message>,
    },
    /// The turn failed (provider down, storage error...). The user's message is kept.
    Failed {
        /// What went wrong, for the user.
        error: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_events_round_trip_with_a_type_tag() {
        let events = [
            TurnEvent::Token { text: "hi".into() },
            TurnEvent::ToolFinished {
                tool: "time.now".into(),
                status: ToolStatus::Succeeded,
                detail: None,
            },
            TurnEvent::Stopped { message: None },
            TurnEvent::Failed {
                error: "down".into(),
            },
            TurnEvent::ApprovalRequested {
                approval: ApprovalPreview {
                    id: ApprovalId::new("ap"),
                    request_id: RequestId::new("rq"),
                    tool: ToolName::new("app.open"),
                    risk: Risk::LocalChange,
                    arguments: serde_json::json!({ "app_id": "x" }),
                    expires_at: UnixMillis(5),
                },
            },
        ];
        for event in events {
            let json = serde_json::to_value(&event).unwrap();
            assert!(json["type"].is_string());
            assert_eq!(serde_json::from_value::<TurnEvent>(json).unwrap(), event);
        }
        assert_eq!(
            serde_json::to_value(TurnEvent::Token { text: "a".into() }).unwrap()["type"],
            "token"
        );
        assert!(
            serde_json::from_value::<TurnEvent>(
                serde_json::json!({ "type": "token", "text": "a", "x": 1 })
            )
            .is_err()
        );
    }

    #[test]
    fn app_status_round_trips_and_rejects_unknown_fields() {
        let status = AppStatus {
            version: "0.0.1".into(),
            provider: "mock".into(),
            model: None,
            ready: true,
            detail: Some("no model selected".into()),
            tools_enabled: true,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(
            serde_json::from_value::<AppStatus>(json.clone()).unwrap(),
            status
        );
        let mut extra = json;
        extra["admin"] = serde_json::json!(true);
        assert!(serde_json::from_value::<AppStatus>(extra).is_err());
    }
}
