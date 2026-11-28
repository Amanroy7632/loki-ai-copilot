//! Permissions with an OS side (P5 permissions redesign): turning a capability on asks the OS
//! first when needed, and the microphone test proves the microphone works end to end.
//!
//! Two independent switches must both be on before Loki uses the microphone: the user's grant in
//! Loki (revocable at any time) and the OS permission (macOS TCC, changed only in System
//! Settings). Loki never works around the OS.

use std::sync::Arc;
use std::time::{Duration, Instant};

use loki_core::{Capability, CapabilityStatus, MicTestResult};
use loki_voice::capture::{AudioSource, CaptureConfig, CaptureError, CaptureSession, CpalSource};

use super::*;

/// Receives the live input level (0..1) about ten times a second during a microphone test.
pub type LevelSink = Arc<dyn Fn(f32) + Send + Sync>;

/// Longest microphone test.
const MAX_TEST_SECONDS: u32 = 10;
/// Anything louder than this peak counts as "heard sound" (silence and idle noise stay below).
const HEARD_PEAK: f32 = 0.02;

fn parse_capability(capability: &str) -> Result<Capability, RuntimeError> {
    Capability::parse(capability)
        .ok_or_else(|| RuntimeError::InvalidInput(format!("unknown capability '{capability}'")))
}

fn mic_error(error: CaptureError) -> RuntimeError {
    RuntimeError::InvalidInput(format!("The microphone couldn't be used: {error}"))
}

impl Runtime {
    /// One capability's row.
    pub async fn permission(&self, capability: Capability) -> Result<PermissionRow, RuntimeError> {
        self.permissions()
            .await?
            .into_iter()
            .find(|row| row.capability == capability)
            .ok_or_else(|| RuntimeError::InvalidInput(String::from("unknown capability")))
    }

    /// Turns a capability on (scope `*`). If the OS permission was never asked, this shows the
    /// OS prompt first (macOS: "Loki would like to access the microphone"); Loki's grant is only
    /// given when the OS allows it. A refusal is returned as the row's `Denied` status, with
    /// nothing granted. Unavailable capabilities are refused.
    pub async fn enable_capability(&self, capability: &str) -> Result<PermissionRow, RuntimeError> {
        let capability = parse_capability(capability)?;
        let mut status = self
            .platform
            .capabilities()
            .get(&capability)
            .cloned()
            .unwrap_or(CapabilityStatus::Unavailable {
                reason: String::from("not reported by this platform"),
            });
        if let CapabilityStatus::Unavailable { reason } = &status {
            return Err(RuntimeError::InvalidInput(format!(
                "{} is unavailable here: {reason}",
                capability.as_str()
            )));
        }
        if matches!(status, CapabilityStatus::PermissionRequired { .. }) {
            let platform = Arc::clone(&self.platform);
            status = tokio::task::spawn_blocking(move || platform.request_permission(capability))
                .await
                .map_err(|error| RuntimeError::Tools(error.to_string()))?;
        }
        if status == CapabilityStatus::Supported {
            self.grant(capability.as_str(), loki_core::ANY_SCOPE)
                .await?;
        }
        self.permission(capability).await
    }

    /// Turns a capability off for Loki (scope `*`). The next tool call that needs it is denied.
    /// It doesn't change the OS permission; only the user can, in the OS settings.
    pub async fn disable_capability(
        &self,
        capability: &str,
    ) -> Result<PermissionRow, RuntimeError> {
        let capability = parse_capability(capability)?;
        self.revoke(capability.as_str(), loki_core::ANY_SCOPE)
            .await?;
        if capability == Capability::Microphone {
            // Turning the microphone off also ends any session that is using it.
            self.voice.cancel();
        }
        self.permission(capability).await
    }

