# P9: iOS / iPadOS (v0.8)

| | |
| --- | --- |
| **Effort** | 3–5 weeks |
| **Depends on** | P5 gate (hard rule) |
| **Needs** | a Mac with Xcode, an Apple Developer account and a physical device |

## Tickets

### P9-T01: Tauri iOS target
- `pnpm tauri ios init`, and the shared UI with iOS layouts.

### P9-T02: Native audio
- A Swift bridge for AVAudioSession and AVAudioEngine: interruptions, route changes and the permission flow. Foreground only.

### P9-T03: App Intents, Shortcuts, share extension
- Expose safe intents ("Ask Loki", "Add reminder"). The share extension imports text and files, and the document picker handles file access.

### P9-T04: Local model spike
- The same as P8-T05. Otherwise fall back to explicit desktop pairing.

### P9-T05: Lifecycle and background honesty
- No background daemon, no persistent wake word and no control of other apps. The UI and docs must not claim them.

## Phase gate

- [ ] Physical-device tests pass (recorded).
- [ ] No unsupported background claims anywhere in the UI or docs.
