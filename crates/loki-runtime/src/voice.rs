//! Voice session control for front ends (P5-T08).
//!
//! `VoiceControl` owns the `loki-voice` state machine and publishes every change as a
//! [`VoiceStatus`]. The listening indicator comes from `VoiceState::requires_capture`, so the UI
//! can't show the microphone as off while a state that may capture is active.
//!
//! Push-to-talk is **unavailable** until local speech-to-text exists (P4-T04): opening the
//! microphone with nothing to transcribe would show an unsupported feature as working. Mute
//! works now: while muted, nothing can start a voice session.

use std::sync::{Arc, Mutex};

use loki_core::{CapabilityStatus, VoicePhase, VoiceStatus};
use loki_voice::{VoiceEvent, VoiceMachine, VoiceState};

use crate::error::RuntimeError;

/// Receives every status change. Returns `false` once the receiver is gone; it is then dropped.
/// Must not block (it typically forwards to an IPC channel or updates a tray item).
pub type VoiceSink = Arc<dyn Fn(&VoiceStatus) -> bool + Send + Sync>;

/// Why push-to-talk can't be used yet.
pub const NO_SPEECH_TO_TEXT: &str =
    "Push-to-talk needs local speech-to-text, which isn't installed yet. The microphone stays off.";

/// The front-end status for a state machine state. Pure, so it can be checked for every state.
pub fn status_for(state: VoiceState, push_to_talk: &CapabilityStatus) -> VoiceStatus {
    let phase = match state {
        VoiceState::Idle => VoicePhase::Idle,
        VoiceState::Listening => VoicePhase::Listening,
        VoiceState::Recording => VoicePhase::Recording,
        VoiceState::Transcribing => VoicePhase::Transcribing,
        VoiceState::Thinking => VoicePhase::Thinking,
        VoiceState::Speaking => VoicePhase::Speaking,
        VoiceState::Cancelled => VoicePhase::Stopping,
        VoiceState::Muted { .. } => VoicePhase::Muted,
    };
    VoiceStatus {
        phase,
        mic_open: state.requires_capture(),
        muted: state.is_muted(),
        push_to_talk: push_to_talk.clone(),
    }
}

/// A cheap, cloneable handle; usable while a chat turn holds the runtime lock.
#[derive(Clone)]
pub struct VoiceControl {
    inner: Arc<Inner>,
}

struct Inner {
    machine: Mutex<VoiceMachine>,
    sinks: Mutex<Vec<VoiceSink>>,
    push_to_talk: CapabilityStatus,
}

impl std::fmt::Debug for VoiceControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VoiceControl")
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

impl VoiceControl {
    /// Starts idle with the microphone released. `push_to_talk` says whether it can be used.
    pub fn new(push_to_talk: CapabilityStatus) -> Self {
        Self {
            inner: Arc::new(Inner {
                machine: Mutex::new(VoiceMachine::new()),
                sinks: Mutex::new(Vec::new()),
                push_to_talk,
            }),
        }
    }

    fn state(&self) -> VoiceState {
        self.inner
            .machine
            .lock()
            .map(|machine| machine.state())
            // A poisoned lock fails closed: report muted, so nothing offers to capture.
            .unwrap_or(VoiceState::Muted {
                cleanup_pending: true,
            })
    }

    /// The current status.
    pub fn status(&self) -> VoiceStatus {
        status_for(self.state(), &self.inner.push_to_talk)
    }

    /// Registers a receiver and sends it the current status at once.
    pub fn subscribe(&self, sink: VoiceSink) -> VoiceStatus {
        let status = self.status();
        if sink(&status)
            && let Ok(mut sinks) = self.inner.sinks.lock()
        {
            sinks.push(sink);
        }
        status
    }

