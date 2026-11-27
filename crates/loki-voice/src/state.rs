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
    Muted {
        /// `true` when the session was muted while work was still running (or being cancelled).
        /// The driver must send `CleanupComplete` once that work is joined and the device is
        /// released. Until then, even an unmute can't lead to a new session: it goes to
        /// `Cancelled`, which waits for the same cleanup. This gives mute the same barrier as cancel.
        cleanup_pending: bool,
    },
}

impl VoiceState {
    /// Whether the runtime may capture audio and must display a listening indicator.
    /// This describes desired ownership, not whether an OS device has actually opened.
    pub const fn requires_capture(self) -> bool {
        matches!(self, Self::Listening | Self::Recording)
    }

    /// `true` in either muted state.
    pub const fn is_muted(self) -> bool {
        matches!(self, Self::Muted { .. })
    }

    /// States with work in flight that a cancel, mute or device loss must clean up.
    const fn has_active_work(self) -> bool {
        matches!(
            self,
            Self::Listening
                | Self::Recording
                | Self::Transcribing
                | Self::Thinking
                | Self::Speaking
        )
    }

    fn next(self, event: VoiceEvent) -> Option<Self> {
        use VoiceEvent as E;
        use VoiceState as S;

        let muted = |cleanup_pending| S::Muted { cleanup_pending };
        match (self, event) {
            // Mute: always accepted. It keeps the cleanup obligation of whatever was running.
            (S::Muted { cleanup_pending }, E::Mute) => Some(muted(cleanup_pending)),
            (S::Idle, E::Mute) => Some(muted(false)),
            (state, E::Mute) if state.has_active_work() || state == S::Cancelled => {
                Some(muted(true))
            }
            // Leaving Muted only by Unmute; with cleanup still pending, via Cancelled.
            (
                S::Muted {
                    cleanup_pending: false,
                },
                E::Unmute,
            ) => Some(S::Idle),
            (
                S::Muted {
                    cleanup_pending: true,
                },
                E::Unmute,
            ) => Some(S::Cancelled),
            (
                S::Muted {
                    cleanup_pending: true,
                },
                E::CleanupComplete,
            ) => Some(muted(false)),
            // No completion, cancellation or device event can unmute the session.
            (S::Muted { .. }, _) => None,
            (S::Idle, E::StartListening) => Some(S::Listening),
            (S::Idle | S::Listening, E::PressTalk) => Some(S::Recording),
            (S::Recording, E::ReleaseTalk | E::SilenceTimeout) => Some(S::Transcribing),
            (S::Transcribing, E::Transcribed) => Some(S::Thinking),
            (S::Thinking, E::ResponseReady) => Some(S::Speaking),
            (S::Speaking, E::SpeechDone) => Some(S::Idle),
            // A step that finished with an error (or an empty transcript) has no work left.
            (S::Transcribing | S::Thinking | S::Speaking, E::Failed) => Some(S::Idle),
            (state, E::Cancel | E::DeviceLost) if state.has_active_work() => Some(S::Cancelled),
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
    /// Recognition, the agent turn or speech output ended with an error, or recognition heard
    /// nothing. Send it only once that step's work has fully finished: there is nothing left to
    /// clean up, so the session returns straight to `Idle`. Capture failures use `DeviceLost`
    /// or `Cancel` instead, because an open input stream does need cleanup.
    Failed,
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
/// A future driver must serialize events and discard completions from cancelled sessions. It
/// must send `CleanupComplete` only after cancelled or muted work is joined and the device is
/// released; the machine refuses a new session until then, after a cancel or a mute alike.
/// There is deliberately no action-approval API here.
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

    const MUTED: VoiceState = S::Muted {
        cleanup_pending: false,
    };
    const MUTED_CLEANING: VoiceState = S::Muted {
        cleanup_pending: true,
    };

    const EVENTS: [VoiceEvent; 13] = [
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
        E::Failed,
    ];
    const STATES: [VoiceState; 9] = [
        S::Idle,
        S::Listening,
        S::Recording,
        S::Transcribing,
        S::Thinking,
        S::Speaking,
        S::Cancelled,
        MUTED,
        MUTED_CLEANING,
    ];
    const ACTIVE: [VoiceState; 5] = [
        S::Listening,
        S::Recording,
        S::Transcribing,
        S::Thinking,
        S::Speaking,
    ];

    #[test]
    fn every_state_event_pair_matches_the_transition_table() {
        // Independent expected table: columns follow EVENTS, rows follow STATES.
        // None means rejection without mutation, including late completion events.
        const N: Option<VoiceState> = None;
        let c = Some(S::Cancelled);
        let m = Some(MUTED);
        let mc = Some(MUTED_CLEANING);
        let idle = Some(S::Idle);
        #[rustfmt::skip]
        let expected: [[Option<VoiceState>; 13]; 9] = [
            //            Listen               PressTalk          Release              Silence              Transcribed       ResponseReady    SpeechDone Cancel Mute Unmute DevLost Cleanup Failed
            /* Idle */   [Some(S::Listening), Some(S::Recording), N,                   N,                   N,                N,                N,         idle,  m,   N,     idle,   N,      N],
            /* Listen */ [N,                  Some(S::Recording), N,                   N,                   N,                N,                N,         c,     mc,  N,     c,      N,      N],
            /* Record */ [N,                  N,                  Some(S::Transcribing), Some(S::Transcribing), N,            N,                N,         c,     mc,  N,     c,      N,      N],
            /* Transc */ [N,                  N,                  N,                   N,                   Some(S::Thinking), N,               N,         c,     mc,  N,     c,      N,      idle],
            /* Think  */ [N,                  N,                  N,                   N,                   N,                Some(S::Speaking), N,        c,     mc,  N,     c,      N,      idle],
            /* Speak  */ [N,                  N,                  N,                   N,                   N,                N,                idle,      c,     mc,  N,     c,      N,      idle],
            /* Cancel */ [N,                  N,                  N,                   N,                   N,                N,                N,         c,     mc,  N,     c,      idle,   N],
            /* Muted  */ [N,                  N,                  N,                   N,                   N,                N,                N,         N,     m,   idle,  N,      N,      N],
            /* MutedC */ [N,                  N,                  N,                   N,                   N,                N,                N,         N,     mc,  c,     N,      m,      N],
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
            assert!(machine.apply(E::Mute).unwrap().is_muted());
            for event in EVENTS.into_iter().filter(|event| *event != E::Unmute) {
                let _ = machine.apply(event);
                assert!(machine.state().is_muted(), "{state:?} + {event:?}");
                assert!(!machine.state().requires_capture());
            }
            // Every non-Unmute event has run, including CleanupComplete, so nothing is pending.
            assert_eq!(machine.state(), MUTED);
            assert_eq!(machine.apply(E::Unmute), Ok(S::Idle));
            assert!(!machine.state().requires_capture());
        }
    }

    #[test]
    fn muting_active_work_keeps_the_cleanup_barrier() {
        for state in ACTIVE.into_iter().chain([S::Cancelled]) {
            let mut machine = VoiceMachine { state };
            assert_eq!(machine.apply(E::Mute), Ok(MUTED_CLEANING), "{state:?}");
            // Unmute before cleanup: no new session yet, only the cancel barrier.
            assert_eq!(machine.apply(E::Unmute), Ok(S::Cancelled));
            assert!(
                machine.apply(E::PressTalk).is_err(),
                "{state:?}: talk before cleanup"
            );
            assert!(machine.apply(E::StartListening).is_err());
            assert_eq!(machine.apply(E::CleanupComplete), Ok(S::Idle));
            assert_eq!(machine.apply(E::PressTalk), Ok(S::Recording));
        }
    }

    #[test]
    fn cleanup_while_muted_stays_muted_until_unmute() {
        let mut machine = VoiceMachine {
            state: S::Recording,
        };
        assert_eq!(machine.apply(E::Mute), Ok(MUTED_CLEANING));
        assert_eq!(machine.apply(E::CleanupComplete), Ok(MUTED));
        assert!(machine.apply(E::CleanupComplete).is_err());
        assert!(machine.apply(E::PressTalk).is_err());
        assert_eq!(machine.apply(E::Unmute), Ok(S::Idle));
    }

    #[test]
    fn muting_when_idle_needs_no_cleanup() {
        let mut machine = VoiceMachine::new();
        assert_eq!(machine.apply(E::Mute), Ok(MUTED));
        assert_eq!(machine.apply(E::Unmute), Ok(S::Idle));
    }

    #[test]
    fn cancellation_and_device_loss_require_cleanup_before_restart() {
        for state in ACTIVE {
            for event in [E::Cancel, E::DeviceLost] {
                let mut machine = VoiceMachine { state };
                assert_eq!(machine.apply(event), Ok(S::Cancelled));
                assert!(!machine.state().requires_capture());
                assert!(machine.apply(E::PressTalk).is_err());
                assert!(machine.apply(E::Transcribed).is_err());
                assert!(machine.apply(E::ResponseReady).is_err());
                assert!(machine.apply(E::SpeechDone).is_err());
                assert!(machine.apply(E::Failed).is_err());
                assert_eq!(machine.apply(E::CleanupComplete), Ok(S::Idle));
                assert_eq!(machine.apply(E::PressTalk), Ok(S::Recording));
            }
        }
    }

    #[test]
    fn failures_after_capture_return_to_idle_without_cleanup() {
        for state in [S::Transcribing, S::Thinking, S::Speaking] {
            let mut machine = VoiceMachine { state };
            assert_eq!(machine.apply(E::Failed), Ok(S::Idle), "{state:?}");
            assert_eq!(machine.apply(E::PressTalk), Ok(S::Recording));
        }
        // An open capture stream must be cleaned up, so it can't just "fail" to Idle.
        for state in [S::Listening, S::Recording] {
            let mut machine = VoiceMachine { state };
            assert!(machine.apply(E::Failed).is_err(), "{state:?}");
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
