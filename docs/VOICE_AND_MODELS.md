# Voice and local model engineering

## Voice state machine (`loki-voice`, P4-T01)

```text
Idle ─► Listening ─► Recording ─► Transcribing ─► Thinking ─► Speaking ─► Idle
Any active state ─► Cancelled ─► Idle
Any state ─► Muted ;  Muted ─► Idle only by explicit user action
```

- With push-to-talk, pressing starts `Recording` directly. `Listening` exists only when the wake word is enabled.
- There is only one voice session at a time.
- Recognition pauses during `Speaking`, to avoid self-triggering. Echo cancellation and barge-in come later, with an ADR.
- The listening indicator is visible in every state where the microphone is open.

### Implemented lifecycle (P4-T01)

`loki_voice::{VoiceMachine, VoiceState, VoiceEvent}` implements this diagram without IO or
additional dependencies. A new machine starts in `Idle`. `apply(event)` returns the resulting
state or `InvalidTransition`; rejected events leave the state unchanged.

| Current state | Event | Next state |
| --- | --- | --- |
| Idle | StartListening (future explicitly enabled wake-word driver only) | Listening |
| Idle / Listening | PressTalk | Recording |
| Recording | ReleaseTalk / SilenceTimeout | Transcribing |
| Transcribing | Transcribed | Thinking |
| Thinking | ResponseReady | Speaking |
| Speaking | SpeechDone | Idle |
| Listening / Recording / Transcribing / Thinking / Speaking | Cancel / DeviceLost | Cancelled |
| Cancelled | CleanupComplete | Idle |
| Any | Mute | Muted |
| Muted | Unmute (explicit user control only) | Idle |

Repeated `Mute` is harmless. `Cancel` and `DeviceLost` leave `Idle` or `Cancelled` unchanged.
All other combinations are rejected, including completion events received while muted and
push-to-talk while speaking. `requires_capture()` is true only for `Listening` and `Recording`;
it describes desired capture and indicator state, not actual OS device ownership.

`StartListening` and `CleanupComplete` make the diagram's listening and cancellation edges
explicit. Cleanup completion prevents a new session starting before cancelled work is joined.
Neither event implements audio capture or a wake-word engine. The future runtime driver must
serialize events, discard completions from old sessions, cancel/join all work on mute, and
release the device before allowing a fresh session. Unmute returns to idle without rearming
listening. Transcribed text is user input, never a source of control events or action approvals.

The transition table is tested across all 8 states × 12 events, with additional sequence tests
for push-to-talk, silence, cancellation, device loss, mute, and capture exclusion during speech.
Hardware capture, STT, TTS and SEC-11 integration remain later P4 tickets.

## Audio pipeline

1. Capture mono PCM (CPAL on desktop; AVAudioEngine or AudioRecord via native bridges on mobile).
2. Send frames over a bounded channel. If it overflows, drop the oldest frames and count them.
3. Resample to the STT input format: 16 kHz mono f32 for whisper.
4. Run voice activity detection with a silence timeout (the VAD engine is chosen by ADR in P4-T03).
5. Run speech-to-text with whisper.cpp bindings, on a blocking thread.
6. Text goes to the agent. The answer is spoken with the native TTS adapter.
7. **No raw audio is kept** unless the user turns on an explicit debug setting.

## Model onboarding (P2-T05)

Detect the CPU architecture, total and free RAM, free disk and GPU (if any), then recommend a model:

| Machine RAM | Starting point to **test** (not a guarantee) |
| --- | --- |
| 8 GB | 1–3B parameter instruct model, quantized |
| 16 GB | 3–8B |
| 32 GB | 7–14B |
| Mobile | Benchmark the smallest models; otherwise fall back to explicit desktop pairing |

- Only recommend models with **verified tool-call support** in the provider you are using.
- Downloads show progress, can be cancelled, verify a checksum, and show the license before starting.
- Record the actual measurements in `docs/benchmarks/`: first-token latency, tokens/s, peak RAM.

## Provider abstraction (`loki-llm`)

Order of implementation: **mock (P1)**, then **Ollama (P2)**, then **embedded llama.cpp (later, with an ADR)**.

Every provider supports:
- streaming tokens
- tool-call events
- cancellation
- context-limit reporting
- a health check

Prompt templates and tool-call parsing are specific to each provider, so they stay inside the adapter.

**There is no silent fallback to cloud inference.** If a cloud provider is ever added, it is a
separate, visibly labelled, opt-in provider, and that needs an ADR and a SECURITY_MODEL update.

## Voice acceptance tests (P4-T09)

| Check | How |
| --- | --- |
| Accuracy | 10 representative utterances, recorded in quiet and in noisy conditions. Report the word error rate. |
| Latency | Time from end of speech to the transcript, and from transcript to the first audio. |
| Mute | While muted, no frames are captured (check the OS mic indicator). |
| Permission denial | Shows a clear message and a way to fix it, and doesn't crash. |
| Device removal | Unplugging the mic mid-recording leads to `Cancelled`, then `Idle`, with a message. |
| Interruption | The emergency stop cancels transcription and TTS within 500 ms. |
| Sleep/resume | After the machine resumes, the state is `Idle` and the mic is released. |
| No retained audio | The data and temp dirs contain no audio files after a session. |
| Wake word (optional) | It is visibly active. Measure false activations per hour of background audio, and idle CPU. |

Fixture recordings for automated tests go in `fixtures/audio/`. They must be synthetic or
recorded with consent.
