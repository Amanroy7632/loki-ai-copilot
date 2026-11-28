//! Settings and data-control payloads. No paths, credentials or arbitrary config keys cross IPC.

use serde::{Deserialize, Serialize};

use crate::{CapabilityStatus, Conversation, Message, NetworkMode, UnixMillis};

/// The only implemented inference backends; cloud inference is never an option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum SettingsProvider {
    /// Explicitly labelled test/demo replies.
    Mock,
    /// A local Ollama server.
    Ollama,
}

/// Editable settings, applied together after validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct DesktopSettings {
    /// Selected inference backend.
    pub provider: SettingsProvider,
    /// Local model tag, required for Ollama.
    pub model_id: Option<String>,
    /// Internet tools still require individual grants.
    pub network_mode: NetworkMode,
    /// Conversations inactive for this many days expire; null keeps them until cleared.
    pub history_days: Option<u32>,
}

/// Settings with honest availability for controls implemented in later tickets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct SettingsSnapshot {
    /// Effective values, including session provider/model overrides.
    pub values: DesktopSettings,
    /// Personal memory storage availability.
    pub memory: CapabilityStatus,
    /// Start-at-login integration availability.
    pub autostart: CapabilityStatus,
    /// Wake-word integration availability.
    pub wake_word: CapabilityStatus,
}

/// A conversation with all its messages in an export snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct ExportConversation {
    /// Title, ID and timestamps.
    pub conversation: Conversation,
    /// Messages in chronological order, including stopped replies.
    pub messages: Vec<Message>,
}

/// Versioned JSON export. Audit, grants, config and credentials are excluded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct DataExport {
    /// Export schema version (currently 1).
    pub version: u32,
    /// When the snapshot was taken.
    pub exported_at: UnixMillis,
    /// Complete history at that instant.
    pub history: Vec<ExportConversation>,
    /// False until personal memory storage ships; distinguishes unavailable from empty.
    pub memory_available: bool,
    /// Empty until the memory schema ships.
    pub memories: Vec<serde_json::Value>,
}

/// Redacted audit row for display; raw requests and tool output never cross this API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct AuditEntry {
    /// Run identifier.
    pub id: String,
    /// Tool name.
    pub tool: String,
    /// Final execution status.
    pub status: String,
    /// Redacted summary only.
    pub summary: Option<String>,
    /// Completion time.
    pub created_at: UnixMillis,
}

/// A failed mutation may already have saved settings or logically deleted history.
/// Front ends must refresh cached state in that case while showing the cleanup error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct DataControlError {
    /// Explanation, including cleanup/retry instructions when applicable.
    pub message: String,
    /// True when the operation changed state before failing.
    pub state_changed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(
        value: T,
    ) {
        let mut json = serde_json::to_value(&value).unwrap();
        assert_eq!(serde_json::from_value::<T>(json.clone()).unwrap(), value);
        json["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<T>(json).is_err());
    }

    #[test]
    fn settings_export_and_audit_contracts_round_trip_and_reject_extra_fields() {
        round_trip(DataControlError {
            message: "cleanup incomplete".into(),
            state_changed: true,
        });
        let values = DesktopSettings {
            provider: SettingsProvider::Mock,
            model_id: None,
            network_mode: NetworkMode::LocalOnly,
            history_days: Some(30),
        };
        round_trip(values.clone());
        let unavailable = CapabilityStatus::Unavailable {
            reason: "not shipped".into(),
        };
        round_trip(SettingsSnapshot {
            values,
            memory: unavailable.clone(),
            autostart: unavailable.clone(),
            wake_word: unavailable,
        });
        let history = ExportConversation {
            conversation: Conversation {
                id: crate::ConversationId::new("c"),
                title: "title".into(),
                created_at: UnixMillis(1),
                updated_at: UnixMillis(1),
            },
            messages: Vec::new(),
        };
        round_trip(history.clone());
        round_trip(DataExport {
            version: 1,
            exported_at: UnixMillis(2),
            history: vec![history],
            memory_available: false,
            memories: vec![],
        });
        round_trip(AuditEntry {
            id: "r".into(),
            tool: "time.now".into(),
            status: "succeeded".into(),
            summary: None,
            created_at: UnixMillis(1),
        });
        for provider in [SettingsProvider::Mock, SettingsProvider::Ollama] {
            let json = serde_json::to_value(provider).unwrap();
            assert_eq!(
                serde_json::from_value::<SettingsProvider>(json).unwrap(),
                provider
            );
        }
        assert!(serde_json::from_str::<SettingsProvider>("\"cloud\"").is_err());
    }
}
