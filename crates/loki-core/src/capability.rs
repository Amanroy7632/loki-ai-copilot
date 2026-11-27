//! Platform capabilities and their runtime status.
//!
//! Loki never assumes an OS feature exists. Platform adapters (`loki-platform`) report a
//! [`CapabilityStatus`] for each [`Capability`]. The policy engine refuses any tool whose
//! capability isn't `Supported`, and the UI hides or explains controls based on the same status.

use serde::{Deserialize, Serialize};

/// Something a tool needs from the operating system or environment.
///
/// Adding a variant is a contract change. Update `docs/CONTRACTS.md` and every platform adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Read the clock and time zone.
    Clock,
    /// Read OS, CPU, memory and disk information.
    SystemInfo,
    /// Enumerate and launch installed applications.
    AppLaunch,
    /// Search and read files inside user-approved roots.
    FileRead,
    /// Make outbound internet requests (web search and fetch).
    Network,
    /// Capture microphone audio.
    Microphone,
    /// Show OS notifications.
    Notifications,
    /// Query local developer tooling such as Docker (read-only).
    DeveloperTools,
}

/// Whether a capability can be used right now, as reported by a platform adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum CapabilityStatus {
    /// Available and permitted.
    Supported,
    /// Available once the user grants an OS permission.
    PermissionRequired {
        /// Explanation shown to the user, e.g. which OS setting to change.
        reason: String,
    },
    /// The user or the OS refused the permission.
    Denied {
        /// Explanation shown to the user.
        reason: String,
    },
    /// Not possible on this platform, build or hardware.
    Unavailable {
        /// Explanation shown to the user.
        reason: String,
    },
}

impl Capability {
    /// Every capability, for exhaustive tests and listings.
    pub const ALL: [Self; 8] = [
        Self::Clock,
        Self::SystemInfo,
        Self::AppLaunch,
        Self::FileRead,
        Self::Network,
        Self::Microphone,
        Self::Notifications,
        Self::DeveloperTools,
    ];

    /// The stable snake_case name (same as serde), used in the `permissions` table and the CLI.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clock => "clock",
            Self::SystemInfo => "system_info",
            Self::AppLaunch => "app_launch",
            Self::FileRead => "file_read",
            Self::Network => "network",
            Self::Microphone => "microphone",
            Self::Notifications => "notifications",
            Self::DeveloperTools => "developer_tools",
        }
    }

    /// Parses [`Capability::as_str`]. `None` for anything else.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == value)
    }
}

impl CapabilityStatus {
    /// `true` only for [`CapabilityStatus::Supported`]. Every other status must block execution.
    pub const fn is_supported(&self) -> bool {
        matches!(self, Self::Supported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_supported_is_supported() {
        assert!(CapabilityStatus::Supported.is_supported());
        let reason = String::from("test");
        assert!(
            !CapabilityStatus::PermissionRequired {
                reason: reason.clone()
            }
            .is_supported()
        );
        assert!(
            !CapabilityStatus::Denied {
                reason: reason.clone()
            }
            .is_supported()
        );
        assert!(!CapabilityStatus::Unavailable { reason }.is_supported());
    }

    #[test]
    fn status_serializes_with_a_tag() {
        let json = serde_json::to_value(CapabilityStatus::Denied {
            reason: "Microphone access is off in System Settings".into(),
        })
        .unwrap();
        assert_eq!(json["status"], "denied");
        assert_eq!(
            serde_json::to_value(Capability::AppLaunch).unwrap(),
            "app_launch"
        );
    }

    #[test]
    fn capability_as_str_matches_serde() {
        for capability in Capability::ALL {
            assert_eq!(
                serde_json::to_value(capability).unwrap(),
                capability.as_str()
            );
            assert_eq!(Capability::parse(capability.as_str()), Some(capability));
        }
        assert_eq!(Capability::parse("root"), None);
    }

    #[test]
    fn capability_and_status_round_trip() {
        for capability in [
            Capability::Clock,
            Capability::SystemInfo,
            Capability::AppLaunch,
            Capability::FileRead,
            Capability::Network,
            Capability::Microphone,
            Capability::Notifications,
            Capability::DeveloperTools,
        ] {
            let encoded = serde_json::to_string(&capability).unwrap();
            assert_eq!(
                serde_json::from_str::<Capability>(&encoded).unwrap(),
                capability
            );
        }
        for status in [
            CapabilityStatus::Supported,
            CapabilityStatus::PermissionRequired { reason: "a".into() },
            CapabilityStatus::Denied { reason: "b".into() },
            CapabilityStatus::Unavailable { reason: "c".into() },
        ] {
            let encoded = serde_json::to_string(&status).unwrap();
            assert_eq!(
                serde_json::from_str::<CapabilityStatus>(&encoded).unwrap(),
                status
            );
        }
    }

    #[test]
    fn status_rejects_unknown_fields_and_unknown_tags() {
        let extra = serde_json::json!({ "status": "denied", "reason": "x", "override": true });
        assert!(serde_json::from_value::<CapabilityStatus>(extra).is_err());
        let unknown = serde_json::json!({ "status": "granted" });
        assert!(serde_json::from_value::<CapabilityStatus>(unknown).is_err());
    }
}
