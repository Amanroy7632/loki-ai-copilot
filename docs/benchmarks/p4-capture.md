# P4-T02: reference capture check

Date: 2026-11-27. Reference host: Apple Silicon macOS (project development machine).
CPAL 0.18.2, Rubato 5.1.0. No transcript or audio file is produced by this probe.

## Commands and observations

The initial unrestricted three-second run returned `Error: Backend` without samples. A read-only
device check then succeeded:

```text
cargo run -p loki-voice --example capture_probe --locked -- --check-device
Default input configuration: Ok(SupportedStreamConfig {
  channels: 1, sample_rate: 48000,
  buffer_size: Range { min: 15, max: 4096 }, sample_format: F32
})
```

A second explicit capture succeeded. Temporary backend-error diagnostics printed no errors
and were removed afterward; no automatic retry or permission bypass was added:

```text
cargo run -p loki-voice --example capture_probe --locked -- 3
MICROPHONE CAPTURE: 3 seconds. Audio stays in memory and is discarded.
Default input found; starting stream.
Microphone released. Native: 48000 Hz, 1 channel(s). Output: 16000 Hz mono, 47958 frames.
Captured: 143872, dropped: 0, peak: 0.064832, RMS: 0.012099. No audio saved.
```

143872 / 48000 = 2.9973 seconds of received native audio. The exact resampled length is
ceil(143872 × 16000 / 48000) = 47958. Nonzero levels demonstrate audio input; this does not
establish speech intelligibility or transcription accuracy. The stream guard was dropped before
the final counters printed. No OS microphone-indicator observation was made by the agent.

## Automated coverage and limits

Synthetic-source tests cover mono averaging, signed/unsigned sample conversion, bounded overflow
and exact drop counts, startup permission denial, device loss after startup, malformed and
non-finite input, resource release, callback-boundary independence, duration/partial tails, and
speech-band preservation with out-of-band attenuation. Native CPAL error-category mapping is
tested separately. Tests never open the microphone.

Actual OS permission denial, physical unplugging, sleep/resume, noise conditions and mute with
the full runtime remain P4-T07/T09 manual acceptance. The first backend error was not reproduced
or attributed to a specific cause. Other native platforms have not been exercised.