    /// Opens the OS settings page for a capability's permission (fixed deep links only).
    pub async fn open_permission_settings(&self, capability: &str) -> Result<(), RuntimeError> {
        let capability = parse_capability(capability)?;
        let platform = Arc::clone(&self.platform);
        tokio::task::spawn_blocking(move || platform.open_permission_settings(capability))
            .await
            .map_err(|error| RuntimeError::Tools(error.to_string()))?
            .map_err(|error| RuntimeError::InvalidInput(error.to_string()))
    }

    /// Both switches are on: Loki's microphone grant and the OS permission.
    async fn microphone_ready(&self) -> Result<(), RuntimeError> {
        let granted = self
            .store
            .run(|db| db.active_grants())
            .await?
            .iter()
            .any(|grant| grant.capability == Capability::Microphone);
        if !granted {
            return Err(RuntimeError::InvalidInput(String::from(
                "The microphone is off in Loki's Permissions. Turn it on first.",
            )));
        }
        match self.platform.capabilities().get(&Capability::Microphone) {
            Some(CapabilityStatus::Supported) => Ok(()),
            Some(
                CapabilityStatus::PermissionRequired { reason }
                | CapabilityStatus::Denied { reason }
                | CapabilityStatus::Unavailable { reason },
            ) => Err(RuntimeError::InvalidInput(reason.clone())),
            None => Err(RuntimeError::InvalidInput(String::from(
                "this platform doesn't report a microphone",
            ))),
        }
    }

    /// Listens on the default microphone for `seconds` (1..=10), reporting the live level, and
    /// keeps no audio. The listening indicator is on for the whole test; stop, mute, quit or
    /// turning the microphone off end it early and release the device.
    pub async fn test_microphone(
        &self,
        seconds: u32,
        on_level: LevelSink,
    ) -> Result<MicTestResult, RuntimeError> {
        self.test_microphone_with(CpalSource::default_input, seconds, on_level)
            .await
    }

    /// [`Runtime::test_microphone`] with any audio source (tests use a fake one).
    pub async fn test_microphone_with<S, F>(
        &self,
        make_source: F,
        seconds: u32,
        on_level: LevelSink,
    ) -> Result<MicTestResult, RuntimeError>
    where
        S: AudioSource + 'static,
        F: FnOnce() -> Result<S, CaptureError> + Send + 'static,
    {
        self.microphone_ready().await?;
        let lease = self.voice.begin_capture()?;
        let duration = Duration::from_secs(u64::from(seconds.clamp(1, MAX_TEST_SECONDS)));
        tokio::task::spawn_blocking(move || {
            // Declared after `lease`, so the session (and the device) is released first.
            let source = make_source().map_err(mic_error)?;
            let mut session =
                CaptureSession::start(source, CaptureConfig::default()).map_err(mic_error)?;
            let started = Instant::now();
            let (mut peak, mut sum_squares, mut count) = (0.0_f32, 0.0_f64, 0_u64);
            let mut measure = |samples: &[f32]| {
                for sample in samples {
                    peak = peak.max(sample.abs());
                    sum_squares += f64::from(sample * sample);
                }
                count += samples.len() as u64;
            };
            while started.elapsed() < duration && !lease.should_stop() {
                std::thread::sleep(Duration::from_millis(100));
                let samples = session.read().map_err(mic_error)?;
                measure(&samples);
                if !samples.is_empty() {
                    let rms =
                        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
                    // Speech RMS is roughly 0.02-0.2; scale so normal speech fills the meter.
                    on_level((rms * 5.0).min(1.0));
                }
            }
            let stopped_early = lease.should_stop();
            let tail = session.finish().map_err(mic_error)?;
            measure(&tail.samples);
            on_level(0.0);
            let elapsed = started.elapsed();
            drop(lease); // the device is released: complete the voice session
            let level = if count == 0 {
                0.0
            } else {
                (sum_squares / count as f64).sqrt() as f32
            };
            Ok(MicTestResult {
                duration_ms: u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX),
                peak,
                level,
                heard_sound: peak >= HEARD_PEAK,
                stopped_early,
            })
        })
        .await
        .map_err(|error| RuntimeError::Tools(error.to_string()))?
    }
}
