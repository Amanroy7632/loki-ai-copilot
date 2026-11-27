//! Strongly typed identifiers.
//!
//! IDs are opaque strings in contracts. The **format** is decided outside core: `loki-runtime`
//! generates UUID v7 strings (ADR-0004), which sort by creation time, so SQLite primary-key
//! order roughly follows insertion order. Core stays free of a UUID dependency.
//!
//! Wrapping each kind in its own type stops a [`SessionId`] from being passed where a
//! [`RequestId`] is expected, a mistake that would otherwise compile.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Generates a transparent string newtype with `new`, `as_str` and `Display`.
macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Wraps an existing identifier string.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Returns the identifier as a string slice.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_id! {
    /// Identifies one tool request. Approvals and audit rows are bound to it.
    RequestId
}

string_id! {
    /// Identifies a conversation session.
    SessionId
}

string_id! {
    /// Stable dotted tool name, such as `app.open` or `files.search`.
    ///
    /// The tool registry (P3-T01) validates the format and rejects duplicate names.
    ToolName
}

string_id! {
    /// Identifies a stored conversation (a row in `conversations`).
    ConversationId
}

string_id! {
    /// Identifies a stored message (a row in `messages`).
    MessageId
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_serialize_as_plain_strings_and_round_trip() {
        let id = ConversationId::new("0193a1b2-0000-7000-8000-000000000001");
        let json = serde_json::to_value(&id).unwrap();
        assert_eq!(json, "0193a1b2-0000-7000-8000-000000000001");
        assert_eq!(serde_json::from_value::<ConversationId>(json).unwrap(), id);

        for raw in ["req-1", "ses-1", "time.now", "msg-1"] {
            let encoded = serde_json::to_string(raw).unwrap();
            assert_eq!(
                serde_json::from_str::<RequestId>(&encoded)
                    .unwrap()
                    .as_str(),
                raw
            );
            assert_eq!(
                serde_json::from_str::<SessionId>(&encoded)
                    .unwrap()
                    .as_str(),
                raw
            );
            assert_eq!(
                serde_json::from_str::<ToolName>(&encoded).unwrap().as_str(),
                raw
            );
            assert_eq!(
                serde_json::from_str::<MessageId>(&encoded)
                    .unwrap()
                    .as_str(),
                raw
            );
        }
    }

    #[test]
    fn ids_reject_non_string_json() {
        assert!(serde_json::from_str::<MessageId>("42").is_err());
        assert!(serde_json::from_str::<ConversationId>("{\"id\":\"x\"}").is_err());
    }

    #[test]
    fn display_matches_inner_string() {
        assert_eq!(MessageId::new("m1").to_string(), "m1");
    }
}
