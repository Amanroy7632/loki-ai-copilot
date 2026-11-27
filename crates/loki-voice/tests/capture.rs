//! Synthetic inputs exercise the same capture/queue/resampling path without a microphone.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loki_voice::capture::{
    AudioSource, CaptureConfig, CaptureError, CaptureSession, CaptureSink, InputFormat,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Default)]
struct MockControl {
    sink: Arc<Mutex<Option<CaptureSink>>>,
    released: Arc<AtomicBool>,
}

impl MockControl {
    fn sink(&self) -> CaptureSink {
        self.sink.lock().unwrap().as_ref().unwrap().clone()
    }
    fn push(&self, samples: &[f32]) {
        self.sink().push_interleaved(samples);
    }
}

struct MockSource {
    format: InputFormat,
    control: MockControl,
    start_error: Option<CaptureError>,
}

struct MockStream(MockControl);
impl Drop for MockStream {
    fn drop(&mut self) {
        self.0.released.store(true, Ordering::Release);
    }
}

impl AudioSource for MockSource {
    type Stream = MockStream;
    fn format(&self) -> InputFormat {
        self.format
    }
    fn start(self, sink: CaptureSink) -> Result<MockStream, CaptureError> {
        if let Some(error) = self.start_error {
            return Err(error);
        }
        *self.control.sink.lock().unwrap() = Some(sink);
        Ok(MockStream(self.control))
    }
}

fn source(rate: u32, channels: u16) -> (MockSource, MockControl) {
    let control = MockControl::default();
    (
        MockSource {
            format: InputFormat {
                sample_rate: rate,
                channels,
            },
            control: control.clone(),
            start_error: None,
        },
        control,
    )
}

fn convert(rate: u32, input: &[f32], callback_size: usize) -> Vec<f32> {
    let (source, control) = source(rate, 1);
    let mut capture = CaptureSession::start(source, CaptureConfig::default()).unwrap();
    let mut samples = Vec::new();
    for chunk in input.chunks(callback_size) {
        control.push(chunk);
        samples.extend(capture.read().unwrap());
    }
    let tail = capture.finish().unwrap();
    samples.extend(tail.samples);
    assert_eq!(tail.stats.captured_frames, input.len() as u64);
    assert_eq!(tail.stats.dropped_frames, 0);
    assert!(control.released.load(Ordering::Acquire));
    samples
}

#[test]
fn mock_stereo_is_averaged_and_16khz_is_not_resampled() {
    let (source, control) = source(16_000, 2);
    let mut capture = CaptureSession::start(source, CaptureConfig::default()).unwrap();
    control.push(&[0.2, 0.6, -0.5, 0.5, -1.0, -1.0]);
    assert_eq!(capture.read().unwrap(), vec![0.4, 0.0, -1.0]);
    assert_eq!(capture.stats().captured_frames, 3);
    assert!(capture.finish().unwrap().samples.is_empty());
}

#[test]
fn mock_overflow_keeps_the_newest_frames_and_counts_exactly() {
    let (source, control) = source(16_000, 1);
    let mut capture = CaptureSession::start(
        source,
        CaptureConfig {
            buffer_ms: 10,
            ..CaptureConfig::default()
        },
    )
    .unwrap();
    let input: Vec<f32> = (0..200).map(|v| v as f32 / 200.0).collect();
    control.push(&input);
    assert_eq!(capture.stats().queued_frames, 160);
    assert_eq!(capture.stats().captured_frames, 200);
    assert_eq!(capture.stats().dropped_frames, 40);
    assert_eq!(capture.read().unwrap(), input[40..]);
    assert_eq!(capture.stats().queued_frames, 0);
}

#[test]
fn mock_signed_and_unsigned_pcm_have_the_correct_zero_and_scale() {
    let (source, control) = source(16_000, 1);
    let mut capture = CaptureSession::start(source, CaptureConfig::default()).unwrap();
    control.sink().push_interleaved(&[i16::MIN, 0, i16::MAX]);
    assert_eq!(capture.read().unwrap(), vec![-1.0, 0.0, 32767.0 / 32768.0]);
    control.sink().push_interleaved(&[0u16, 32768, u16::MAX]);
    assert_eq!(capture.read().unwrap(), vec![-1.0, 0.0, 32767.0 / 32768.0]);
}

