# Roadmap

Do **one phase at a time**. A phase starts only after the previous phase's **gate** has passed
with recorded evidence. **No mobile work starts before the P5 gate passes.** Estimates assume one
developer working 10–12 hours a week. They are planning ranges, not commitments.

| Phase | Name | Effort | Release | Gate (summary) | Detail |
| --- | --- | --- | --- | --- | --- |
| P0 | Feasibility | 1–2 wk | n/a | Reproducible smoke tests for inference, audio, packaging and permissions; ADRs and the license review recorded | [P00](phases/P00-feasibility.md) |
| P1 | Foundation | 2–3 wk | v0.1 CLI | Mock chat persists and reloads; cargo tests pass in CI | [P01](phases/P01-foundation.md) |
| P2 | Local chat | 2–3 wk | v0.2 | Offline chat works after the model is installed; no hidden cloud fallback | [P02](phases/P02-local-chat.md) |
| P3 | Tools and policy | 3–4 wk | v0.3 | An allowed tool works; denied and malformed requests can't execute; audit recorded | [P03](phases/P03-tools-policy.md) |
| P4 | Voice | 3–4 wk | v0.4 | Voice round trip, mute and cancellation verified | [P04](phases/P04-voice.md) |
| P5 | Desktop app | 3–4 wk | v0.5 MVP | Close, reopen, quit, reboot and sleep tests pass on the reference OS | [P05](phases/P05-desktop-app.md) |
| P6 | Other desktops | 3–5 wk | v0.6 | The same core suite passes; unsupported tools are clearly disabled | [P06](phases/P06-other-desktops.md) |
| P7 | Web, memory, reminders | 2–4 wk | v0.7 | Offline mode blocks the network; injection tests pass; reminders recover after wake | [P07](phases/P07-web-memory-reminders.md) |
| P8 | Android | 3–5 wk | v0.8 | Foreground voice and lifecycle tests pass on a physical device | [P08](phases/P08-android.md) |
| P9 | iOS | 3–5 wk | v0.8 | Physical-device tests pass; no unsupported background claims | [P09](phases/P09-ios.md) |
| P10 | Pairing | 3–5 wk | v0.9 | Unauthorized and replayed requests fail; offline behaviour verified | [P10](phases/P10-pairing.md) |
| P11 | Hardening and release | 3–5 wk | v1.0 | Security and platform release checklist complete | [P11](phases/P11-hardening-release.md) |

- **Desktop MVP (P0–P5):** about 14–20 weeks.
- **Total (P0–P11):** about 31–49 weeks (roughly 310–588 hours). A polished general-purpose agent may take longer.

Prioritize desktop reliability over identical features on every platform.

## Release sequence

`v0.1 CLI → v0.2 local chat → v0.3 tools → v0.4 voice → v0.5 desktop MVP → v0.6 Windows/Linux (or the
other desktops) → v0.7 web/memory → v0.8 mobile → v0.9 pairing → v1.0 hardened release`

## Requirement coverage

| Requirement | Phases |
| --- | --- |
| FR-01 Chat | P1, P2, P5 |
| FR-02 Voice | P4, P5 |
| FR-03 Apps, files, system info | P3, P6 |
| FR-04 Tool calling and approvals | P3, P5 |
| FR-05 Web research | P7 |
| FR-06 Tray and background | P5 |
| FR-07 Reminders and notifications | P7 |
| FR-08 Mobile and pairing | P8, P9, P10 |
| FR-09 Data control | P5, P7 |
