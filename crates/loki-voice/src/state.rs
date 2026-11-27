//! Pure session lifecycle. Device ownership and cancellation are handled by the future runtime
//! driver, which must finish cleanup before sending `CleanupComplete`.

use std::fmt;

/// A session's lifecycle, independent of audio devices or asynchronous work.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    /// No session is running and the microphone must be released.
    #[default]
    Idle,
    /// An explicitly enabled wake-word session is waiting for speech.
    Listening,
    /// Capturing the current utterance in memory.
    Recording,
    /// Capture has stopped and local recognition is running.
    Transcribing,
    /// The agent is processing the transcript; it cannot approve actions.
    Thinking,
    /// Speech output is running with recognition paused.
    Speaking,
    /// Work is cancelled; wait for resource cleanup before accepting another session.
    Cancelled,
    /// Capture is forbidden until the user explicitly unmutes.
    Muted,
}

impl VoiceState {
    /// Whether the runtime may capture audio and must display a listening indicator.
    /// This describes desired ownership, not whether an OS device has actually opened.
    pub const fn requires_capture(self) -> bool {
        matches!(self, Self::Listening | Self::Recording)
    }

    fn next(self, event: VoiceEvent) -> Option<Self> {
        use VoiceEvent as E;
        use VoiceState as S;

        match (self, event) {
            (_, E::Mute) => Some(S::Muted),
            (S::Muted, E::Unmute) => Some(S::Idle),
            // No completion, cancellation or device event can unmute the session.
            (S::Muted, _) => None,
            (S::Idle, E::StartListening) => Some(S::Listening),
            (S::Idle | S::Listening, E::PressTalk) => Some(S::Recording),
            (S::Recording, E::ReleaseTalk | E::SilenceTimeout) => Some(S::Transcribing),
            (S::Transcribing, E::Transcribed) => Some(S::Thinking),
            (S::Thinking, E::ResponseReady) => Some(S::Speaking),
            (S::Speaking, E::SpeechDone) => Some(S::Idle),
            (
                S::Listening | S::Recording | S::Transcribing | S::Thinking | S::Speaking,
                E::Cancel | E::DeviceLost,
            ) => Some(S::Cancelled),
            (S::Idle, E::Cancel | E::DeviceLost) => Some(S::Idle),
            (S::Cancelled, E::Cancel | E::DeviceLost) => Some(S::Cancelled),
            (S::Cancelled, E::CleanupComplete) => Some(S::Idle),
            _ => None,
        }
    }
}

/// Typed control/completion events. Only explicit user controls may produce `Unmute`;
/// transcripts and tool results must never be interpreted as control events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceEvent {
    /// Explicitly arm listening, only when the future wake-word feature is enabled.
    /// This event supplies the diagram's Idle → Listening edge, not a wake-word engine.
    StartListening,
    /// Begin a push-to-talk utterance (also usable by the future wake-word driver).
    PressTalk,
    /// Stop recording and begin recognition.
    ReleaseTalk,
    /// The configured silence limit ended the utterance.
    SilenceTimeout,
    /// Recognition finished; pass its text to the agent as user input, never approval.
    Transcribed,
    /// The agent reply is ready for speech output.
    ResponseReady,
    /// Speech output finished normally.
    SpeechDone,
    /// Emergency stop: cancel capture, recognition, the agent turn and speech output.
    Cancel,
    /// Cancel active work and release the microphone until an explicit unmute.
    Mute,
    /// Explicit user action to leave Muted; it does not start capture.
    Unmute,
    /// Device removal aborts the current session.
    DeviceLost,
    /// The driver has joined cancelled work and released its resources.
    CleanupComplete,
}

/// An event that is out of order. The machine keeps its previous state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTransition {
    /// State when the event was rejected.
    pub state: VoiceState,
    /// Rejected control or completion event.
    pub event: VoiceEvent,
}

impl fmt::Display for InvalidTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "voice event {:?} is invalid in {:?}",
            self.event, self.state
        )
    }
}

impl std::error::Error for InvalidTransition {}

/// One session's pure state machine. Constructed idle; it owns no device or audio buffer.
///
/// A future driver must serialize events, discard completions from cancelled sessions, and
/// cancel/join work on mute before permitting a fresh session. State alone does not release
/// devices or cancel futures. There is deliberately no action-approval API here.
#[derive(Debug, Default)]
pub struct VoiceMachine {
    state: VoiceState,
}

impl VoiceMachine {
    /// Starts with no capture, including when wake-word support is eventually enabled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Current state for the driver and its visible indicator.
    pub const fn state(&self) -> VoiceState {
        self.state
    }

