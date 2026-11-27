# Requirements and scope

Requirement IDs are stable. Tickets, tests and ADRs refer to them (e.g. "implements FR-04").

## Functional requirements

| ID | Requirement | First delivered |
| --- | --- | --- |
| FR-01 | Text chat with streaming output, stop generation, retry, and local conversation history. | P1 (CLI, mock), P2 (real model), P5 (UI) |
| FR-02 | Local speech-to-text and speech synthesis. Push-to-talk first; an optional desktop wake word later. | P4 |
| FR-03 | Discover installed apps, launch allowed apps, search and read approved folders, report system metrics. | P3 (reference OS), P6 (other desktops) |
| FR-04 | Structured tool calling with previews, approvals, cancellation, timeouts and verified outcomes. | P3 |
| FR-05 | Web search and source-linked summaries. Network access is opt-in and separate from cloud inference. | P7 |
| FR-06 | Desktop tray or menu bar, close-to-background, a login-startup toggle, mute, and an explicit quit. | P5 |
| FR-07 | Scheduled reminders, persistent task status, and notifications where the OS allows them. | P7 |
| FR-08 | Mobile foreground voice and chat, share/import of files, and optional paired desktop actions. | P8, P9, P10 |
| FR-09 | Export and delete memory, clear history, revoke permissions, and work offline after the model download. | P5 (UI), P7 (memory) |

## Non-functional requirements

| ID | Requirement | How it is verified |
| --- | --- | --- |
| NFR-01 | **Local-only mode makes no network calls**, except explicit model downloads and user-authorized internet tools. | Network-policy unit tests, plus an integration test with a blocking HTTP client (P2-T06, P7) |
| NFR-02 | The UI stays responsive during inference. | Inference runs off the UI thread; manual check in P5-T10 |
| NFR-03 | Warm tool dispatch takes **under 250 ms**, excluding model and OS execution time. | Benchmark in `docs/benchmarks/` (P3, P11) |
| NFR-04 | Speech latency is **measured** on reference hardware, not promised as a fixed number. | Voice runbook (P4-T09) |
| NFR-05 | The CPU and battery cost of idle wake-word mode is benchmarked before it ships. | P4-T08, P11 |
| NFR-06 | No raw audio is retained by default. | Test that the temp and data dirs contain no audio after a session (P4-T07) |
| NFR-07 | Startup recovery never replays side effects. Incomplete tasks become `Interrupted`. | Runtime test (P3-T04) |
| NFR-08 | Each platform reports capabilities at runtime, and unsupported controls are never shown as working. | Capability tests per adapter (P3, P6, P8, P9) |

## Out of scope for v1

- Training a foundation model.
- Universal control of every application.
- Bypassing OS security (UAC, macOS TCC, Wayland restrictions, mobile sandboxing).
- Always-on listening on iOS.
- Autonomous purchases.
- **Arbitrary shell execution.**
- Unattended destructive changes.
- Public, multi-user cloud hosting.

Anything not listed in this document is out of scope until an ADR adds it.
