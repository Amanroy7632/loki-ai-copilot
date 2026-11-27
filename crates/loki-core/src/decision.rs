//! Policy decisions.

use serde::{Deserialize, Serialize};

/// The outcome of evaluating a tool request against policy (`loki-policy`).
///
/// The policy engine must **fail closed**: anything it can't positively allow becomes
/// [`Decision::Deny`] or [`Decision::RequireApproval`], never [`Decision::Allow`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
pub enum Decision {
    /// Execute without asking the user.
    Allow,
    /// Never execute this request.
    Deny {
        /// Recorded in the audit log and shown to the user.
        reason: String,
    },
    /// Execute only after the user approves these exact arguments (see `docs/CONTRACTS.md`).
    RequireApproval,
}

impl Decision {
    /// `true` only for [`Decision::Allow`].
    pub const fn is_allow(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decision_round_trips() {
        for decision in [
            Decision::Allow,
            Decision::Deny {
                reason: "capability unavailable".into(),
            },
            Decision::RequireApproval,
        ] {
            let encoded = serde_json::to_string(&decision).unwrap();
            assert_eq!(
                serde_json::from_str::<Decision>(&encoded).unwrap(),
                decision
            );
        }
    }

    #[test]
    fn decision_wire_format_is_tagged() {
        assert_eq!(
            serde_json::to_value(Decision::RequireApproval).unwrap(),
            json!({ "decision": "require_approval" })
        );
    }

    #[test]
    fn decision_rejects_unknown_fields_and_tags() {
        let extra = json!({ "decision": "deny", "reason": "x", "approved": true });
        assert!(serde_json::from_value::<Decision>(extra).is_err());
        assert!(serde_json::from_value::<Decision>(json!({ "decision": "maybe" })).is_err());
    }

    #[test]
    fn only_allow_is_allow() {
        assert!(Decision::Allow.is_allow());
        assert!(!Decision::RequireApproval.is_allow());
        assert!(!Decision::Deny { reason: "x".into() }.is_allow());
    }
}
