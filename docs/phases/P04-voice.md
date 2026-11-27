# P4: Voice (v0.4)

| | |
| --- | --- |
| **Effort** | 3–4 weeks |
| **Depends on** | P3 gate |
| **Release** | v0.4: push-to-talk voice round trip in the CLI |
| **Crates** | loki-voice, loki-platform (TTS), loki-runtime, loki-cli |
| **Must read** | VOICE_AND_MODELS.md, SECURITY_MODEL.md (voice authorization) |

## Tickets

### P4-T01: Voice state machine
- **Do:** A pure state machine in `loki-voice`, following the diagram in VOICE_AND_MODELS.md, with typed events (PressTalk, ReleaseTalk, SilenceTimeout, Transcribed, ResponseReady, SpeechDone, Cancel, Mute, Unmute, DeviceLost).
- **Acceptance:** Exhaustive transition tests. `Muted → Idle` happens only on Unmute.

### P4-T02: Audio capture
- **Do:** CPAL default input, mono conversion, a bounded channel (drop the oldest and count), resampling to 16 kHz (`rubato`). Handle device removal and permission denial as typed errors.
- **Acceptance:** Tests use a synthetic source behind a trait. A manual capture on the reference OS is recorded.

### P4-T03: VAD and silence timeout
- **Do:** Choose the VAD engine (D-04, ADR, license-checked). Configure the silence timeout (default 1.2 s) and a maximum utterance length.
- **Acceptance:** Fixture-based tests: speech followed by silence ends the recording, and a noise-only fixture doesn't trigger it.

### P4-T04: Speech-to-text
- **Do:**
  - Use whisper.cpp bindings, with the model downloaded through onboarding (checksum, license).
  - Run it on a blocking thread, and make it cancellable between segments.
- **Acceptance:** A fixture WAV transcribes to the expected text (fuzzy match), and cancellation is honoured.

### P4-T05: Text-to-speech
- **Do:** A `Tts` trait with a native implementation on the reference OS, in `loki-platform`, and a mock. Support stop.
- **Acceptance:** Stop ends speech within 300 ms (measured manually).

### P4-T06: Push-to-talk in the CLI
- **Do:** `loki voice`: Enter starts and stops recording (or a hold key if the terminal supports it). Show the transcript, run the agent turn, speak the answer. Recognition is paused while speaking.
- **Acceptance:** A full round trip is demonstrated and recorded.

### P4-T07: Mute, emergency stop, no retained audio
- **Do:** Mute releases the device. The emergency stop cancels STT, the agent turn and TTS. Audio buffers live only in memory.
- **Acceptance:** Tests check that no audio files exist in the data or temp dirs after a session, and that muted means no capture stream.

### P4-T08: Wake-word spike (optional, feature-flagged)
- **Do:** Behind the cargo feature `wake-word`, off by default: evaluate one license-compatible engine.
- **Acceptance:** A report in `docs/benchmarks/p4-wake-word.md` with idle CPU %, false activations per hour on background audio, and the license. Decide in an ADR whether it ships.

### P4-T09: Voice acceptance runbook
- **Do:** Run the table in VOICE_AND_MODELS.md (10 utterances, quiet and noisy; mute; denial; device removal; interruption; sleep/resume).
- **Acceptance:** Results in `docs/benchmarks/p4-voice.md`.

## Phase gate

- [ ] Voice round trip works, and mute and cancellation are verified.
- [ ] No raw audio is retained (test).
- [ ] SEC-11: a transcribed "approve" doesn't approve a pending action.

## Out of scope

UI (P5), echo cancellation and barge-in (later ADR), mobile audio (P8/P9).
