# ADR-0017: Voice UI ships before speech-to-text, with push-to-talk shown unavailable

- **Status:** Accepted
- **Date:** 2026-11-28
- **Ticket:** P5-T08
- **Deciders:** implementation agent (owner asked to proceed to the next ticket)

## Context

P5-T08 asks for a push-to-talk button and shortcut, an always-visible listening indicator, a
state display and mute. Its acceptance tests the indicator against the voice state machine. But
there is no speech-to-text yet (P4-T04 is open), so a working push-to-talk would open the
microphone and discard the audio. AGENTS.md forbids showing an unsupported feature as working.

## Decision

- The runtime owns the voice state (`VoiceControl` over `loki-voice`'s `VoiceMachine`) and pushes
  every change to front ends. The indicator shows whenever `mic_open`, which is
  `VoiceState::requires_capture()`; the UI never derives it.
- **Push-to-talk is shipped disabled**, with the reason ("needs local speech-to-text"), until
  P4-T04 provides recognition. The button, the shortcut (hold Ctrl+Shift+Space in the window) and
  the state display are in place, and pressing the shortcut explains why nothing happens.
- **Mute is real now**: muting forbids capture until an explicit unmute, and is reachable from
  the window and the tray. Emergency stop and quit cancel voice; a stop never unmutes.
- The UI test is tied to the state machine through `fixtures/voice-states.json`, which a Rust test
  generates from `VoiceState::ALL` and checks for staleness.
- Until a capture driver exists, cancelling or muting completes cleanup immediately, because
  there is no work to join. **The P4 driver must replace this** with a real `CleanupComplete`
  after it has released the device.

## Consequences

- A wake-word engine (Listening) and real capture plug into `VoiceControl` later, without UI
  changes.
- The shortcut works only while the Loki window is focused; a global shortcut would need another
  plugin and OS permissions. It can be revisited with P4-T06.