#[test]
fn mock_invalid_frames_terminate_and_release_capture() {
    for input in [vec![0.0], vec![f32::NAN, 0.0], vec![0.0, f32::INFINITY]] {
        let (source, control) = source(16_000, 2);
        let mut capture = CaptureSession::start(source, CaptureConfig::default()).unwrap();
        control.push(&input);
        assert_eq!(capture.read(), Err(CaptureError::InvalidSamples));
        assert!(control.released.load(Ordering::Acquire));
        control.push(&[0.5, 0.5]);
        assert_eq!(capture.stats().captured_frames, 0);
    }
}

#[test]
fn mock_permission_denial_and_device_loss_stay_typed() {
    for error in [CaptureError::PermissionDenied, CaptureError::DeviceLost] {
        let (mut source, _) = source(48_000, 2);
        source.start_error = Some(error);
        assert!(
            matches!(CaptureSession::start(source, CaptureConfig::default()), Err(e) if e == error)
        );
        let (source, control) = self::source(48_000, 2);
        let mut capture = CaptureSession::start(source, CaptureConfig::default()).unwrap();
        control.sink().fail(error);
        control.sink().fail(CaptureError::Backend); // first failure wins
        assert_eq!(capture.read(), Err(error));
        assert!(control.released.load(Ordering::Acquire));
        control.push(&[0.5, 0.5]);
        assert_eq!(capture.stats().captured_frames, 0);
        assert!(matches!(capture.finish(), Err(e) if e == error));
    }
}

#[test]
fn mock_drop_releases_stream_without_explicit_stop() {
    let (source, control) = source(48_000, 1);
    let capture = CaptureSession::start(source, CaptureConfig::default()).unwrap();
    control.push(&[0.25; 10]);
    drop(capture);
    assert!(control.released.load(Ordering::Acquire));
    // A retained callback cannot revive capture or panic after the owner has gone.
    control.push(&[0.5; 100]);
}

#[test]
fn mock_invalid_configuration_is_rejected_before_source_starts() {
    for config in [
        CaptureConfig {
            buffer_ms: 0,
            ..CaptureConfig::default()
        },
        CaptureConfig {
            buffer_ms: 2001,
            ..CaptureConfig::default()
        },
        CaptureConfig {
            chunk_frames: 0,
            ..CaptureConfig::default()
        },
        CaptureConfig {
            chunk_frames: 8193,
            ..CaptureConfig::default()
        },
    ] {
        let (source, control) = source(48_000, 1);
        assert!(matches!(
            CaptureSession::start(source, config),
            Err(CaptureError::InvalidConfig)
        ));
        assert!(control.sink.lock().unwrap().is_none());
    }
    for (rate, channels) in [(0, 1), (48_000, 0), (48_000, 33), (44_101, 1)] {
        let (source, control) = source(rate, channels);
        assert!(matches!(
            CaptureSession::start(source, CaptureConfig::default()),
            Err(CaptureError::UnsupportedFormat)
        ));
        assert!(control.sink.lock().unwrap().is_none());
    }
}

#[test]
fn mock_resampling_preserves_duration_including_short_and_partial_tails() {
    for rate in [8_000, 16_000, 44_100, 48_000, 96_000] {
        for len in [0, 1, 127, 1024, 2049, rate as usize] {
            let input: Vec<f32> = (0..len).map(|i| (i as f32 * 0.13).sin() * 0.5).collect();
            let output = convert(rate, &input, 73);
            assert_eq!(
                output.len(),
                (len * 16_000).div_ceil(rate as usize),
                "rate={rate}, len={len}"
            );
            assert!(output.iter().all(|s| s.is_finite()));
        }
    }
}

#[test]
fn mock_resampling_is_independent_of_callback_boundaries() {
    let input: Vec<f32> = (0..12_345).map(|i| (i as f32 * 0.03).sin() * 0.5).collect();
    for rate in [44_100, 48_000] {
        assert_eq!(convert(rate, &input, 31), convert(rate, &input, 997));
    }
}

#[test]
fn mock_downsampling_preserves_speech_band_and_filters_aliasing() {
    let tone = |frequency: f32| -> Vec<f32> {
        (0..48_000)
            .map(|i| (std::f32::consts::TAU * frequency * i as f32 / 48_000.0).sin() * 0.5)
            .collect()
    };
    let low = convert(48_000, &tone(1000.0), 960);
    let high = convert(48_000, &tone(12_000.0), 960);
    let rms = |samples: &[f32]| {
        (samples[500..15_500].iter().map(|s| s * s).sum::<f32>() / 15_000.0).sqrt()
    };
    assert!((rms(&low) - 0.35355).abs() < 0.01);
    assert!(
        rms(&high) < 0.01,
        "out-of-band tone should be attenuated: {}",
        rms(&high)
    );
}
