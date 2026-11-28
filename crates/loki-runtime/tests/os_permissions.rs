//! Permissions with an OS side: turning the microphone on asks the OS first and grants only if
//! the OS allows it; turning it off revokes Loki's grant and ends any session; the microphone
//! test needs both switches, shows the indicator while listening, and stops on mute.
//! `MockPlatform` plays the OS prompt; `MockMicSource` plays the microphone.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use loki_core::{Capability, CapabilityStatus, VoicePhase, VoiceStatus};
use loki_runtime::{MockPlatform, ProviderKind, Runtime, RuntimeOptions};
use loki_tools::platform::Platform;
use loki_voice::capture::{AudioSource, CaptureError, CaptureSink, InputFormat};

fn not_asked() -> CapabilityStatus {
    CapabilityStatus::PermissionRequired {
        reason: "macOS will ask".into(),
    }
}

fn runtime(dir: &tempfile::TempDir, platform: MockPlatform) -> (Runtime, Arc<MockPlatform>) {
    let platform = Arc::new(platform);
    let rt = Runtime::open(RuntimeOptions {
        data_dir: Some(dir.path().into()),
        provider: Some(ProviderKind::Mock),
        platform: Some(Arc::clone(&platform) as Arc<dyn Platform>),
        ..RuntimeOptions::default()
    })
    .unwrap();
    (rt, platform)
}

fn mic_platform(answer: CapabilityStatus) -> MockPlatform {
    MockPlatform::default()
        .with_capability(Capability::Microphone, not_asked())
        .with_permission_answer(Capability::Microphone, answer)
}

#[tokio::test(flavor = "multi_thread")]
async fn turning_the_microphone_on_asks_macos_first_and_grants_only_if_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let (rt, platform) = runtime(&dir, mic_platform(CapabilityStatus::Supported));
    let row = rt.enable_capability("microphone").await.unwrap();
    assert_eq!(platform.permission_requests(), [Capability::Microphone]);
    assert_eq!(row.status, CapabilityStatus::Supported);
    assert_eq!(row.scopes, ["*"]);

    // Already allowed: no second prompt.
    rt.enable_capability("microphone").await.unwrap();
    assert_eq!(platform.permission_requests().len(), 1);

    // Off: Loki's grant is revoked; the OS permission is the user's to change.
    let row = rt.disable_capability("microphone").await.unwrap();
    assert!(row.scopes.is_empty());
    assert_eq!(row.status, CapabilityStatus::Supported);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_in_the_os_prompt_grants_nothing_and_says_where_to_change_it() {
    let dir = tempfile::tempdir().unwrap();
    let denied = CapabilityStatus::Denied {
        reason: "Microphone access is off in System Settings".into(),
    };
    let (rt, platform) = runtime(&dir, mic_platform(denied.clone()));
    let row = rt.enable_capability("microphone").await.unwrap();
    assert_eq!(row.status, denied);
    assert!(row.scopes.is_empty(), "nothing granted");

    // Asking again doesn't re-prompt or work around the refusal.
    let row = rt.enable_capability("microphone").await.unwrap();
    assert!(row.scopes.is_empty());
    assert_eq!(platform.permission_requests().len(), 1);

    rt.open_permission_settings("microphone").await.unwrap();
    assert_eq!(platform.opened_settings(), [Capability::Microphone]);
}

#[tokio::test(flavor = "multi_thread")]
async fn plain_capabilities_switch_on_and_off_and_unavailable_ones_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (rt, platform) = runtime(&dir, MockPlatform::default());
    assert_eq!(rt.enable_capability("clock").await.unwrap().scopes, ["*"]);
    assert!(platform.permission_requests().is_empty(), "no OS prompt");
    assert!(
        rt.disable_capability("clock")
            .await
            .unwrap()
            .scopes
            .is_empty()
    );
    assert!(rt.enable_capability("network").await.is_err());
    assert!(rt.enable_capability("teleport").await.is_err());
}

/// A fake microphone: pushes a 440 Hz tone at 16 kHz until its stream is dropped.
struct MockMicSource {
    released: Arc<AtomicBool>,
}

