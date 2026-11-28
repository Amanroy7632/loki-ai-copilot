//! Desktop lifecycle payloads, separate from model configuration.

use crate::CapabilityStatus;
use serde::{Deserialize, Serialize};

/// Actual OS registration state, never an assumed or merely saved preference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct AutostartStatus {
    /// Null when unavailable or the OS query failed; not silently treated as disabled.
    pub enabled: Option<bool>,
    /// Availability or the reason the toggle cannot be used.
    pub availability: CapabilityStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_status_round_trips_and_rejects_unknown_fields() {
        for status in [
            AutostartStatus {
                enabled: Some(true),
                availability: CapabilityStatus::Supported,
            },
            AutostartStatus {
                enabled: None,
                availability: CapabilityStatus::Unavailable {
                    reason: "development build".into(),
                },
            },
        ] {
            let mut json = serde_json::to_value(&status).unwrap();
            assert_eq!(
                serde_json::from_value::<AutostartStatus>(json.clone()).unwrap(),
                status
            );
            json["registered"] = serde_json::json!(true);
            assert!(serde_json::from_value::<AutostartStatus>(json).is_err());
        }
    }
}
