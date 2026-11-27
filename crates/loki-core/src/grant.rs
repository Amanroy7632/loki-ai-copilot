//! User grants: which capabilities the user allowed Loki's tools to use, and where.

use serde::{Deserialize, Serialize};

use crate::capability::Capability;

/// Scope meaning "every scope" for a capability (e.g. all allowed roots).
pub const ANY_SCOPE: &str = "*";

/// One active grant. Stored in the `permissions` table; revoked grants are not loaded.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct Grant {
    /// The capability granted.
    pub capability: Capability,
    /// `*`, or a specific scope such as an allowed-root ID for `file_read`.
    pub scope: String,
}

impl Grant {
    /// A grant for every scope of `capability`.
    pub fn any(capability: Capability) -> Self {
        Self {
            capability,
            scope: String::from(ANY_SCOPE),
        }
    }

    /// Whether this grant covers `capability` in `scope`. Exact match or `*`, nothing fuzzier.
    pub fn covers(&self, capability: Capability, scope: &str) -> bool {
        self.capability == capability && (self.scope == ANY_SCOPE || self.scope == scope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_exact_scope_or_wildcard_only() {
        let docs = Grant {
            capability: Capability::FileRead,
            scope: "docs".into(),
        };
        assert!(docs.covers(Capability::FileRead, "docs"));
        assert!(!docs.covers(Capability::FileRead, "docs2"));
        assert!(
            !docs.covers(Capability::FileRead, "*"),
            "a scoped grant is not a wildcard"
        );
        assert!(!docs.covers(Capability::AppLaunch, "docs"));
        assert!(Grant::any(Capability::Clock).covers(Capability::Clock, "anything"));
    }

    #[test]
    fn grant_round_trips_and_rejects_unknown_fields() {
        let grant = Grant::any(Capability::SystemInfo);
        let encoded = serde_json::to_string(&grant).unwrap();
        assert_eq!(serde_json::from_str::<Grant>(&encoded).unwrap(), grant);
        assert!(
            serde_json::from_str::<Grant>(r#"{"capability":"clock","scope":"*","forever":true}"#)
                .is_err()
        );
    }
}
