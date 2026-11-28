//! Payloads the runtime API returns to front ends (Tauri commands from P5). Generated into
//! TypeScript like every other contract type, so the UI never hand-writes IPC types.

use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;

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
