//! Manual P4-T02 microphone check: aggregate counters only; no audio files or transcripts.
use loki_voice::capture::{CaptureConfig, CaptureSession, CpalSource};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("--check-device") {
        use cpal::traits::{DeviceTrait, HostTrait};
        let device = cpal::default_host()
            .default_input_device()
            .ok_or("no default input device")?;
        // Read-only diagnostics, no stream or device names. Keep backend detail out of audio
        // callbacks, but expose it here when troubleshooting native setup explicitly.
        println!(
            "Default input configuration: {:?}",
            device.default_input_config()
        );
        return Ok(());
    }
    let seconds: u64 = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "3".into())
        .parse()?;
    if !(1..=10).contains(&seconds) {
        return Err("duration must be 1..=10 seconds".into());
    }
    eprintln!("MICROPHONE CAPTURE: {seconds} seconds. Audio stays in memory and is discarded.");
    let source = CpalSource::default_input()?;
    eprintln!("Default input found; starting stream.");
    let mut session = CaptureSession::start(source, CaptureConfig::default())?;
    let format = session.format();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut count = 0;
    let mut sum_squares = 0.0f64;
    let mut peak = 0.0f32;
    let mut measure = |samples: &[f32]| {
        count += samples.len();
        for sample in samples {
            sum_squares += f64::from(*sample).powi(2);
            peak = peak.max(sample.abs());
        }
    };
    while Instant::now() < deadline {
        measure(&session.read()?);
        std::thread::sleep(Duration::from_millis(10));
    }
    let tail = session.finish()?;
    measure(&tail.samples);
    println!(
        "Microphone released. Native: {} Hz, {} channel(s). Output: 16000 Hz mono, {count} frames.",
        format.sample_rate, format.channels
    );
    println!(
        "Captured: {}, dropped: {}, peak: {peak:.6}, RMS: {:.6}. No audio saved.",
        tail.stats.captured_frames,
        tail.stats.dropped_frames,
        (sum_squares / count.max(1) as f64).sqrt()
    );
    if count == 0 {
        return Err(
            "no frames received; check microphone permission and device availability".into(),
        );
    }
    Ok(())
}