struct MockMicStream {
    released: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Drop for MockMicStream {
    fn drop(&mut self) {
        self.released.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl AudioSource for MockMicSource {
    type Stream = MockMicStream;
    fn format(&self) -> InputFormat {
        InputFormat {
            sample_rate: 16_000,
            channels: 1,
        }
    }
    fn start(self, sink: CaptureSink) -> Result<MockMicStream, CaptureError> {
        let released = Arc::clone(&self.released);
        let worker = std::thread::spawn(move || {
            let mut t = 0_u32;
            while !released.load(Ordering::SeqCst) {
                let chunk: Vec<f32> = (0..160)
                    .map(|i| {
                        let n = (t + i) as f32;
                        0.3 * (n * 440.0 * std::f32::consts::TAU / 16_000.0).sin()
                    })
                    .collect();
                t += 160;
                sink.push_interleaved(&chunk);
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        Ok(MockMicStream {
            released: self.released,
            worker: Some(worker),
        })
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_microphone_test_needs_both_switches() {
    let dir = tempfile::tempdir().unwrap();
    let (rt, _) = runtime(&dir, mic_platform(CapabilityStatus::Supported));
    let released = Arc::new(AtomicBool::new(false));
    let source = {
        let released = Arc::clone(&released);
        move || Ok(MockMicSource { released })
    };
    let error = rt
        .test_microphone_with(source, 1, Arc::new(|_| {}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Turn it on first"), "{error}");
    assert!(!rt.voice().status().mic_open);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_microphone_test_listens_shows_the_indicator_and_releases_the_device() {
    let dir = tempfile::tempdir().unwrap();
    let (rt, _) = runtime(&dir, mic_platform(CapabilityStatus::Supported));
    rt.enable_capability("microphone").await.unwrap();
    let statuses = Arc::new(Mutex::new(Vec::<VoiceStatus>::new()));
    let store = Arc::clone(&statuses);
    rt.voice().subscribe(Arc::new(move |s: &VoiceStatus| {
        store.lock().unwrap().push(s.clone());
        true
    }));
    let levels = Arc::new(Mutex::new(Vec::new()));
    let level_store = Arc::clone(&levels);
    let released = Arc::new(AtomicBool::new(false));
    let source = {
        let released = Arc::clone(&released);
        move || Ok(MockMicSource { released })
    };
    let result = rt
        .test_microphone_with(
            source,
            1,
            Arc::new(move |level| level_store.lock().unwrap().push(level)),
        )
        .await
        .unwrap();

    assert!(result.heard_sound && result.peak > 0.25, "{result:?}");
    assert!(!result.stopped_early);
    assert!(result.duration_ms >= 900);
    assert!(
        levels.lock().unwrap().iter().any(|l| *l > 0.5),
        "the meter moved"
    );
    assert!(released.load(Ordering::SeqCst), "the device was released");
    let seen: Vec<_> = statuses
        .lock()
        .unwrap()
        .iter()
        .map(|s| (s.phase, s.mic_open))
        .collect();
    assert!(
        seen.contains(&(VoicePhase::Recording, true)),
        "indicator on: {seen:?}"
    );
    assert_eq!(
        seen.last(),
        Some(&(VoicePhase::Idle, false)),
        "and off at the end"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn mute_or_turning_the_microphone_off_stops_the_test_early() {
    let dir = tempfile::tempdir().unwrap();
    let (rt, _) = runtime(&dir, mic_platform(CapabilityStatus::Supported));
    let rt = Arc::new(rt);
    for stop_by_mute in [true, false] {
        rt.enable_capability("microphone").await.unwrap();
        let released = Arc::new(AtomicBool::new(false));
        let source = {
            let released = Arc::clone(&released);
            move || Ok(MockMicSource { released })
        };
        let running = Arc::clone(&rt);
        let test = tokio::spawn(async move {
            running
                .test_microphone_with(source, 10, Arc::new(|_| {}))
                .await
        });
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(rt.voice().status().mic_open);
        if stop_by_mute {
            rt.voice().set_muted(true);
        } else {
            rt.disable_capability("microphone").await.unwrap();
        }
        let result = tokio::time::timeout(Duration::from_secs(3), test)
            .await
            .expect("stops quickly")
            .unwrap()
            .unwrap();
        assert!(result.stopped_early);
        assert!(released.load(Ordering::SeqCst));
        let status = rt.voice().status();
        assert!(!status.mic_open);
        assert_eq!(status.muted, stop_by_mute);
        rt.voice().set_muted(false);
    }
}
