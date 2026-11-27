# P8: Android (v0.8)

| | |
| --- | --- |
| **Effort** | 3–5 weeks |
| **Depends on** | P5 gate (hard rule), and P7 recommended |
| **Needs** | a physical Android device. Emulators aren't enough for the gate. |

## Tickets

### P8-T01: Tauri Android target
- `pnpm tauri android init`, shared UI with mobile layouts, and a build on CI (Linux runner) and locally.

### P8-T02: Native microphone and voice
- A Kotlin bridge for AudioRecord and the runtime mic permission. Foreground voice only. A user-started foreground service is allowed only if Play policy permits it for this use case; record that review in an ADR.

### P8-T03: Intents, share target, storage
- Receive shared text and files (share target), and read files through the Storage Access Framework only.

### P8-T04: Notifications and reminders
- Use OS scheduling APIs (WorkManager/AlarmManager per policy). Don't assume a continuously running process.

### P8-T05: Local model spike
- Benchmark small models on the device: RAM, thermal behaviour, tokens/s. If they aren't viable, the app relies on desktop pairing (P10) and says so clearly.

### P8-T06: Lifecycle tests
- Backgrounding, process death, rotation, permission revoked while running, and Doze.

## Phase gate

- [ ] Foreground voice and lifecycle tests pass on a physical device (recorded).
- [ ] No unsupported capability is shown as working.
