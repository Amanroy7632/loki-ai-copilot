//! Voice status for front ends (P5-T08). `loki-runtime` derives it from the `loki-voice` state
//! machine; the UI only displays it, so the listening indicator can't drift from the real state.

use serde::{Deserialize, Serialize};

use crate::CapabilityStatus;

/// What a voice session is doing, as the UI names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum VoicePhase {
    /// Nothing running; the microphone is released.
    Idle,
    /// Waiting for the wake word (only when that feature is enabled).
    Listening,
    /// Capturing the current utterance.
    Recording,
    /// Recognising speech locally.
    Transcribing,
    /// The assistant is working on the request.
    Thinking,
    /// Speaking the reply.
    Speaking,
    /// Stopping: cancelled work is being cleaned up.
    Stopping,
    /// Capture is forbidden until the user unmutes.
    Muted,
}

/// The voice state a front end shows (P5-T08).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct VoiceStatus {
    /// Current phase.
    pub phase: VoicePhase,
    /// The microphone may be open. While `true` the listening indicator must be visible.
    pub mic_open: bool,
    /// Voice is muted: nothing can start capture until an explicit unmute.
    pub muted: bool,
    /// Whether push-to-talk can be used here, and why not.
    pub push_to_talk: CapabilityStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_status_round_trips_and_rejects_unknown_fields() {
        let status = VoiceStatus {
            phase: VoicePhase::Recording,
            mic_open: true,
            muted: false,
            push_to_talk: CapabilityStatus::Supported,
        };
        let mut json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["phase"], "recording");
        assert_eq!(
            serde_json::from_value::<VoiceStatus>(json.clone()).unwrap(),
            status
        );
        json["listening"] = serde_json::json!(false);
        assert!(serde_json::from_value::<VoiceStatus>(json).is_err());
    }
}
