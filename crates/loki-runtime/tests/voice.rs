//! P5-T08 voice control. Acceptance: the listening indicator is visible in every mic-open state,
//! tested against the state machine. `voice_indicator_fixture_matches_the_state_machine` writes
//! the full state table that the desktop's Vitest test checks the indicator against, so the UI
//! test can't drift from `loki-voice`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use loki_core::{CapabilityStatus, VoicePhase, VoiceStatus};
use loki_runtime::voice::{NO_SPEECH_TO_TEXT, VoiceControl, VoiceSink, status_for};
use loki_runtime::{ProviderKind, Runtime, RuntimeOptions};
use loki_voice::VoiceState;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/voice-states.json")
}

/// Every state machine state with the status the UI receives for it.
fn table() -> serde_json::Value {
    let rows: Vec<_> = VoiceState::ALL
        .into_iter()
        .map(|state| {
            serde_json::json!({
                "state": format!("{state:?}"),
                "status": status_for(state, &CapabilityStatus::Supported),
            })
        })
        .collect();
    serde_json::Value::Array(rows)
}

#[test]
fn voice_indicator_fixture_matches_the_state_machine() {
    let expected = serde_json::to_string_pretty(&table()).unwrap() + "\n";
    let path = fixture_path();
    if std::env::var_os("LOKI_UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &expected).unwrap();
    }
    let actual = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        actual, expected,
        "fixtures/voice-states.json is stale; regenerate with \
         LOKI_UPDATE_FIXTURES=1 cargo test -p loki-runtime --test voice"
    );
    // The table covers the mic-open states at all (Listening and Recording today).
    let open = VoiceState::ALL
        .into_iter()
        .filter(|state| status_for(*state, &CapabilityStatus::Supported).mic_open)
        .count();
    assert!(open >= 2);
}

/// Records every status a receiver gets.
fn recorder() -> (VoiceSink, Arc<Mutex<Vec<VoiceStatus>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let store = Arc::clone(&seen);
    (
        Arc::new(move |status: &VoiceStatus| {
            store.lock().unwrap().push(status.clone());
            true
        }),
        seen,
    )
}

#[test]
fn push_to_talk_is_refused_with_the_reason_until_speech_to_text_exists() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Runtime::open(RuntimeOptions {
        data_dir: Some(dir.path().into()),
        provider: Some(ProviderKind::Mock),
        ..RuntimeOptions::default()
    })
    .unwrap();
    let voice = rt.voice();
    let status = voice.status();
    assert_eq!(status.phase, VoicePhase::Idle);
    assert!(!status.mic_open);
    assert_eq!(
        status.push_to_talk,
        CapabilityStatus::Unavailable {
            reason: NO_SPEECH_TO_TEXT.into()
        }
    );
    let error = voice.press_talk().unwrap_err().to_string();
    assert!(error.contains("speech-to-text"), "{error}");
    assert!(!voice.status().mic_open, "the microphone stays off");
    assert_eq!(voice.release_talk().unwrap().phase, VoicePhase::Idle);
}

#[test]
fn mute_blocks_capture_and_unmute_never_starts_it() {
    let voice = VoiceControl::new(CapabilityStatus::Supported);
    let (sink, seen) = recorder();
    voice.subscribe(sink);
    let muted = voice.set_muted(true);
    assert!(muted.muted && !muted.mic_open);
    assert_eq!(muted.phase, VoicePhase::Muted);
    assert!(voice.press_talk().is_err(), "no capture while muted");
    assert!(voice.set_muted(true).muted, "idempotent");
    let unmuted = voice.set_muted(false);
    assert_eq!(unmuted.phase, VoicePhase::Idle);
    assert!(!unmuted.mic_open);
    // Initial status, muted, unmuted: one notification per change, none for no-ops.
    let phases: Vec<_> = seen.lock().unwrap().iter().map(|s| s.phase).collect();
    assert_eq!(
        phases,
        [VoicePhase::Idle, VoicePhase::Muted, VoicePhase::Idle]
    );
}

#[test]
fn the_indicator_is_on_exactly_while_recording_and_every_stop_turns_it_off() {
    let voice = VoiceControl::new(CapabilityStatus::Supported);
    let (sink, seen) = recorder();
    voice.subscribe(sink);

    assert!(voice.press_talk().unwrap().mic_open);
    let after = voice.release_talk().unwrap();
    assert_eq!(after.phase, VoicePhase::Transcribing);
    assert!(!after.mic_open, "capture ends when talk is released");

    // Emergency stop from any working state releases everything.
    assert_eq!(voice.cancel().phase, VoicePhase::Idle);
    assert!(voice.press_talk().unwrap().mic_open);
    let muted = voice.set_muted(true);
    assert!(
        muted.muted && !muted.mic_open,
        "mute turns the microphone off at once"
    );
    assert_eq!(
        voice.cancel().phase,
        VoicePhase::Muted,
        "stop never unmutes"
    );

    // Every published status kept the indicator consistent with the phase.
    for status in seen.lock().unwrap().iter() {
        assert_eq!(
            status.mic_open,
            matches!(status.phase, VoicePhase::Recording | VoicePhase::Listening),
            "{status:?}"
        );
    }
}

#[test]
fn receivers_that_are_gone_are_dropped() {
    let voice = VoiceControl::new(CapabilityStatus::Supported);
    let calls = Arc::new(Mutex::new(0));
    let counter = Arc::clone(&calls);
    voice.subscribe(Arc::new(move |_| {
        *counter.lock().unwrap() += 1;
        false // e.g. the window's channel closed
    }));
    voice.set_muted(true);
    voice.set_muted(false);
    assert_eq!(*calls.lock().unwrap(), 1, "only the initial status");
}

#[tokio::test]
async fn emergency_stop_and_quit_also_stop_voice() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Runtime::open(RuntimeOptions {
        data_dir: Some(dir.path().into()),
        provider: Some(ProviderKind::Mock),
        ..RuntimeOptions::default()
    })
    .unwrap();
    let control = rt.lifecycle_control();
    let voice = rt.voice();
    voice.set_muted(true);
    control.emergency_stop();
    assert!(voice.status().muted, "emergency stop keeps mute");
    voice.set_muted(false);
    control.begin_shutdown();
    let status = voice.status();
    assert_eq!(status.phase, VoicePhase::Idle);
    assert!(!status.mic_open);
}