    /// Applies a legal event, returning the resulting state. Rejected events have no effect.
    pub fn apply(&mut self, event: VoiceEvent) -> Result<VoiceState, InvalidTransition> {
        let next = self.state.next(event).ok_or(InvalidTransition {
            state: self.state,
            event,
        })?;
        self.state = next;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use VoiceEvent as E;
    use VoiceState as S;

    const EVENTS: [VoiceEvent; 12] = [
        E::StartListening,
        E::PressTalk,
        E::ReleaseTalk,
        E::SilenceTimeout,
        E::Transcribed,
        E::ResponseReady,
        E::SpeechDone,
        E::Cancel,
        E::Mute,
        E::Unmute,
        E::DeviceLost,
        E::CleanupComplete,
    ];
    const STATES: [VoiceState; 8] = [
        S::Idle,
        S::Listening,
        S::Recording,
        S::Transcribing,
        S::Thinking,
        S::Speaking,
        S::Cancelled,
        S::Muted,
    ];

    #[test]
    fn every_state_event_pair_matches_the_transition_table() {
        // Independent expected table: columns follow EVENTS, rows follow STATES.
        // None means rejection without mutation, including late completion events.
        let expected = [
            [
                Some(S::Listening),
                Some(S::Recording),
                None,
                None,
                None,
                None,
                None,
                Some(S::Idle),
                Some(S::Muted),
                None,
                Some(S::Idle),
                None,
            ],
            [
                None,
                Some(S::Recording),
                None,
                None,
                None,
                None,
                None,
                Some(S::Cancelled),
                Some(S::Muted),
                None,
                Some(S::Cancelled),
                None,
            ],
            [
                None,
                None,
                Some(S::Transcribing),
                Some(S::Transcribing),
                None,
                None,
                None,
                Some(S::Cancelled),
                Some(S::Muted),
                None,
                Some(S::Cancelled),
                None,
            ],
            [
                None,
                None,
                None,
                None,
                Some(S::Thinking),
                None,
                None,
                Some(S::Cancelled),
                Some(S::Muted),
                None,
                Some(S::Cancelled),
                None,
            ],
            [
                None,
                None,
                None,
                None,
                None,
                Some(S::Speaking),
                None,
                Some(S::Cancelled),
                Some(S::Muted),
                None,
                Some(S::Cancelled),
                None,
            ],
            [
                None,
                None,
                None,
                None,
                None,
                None,
                Some(S::Idle),
                Some(S::Cancelled),
                Some(S::Muted),
                None,
                Some(S::Cancelled),
                None,
            ],
            [
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(S::Cancelled),
                Some(S::Muted),
                None,
                Some(S::Cancelled),
                Some(S::Idle),
            ],
            [
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(S::Muted),
                Some(S::Idle),
                None,
                None,
            ],
        ];
        for (state, row) in STATES.into_iter().zip(expected) {
            for (event, next) in EVENTS.into_iter().zip(row) {
                let mut machine = VoiceMachine { state };
                assert_eq!(
                    machine.apply(event),
                    next.ok_or(InvalidTransition { state, event }),
                    "{state:?} + {event:?}",
                );
                assert_eq!(machine.state(), next.unwrap_or(state));
            }
        }
    }

    #[test]
    fn push_to_talk_round_trip_pauses_capture_before_recognition_and_speech() {
        let mut machine = VoiceMachine::new();
        assert_eq!(machine.state(), S::Idle);
        assert!(!machine.state().requires_capture());
        for (event, state, capture) in [
            (E::PressTalk, S::Recording, true),
            (E::ReleaseTalk, S::Transcribing, false),
            (E::Transcribed, S::Thinking, false),
            (E::ResponseReady, S::Speaking, false),
            (E::SpeechDone, S::Idle, false),
        ] {
            assert_eq!(machine.apply(event), Ok(state));
            assert_eq!(machine.state().requires_capture(), capture);
        }
    }

    #[test]
    fn listening_is_explicit_and_silence_ends_recording() {
        let mut machine = VoiceMachine::new();
        assert_eq!(machine.apply(E::StartListening), Ok(S::Listening));
        assert!(machine.state().requires_capture());
        assert_eq!(machine.apply(E::PressTalk), Ok(S::Recording));
        assert_eq!(machine.apply(E::SilenceTimeout), Ok(S::Transcribing));
        assert!(!machine.state().requires_capture());
    }

    #[test]
    fn only_explicit_unmute_can_leave_muted_from_every_starting_state() {
        for state in STATES {
            let mut machine = VoiceMachine { state };
            assert_eq!(machine.apply(E::Mute), Ok(S::Muted));
            for event in EVENTS.into_iter().filter(|event| *event != E::Unmute) {
                let _ = machine.apply(event);
                assert_eq!(machine.state(), S::Muted, "{state:?} + {event:?}");
                assert!(!machine.state().requires_capture());
            }
            assert_eq!(machine.apply(E::Unmute), Ok(S::Idle));
            assert!(!machine.state().requires_capture());
        }
    }

    #[test]
    fn cancellation_and_device_loss_require_cleanup_before_restart() {
        for state in [
            S::Listening,
            S::Recording,
            S::Transcribing,
            S::Thinking,
            S::Speaking,
        ] {
            for event in [E::Cancel, E::DeviceLost] {
                let mut machine = VoiceMachine { state };
                assert_eq!(machine.apply(event), Ok(S::Cancelled));
                assert!(!machine.state().requires_capture());
                assert!(machine.apply(E::PressTalk).is_err());
                assert!(machine.apply(E::Transcribed).is_err());
                assert!(machine.apply(E::ResponseReady).is_err());
                assert!(machine.apply(E::SpeechDone).is_err());
                assert_eq!(machine.apply(E::CleanupComplete), Ok(S::Idle));
                assert_eq!(machine.apply(E::PressTalk), Ok(S::Recording));
            }
        }
    }

    #[test]
    fn speaking_rejects_capture_and_transcription_events() {
        let mut machine = VoiceMachine { state: S::Speaking };
        for event in [
            E::StartListening,
            E::PressTalk,
            E::ReleaseTalk,
            E::SilenceTimeout,
            E::Transcribed,
        ] {
            assert!(machine.apply(event).is_err());
            assert_eq!(machine.state(), S::Speaking);
            assert!(!machine.state().requires_capture());
        }
    }
}
