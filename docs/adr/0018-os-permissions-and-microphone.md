# ADR-0018: Real OS permissions (two switches) and the first microphone driver

- **Status:** Accepted
- **Date:** 2026-11-28
- **Ticket:** P5 permissions redesign (owner request: "give all the necessary permissions, and let me turn them off")
- **Deciders:** implementation agent

## Context

The owner's microphone "didn't work": every adapter hard-coded the microphone as unavailable, so
it could never be turned on, and nothing asked macOS. The Permissions screen used buttons rather
than the switches people know from the OS settings app.

## Decision

- **Two switches, both required.** Loki's own grant (on/off in Privacy & Permissions, revocable at
  any time, effective on the next use) **and** the OS permission (macOS TCC). Loki never works
  around the OS: it reads the status, and only when the user turns a capability on does it ask
  macOS to show its own prompt. A refusal leaves Loki's switch off and offers "Open System
  Settings", a fixed deep link to the right privacy pane.
- **`Platform` gains two methods with safe defaults**: `request_permission(capability)` and
  `open_permission_settings(capability)`. Existing adapters keep working unchanged.
- **macOS microphone via `AVAudioApplication`** (`recordPermission`,
  `requestRecordPermissionWithCompletionHandler`; macOS 14+). The objc2 crates were already in the
  lock through cpal; they become direct, exactly pinned dependencies of `loki-platform` on macOS.
  The FFI lives in `macos_mic.rs`, with `unsafe` allowed locally and a SAFETY comment per block.
- **`Info.plist` with `NSMicrophoneUsageDescription`** (macOS ends an app that uses the microphone
  without one). Tauri embeds it in dev builds and merges it into bundles. In `tauri dev`, macOS
  attributes the request to the app that launched Loki.
- **Microphone test as the first real capture driver.** It listens for up to 10 s with a live level
  and keeps no audio. It holds a `CaptureLease` from `VoiceControl`, so the indicator shows
  "Microphone on" throughout. Stop, mute, quit or switching the microphone off ask it to stop,
  and the voice session completes its cleanup only **after** the device is released. This
  replaces the "no driver" shortcut from ADR-0017 whenever a driver is running.
- Other OSs report the microphone as unavailable, with a reason, until P6.

## Consequences

- Push-to-talk still needs speech-to-text (P4-T04). The microphone itself now works and can be
  proven with the test.
- Files and Folders has no queryable TCC status on macOS. The screen says macOS may ask on first
  access to Desktop, Documents or Downloads, and links to the settings page.
