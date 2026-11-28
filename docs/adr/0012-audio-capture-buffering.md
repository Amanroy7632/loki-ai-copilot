# ADR-0012: Bounded microphone capture and worker-side resampling

- **Status:** Accepted (implements the existing P4-T02 architecture)
- **Date:** 2026-11-27
- **Ticket:** P4-T02

## Context

Audio callbacks must not wait for consumers or do expensive resampling. P4-T02 requires CPAL
default input, downmixing, bounded buffering with oldest-frame replacement and counting, and
Rubato conversion to 16 kHz. The same path must be testable without microphone access.

## Decision

- `loki-voice::capture::AudioSource` supplies a fixed native format and an owning stream guard.
  CPAL implements it; integration tests inject `MockSource`. No internal crate edges change.
- Use CPAL 0.18.2 (Apache-2.0), Rubato 5.1.0 and crossbeam-queue 0.3.14 (both
  MIT OR Apache-2.0), exact-pinned in workspace dependencies. Versions/licenses verified via
  `cargo info` on 2026-11-27 and upstream package documentation:
  [CPAL](https://docs.rs/crate/cpal/0.18.2),
  [Rubato](https://docs.rs/rubato/5.1.0/rubato/),
  [crossbeam-queue](https://docs.rs/crossbeam-queue/0.3.14/crossbeam_queue/).
- A preallocated `ArrayQueue<f32>` holds mono frames. `force_push` replaces the oldest frame;
  an atomic counter records every replacement. Capacity defaults to 250 ms, configurable up
  to 2 seconds. Callback work performs no allocation, mutex locking, logging or disk IO.
  Lock-free operations avoid waiting for a slow consumer; this is not a hard realtime guarantee.
- CPAL integer/floating formats normalize to f32; channels are averaged, finite values clipped
  to [-1, 1], and malformed/non-finite input ends the session. Up to 32 channels and reviewed
  standard rates from 8–192 kHz are supported; other formats are reported unsupported.
- Rubato `Fft` with fixed input chunks runs on the consuming worker. The default chunk is 1024
  input frames. Sinc filtering prevents aliasing, initial converter delay is removed and stop
  flushes the tail to exactly ceil(input_frames × 16000 / input_rate) samples. 16 kHz bypasses
  the converter. Buffer overflows shorten the retained audio; no synthetic gap is inserted.
- A first-error latch preserves permission denial or device loss without allocating in the
  callback. Unknown backend errors fail closed. Route changes end the session instead of
  silently adopting a different device/format. Dropping the owning stream stops capture.
- `finish()` releases capture and returns the tail. Dropping a session cancels and discards
  queued audio. The source guard must stop callbacks on drop; the sink additionally closes
  before release. The future runtime owns session serialization and visible microphone state.

## Alternatives considered

- An unbounded channel risks retaining arbitrary audio and memory growth.
- A blocking channel stalls the hardware callback; drop-newest retains stale audio.
- Mutex plus `try_lock` may drop new samples on contention instead of the specified oldest ones.
- Resampling inside the callback increases deadline risk; per-callback independent resampling
  breaks continuity and repeats startup delay. Linear conversion lacks an anti-alias filter.

## Consequences

Only explicit capture opens a microphone; tests do not. No audio files, transcripts, downloads
or network calls are added. Linux builds need ALSA development headers (CI installs them).
CPAL isolates OS code; the shared capture path has no OS conditionals or unsafe code. Native
macOS validation must be recorded separately; a synthetic source cannot establish OS permission
behavior, actual device release or audible recording quality. VAD/STT/TTS/CLI wiring remain
later tickets. Consumers must drain frequently and inspect overflow/error diagnostics.
