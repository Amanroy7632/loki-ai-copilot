//! Outbound network policy mode (SECURITY_MODEL.md "Network modes").
//!
//! Lives in core (ADR-0011) because the policy engine evaluates it and may only depend on core.

use serde::{Deserialize, Serialize};

/// Which outbound traffic is allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum NetworkMode {
    /// Loopback only (the local model server). The default.
    LocalOnly,
    /// Also the internet tools the user has granted. Still no cloud inference.
    ToolsAllowed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_mode_round_trips_as_snake_case() {
        for (mode, text) in [
            (NetworkMode::LocalOnly, "\"local_only\""),
            (NetworkMode::ToolsAllowed, "\"tools_allowed\""),
        ] {
            assert_eq!(serde_json::to_string(&mode).unwrap(), text);
            assert_eq!(serde_json::from_str::<NetworkMode>(text).unwrap(), mode);
        }
    }
}
