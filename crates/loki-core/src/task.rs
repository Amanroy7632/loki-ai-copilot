//! Task lifecycle.
//!
//! ```text
//! Queued ──► AwaitingApproval ──► Running ──► Succeeded
//!   │               │                │
//!   ├──► Running    ├──► Failed      ├──► Failed
//!   ├──► Failed     ├──► Cancelled   ├──► Cancelled
//!   ├──► Cancelled  └──► Interrupted └──► Interrupted
//!   └──► Interrupted
//! ```
//!
//! `Interrupted` is assigned on startup to any task that wasn't in a terminal state when Loki
//! stopped. Interrupted tasks are **never replayed automatically**, because replaying could
//! repeat a side effect.

use serde::{Deserialize, Serialize};

/// The state of one task (normally one tool execution).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    /// Accepted and waiting for policy evaluation or a worker slot.
    Queued,
    /// Executing.
    Running,
    /// Waiting for the user to approve the exact request.
    AwaitingApproval,
    /// Finished successfully, with a verified outcome.
    Succeeded,
    /// Finished with an error, or was denied by policy.
    Failed,
    /// Stopped by the user, or an approval was rejected or expired.
    Cancelled,
    /// Loki stopped before the task finished. Never resumed automatically.
    Interrupted,
}

impl TaskState {
    /// Every state, for exhaustive tests and UI legends.
    pub const ALL: [Self; 7] = [
        Self::Queued,
        Self::Running,
        Self::AwaitingApproval,
        Self::Succeeded,
        Self::Failed,
        Self::Cancelled,
        Self::Interrupted,
    ];

    /// Terminal states never change again.
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }

    /// Whether moving from `self` to `next` is a legal transition.
    ///
    /// Persisting a state change should go through this check, so an illegal transition (for
    /// example, skipping approval) is a bug that tests catch.
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Queued,
                Self::AwaitingApproval
                    | Self::Running
                    | Self::Failed
                    | Self::Cancelled
                    | Self::Interrupted
            ) | (
                Self::AwaitingApproval,
                Self::Running | Self::Failed | Self::Cancelled | Self::Interrupted
            ) | (
                Self::Running,
                Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted
            )
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_states_have_no_outgoing_transitions() {
        for from in TaskState::ALL
            .into_iter()
            .filter(|state| state.is_terminal())
        {
            for to in TaskState::ALL {
                assert!(
                    !from.can_transition_to(to),
                    "{from:?} -> {to:?} must be illegal"
                );
            }
        }
    }

    #[test]
    fn approval_path_is_legal() {
        assert!(TaskState::Queued.can_transition_to(TaskState::AwaitingApproval));
        assert!(TaskState::AwaitingApproval.can_transition_to(TaskState::Running));
        assert!(TaskState::Running.can_transition_to(TaskState::Succeeded));
    }

    #[test]
    fn cannot_succeed_without_running() {
        assert!(!TaskState::Queued.can_transition_to(TaskState::Succeeded));
        assert!(!TaskState::AwaitingApproval.can_transition_to(TaskState::Succeeded));
    }

    #[test]
    fn running_cannot_go_back_to_approval() {
        assert!(!TaskState::Running.can_transition_to(TaskState::AwaitingApproval));
        assert!(!TaskState::Running.can_transition_to(TaskState::Queued));
    }

    #[test]
    fn every_state_round_trips() {
        for state in TaskState::ALL {
            let encoded = serde_json::to_string(&state).unwrap();
            assert_eq!(serde_json::from_str::<TaskState>(&encoded).unwrap(), state);
        }
    }

    #[test]
    fn serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_value(TaskState::AwaitingApproval).unwrap(),
            "awaiting_approval"
        );
    }
}