    /// Applies events in order, then notifies receivers once if anything changed.
    fn apply(&self, events: &[VoiceEvent]) -> Result<VoiceStatus, RuntimeError> {
        let before = self.state();
        let result = {
            let mut machine =
                self.inner.machine.lock().map_err(|_| {
                    RuntimeError::InvalidInput(String::from("voice state unavailable"))
                })?;
            events
                .iter()
                .try_for_each(|event| machine.apply(*event).map(|_| ()))
        };
        let status = self.status();
        if self.state() != before {
            self.notify(&status);
        }
        result
            .map(|()| status)
            .map_err(|error| RuntimeError::InvalidInput(error.to_string()))
    }

    fn notify(&self, status: &VoiceStatus) {
        let sinks = self
            .inner
            .sinks
            .lock()
            .map(|sinks| sinks.clone())
            .unwrap_or_default();
        // Call receivers outside the lock; then drop only the ones that are gone (receivers that
        // subscribed meanwhile stay).
        let gone: Vec<VoiceSink> = sinks.into_iter().filter(|sink| !sink(status)).collect();
        if !gone.is_empty()
            && let Ok(mut sinks) = self.inner.sinks.lock()
        {
            sinks.retain(|sink| !gone.iter().any(|dead| Arc::ptr_eq(dead, sink)));
        }
    }

    /// Push-to-talk pressed. Refused while muted, or while push-to-talk is unavailable.
    pub fn press_talk(&self) -> Result<VoiceStatus, RuntimeError> {
        if self.state().is_muted() {
            return Err(RuntimeError::InvalidInput(String::from(
                "Voice is muted. Unmute to talk.",
            )));
        }
        if let CapabilityStatus::PermissionRequired { reason }
        | CapabilityStatus::Denied { reason }
        | CapabilityStatus::Unavailable { reason } = &self.inner.push_to_talk
        {
            return Err(RuntimeError::InvalidInput(reason.clone()));
        }
        self.apply(&[VoiceEvent::PressTalk])
    }

    /// Push-to-talk released: recording ends and recognition starts.
    pub fn release_talk(&self) -> Result<VoiceStatus, RuntimeError> {
        if self.state() != VoiceState::Recording {
            return Ok(self.status());
        }
        self.apply(&[VoiceEvent::ReleaseTalk])
    }

    /// Mute or unmute. Muting is always accepted and cancels any session; unmuting never starts
    /// capture. Idempotent.
    pub fn set_muted(&self, muted: bool) -> VoiceStatus {
        let state = self.state();
        let result = match (muted, state.is_muted()) {
            (true, _) => self.apply(&[VoiceEvent::Mute]),
            (false, true) => self.apply(&[VoiceEvent::Unmute]),
            (false, false) => Ok(self.status()),
        };
        // There is no capture driver yet (push-to-talk is unavailable), so nothing can be left
        // to clean up. The P4 driver must instead send `CleanupComplete` after joining its work.
        self.finish_cleanup_without_driver();
        result.unwrap_or_else(|_| self.status())
    }

    /// Emergency stop / quit: cancels any voice session.
    pub fn cancel(&self) -> VoiceStatus {
        let _ = self.apply(&[VoiceEvent::Cancel]);
        self.finish_cleanup_without_driver();
        self.status()
    }

    /// With no capture driver there is no work to join, so a pending cleanup is complete.
    fn finish_cleanup_without_driver(&self) {
        if matches!(
            self.state(),
            VoiceState::Cancelled
                | VoiceState::Muted {
                    cleanup_pending: true
                }
        ) {
            let _ = self.apply(&[VoiceEvent::CleanupComplete]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_maps_to_a_distinct_phase_and_the_indicator_follows_capture() {
        let mut phases = std::collections::BTreeSet::new();
        for state in VoiceState::ALL {
            let status = status_for(state, &CapabilityStatus::Supported);
            assert_eq!(status.mic_open, state.requires_capture(), "{state:?}");
            assert_eq!(status.muted, state.is_muted(), "{state:?}");
            phases.insert(format!("{:?}", status.phase));
        }
        // Both muted variants share one phase; every other state has its own.
        assert_eq!(phases.len(), VoiceState::ALL.len() - 1);
    }
}
