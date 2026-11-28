# ADR-0013: Voice activity detection with earshot (VAD part of D-04)

- **Status:** Accepted for P4. Re-evaluate in P4-T09 on real recordings.
- **Date:** 2026-11-28
- **Ticket:** P4-T03 (resolves the VAD half of D-04; the wake-word half stays open for P4-T08)
- **Deciders:** implementation agents (P4 session + review)

## Context

P4-T03 needs a local VAD with a silence timeout (default 1.2 s) and a maximum utterance length,
license-checked, with fixture tests: speech followed by silence ends recording; noise alone doesn't.
Input is the 16 kHz mono stream from P4-T02. No network, no model download at runtime.

## Decision

- **earshot 1.2.2** (MIT OR Apache-2.0, pure Rust, embedded predictor, no native runtime, no
  download), behind a `SpeechDetector` trait so a different engine can replace it.
- `UtteranceDetector` frames audio into 256-sample (16 ms) windows regardless of capture chunk
  sizes. Timing counts samples, not wall-clock. Defaults: threshold 0.5, **speech confirmation
  400 ms** of consecutive voiced frames, silence timeout 1.2 s, maximum utterance 30 s (cap 5 min).
  Silence before confirmed speech never ends a recording; the cap ends it as `NoSpeech` or
  `MaxDuration`. Invalid input or scores fail closed until reset.

### Why 400 ms, not 80 ms (measured 2026-11-28, synthetic signals in `tests/vad.rs`)

| Signal | Longest run of frames scoring >= 0.5 |
| --- | --- |
| Synthetic vowels (/a/ /i/ /u/ /a/) | 1376 ms |
| White noise from stream start, 0.004–0.24 peak | up to 240 ms (an onset transient that decays to ~0.25) |
| Same noise after 0.2–3 s of digital silence, peak 0.08 | up to 352 ms |
| Same noise after digital silence, peak 0.02–0.04 | **the whole 4 s** (see limitation 1) |

Speech over a continuous noise bed (peak 0.005–0.04) still ended 1.2 s after the speech with the
400 ms window. With 80 ms, noise alone at peak 0.02–0.04 was falsely confirmed as speech;
with 240 ms, at 0.04. With 400 ms, none of the continuous-noise cases were.

## Known limitations

1. **Quiet broadband noise right after exact digital zeros** is scored as speech for seconds.
   Real microphones never deliver exact zeros (they always have a noise floor), so this is
   unlikely in normal capture. But macOS delivers zeros when microphone access is denied, and a
   denied microphone must be detected before capture (P4-T07 / platform permission status),
   not by the VAD.
2. A 400 ms continuous-voicing requirement may not confirm a very short reply ("yes"). In
   push-to-talk that only means the recording ends on key release instead of on silence; in a
   future hands-free mode it would run to the maximum length. Tune on real speech in P4-T09.
3. Only synthetic signals have been measured. The signals are not intelligible speech.

## Alternatives considered

| Option | Why not chosen now |
| --- | --- |
| Silero VAD (ONNX, MIT) | Needs an ONNX runtime (`ort`, native library) and a model file; heavier to ship and verify. First candidate if P4-T09 shows earshot is not good enough |
| WebRTC VAD (BSD) via FFI | C build and `unsafe` FFI; energy/GMM based, weaker on noise |
| Energy threshold only | Fails on any steady background noise |

## Consequences

`loki-voice` gains one pure-Rust dependency. Test audio is generated in Rust at test time: no
recorded or binary fixtures, nothing to keep out of git. Reverse by implementing
`SpeechDetector` for another engine; the timing logic and tests stay.
