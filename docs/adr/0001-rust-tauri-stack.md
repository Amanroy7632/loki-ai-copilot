# ADR-0001: Rust workspace + Tauri 2 + React/TypeScript

- **Status:** Accepted (taken from the blueprint; versions to be verified in P0)
- **Date:** 2026-11-27
- **Ticket:** setup

## Context

Loki has to run on macOS, Windows, Linux, Android and iOS, keep the inference loop and policy
correct and fast, use native OS capabilities, and stay maintainable by one developer helped by
coding agents.

## Decision

| Layer | Choice |
| --- | --- |
| Core | Rust stable, Cargo workspace, Tokio, serde, thiserror, tracing |
| UI | Tauri 2, React, TypeScript, Vite, pnpm |
| Inference | Ollama HTTP adapter (MVP); embedded llama.cpp later |
| Voice | CPAL capture (desktop), native audio on mobile, whisper.cpp bindings, native TTS, local VAD, optional wake word |
| Storage | SQLite via rusqlite, FTS5 first; OS secret stores for secrets |
| Networking | reqwest + rustls; typed search provider; Playwright sidecar optional (desktop only) |
| Native | Swift/Obj-C (Apple), Kotlin (Android), `windows` crate, zbus + portals (Linux) |
| Testing/CI | cargo test, clippy, rustfmt, Vitest, GitHub Actions native runners |

Go and Python are **not** added to the runtime unless a measured need justifies a separate service, and that needs its own ADR.

## Alternatives considered

| Option | Why not chosen |
| --- | --- |
| Electron | Larger footprint, and a weaker mobile story |
| Flutter | Weaker fit with Rust core libraries and the web UI ecosystem |
| Native app per platform | Too much duplicated work for a solo developer |
| Python core | Packaging, performance and distribution are harder; GIL and async complexity |

## Consequences

The project is shared across platforms, but native bridges are still needed for permissions,
audio and OS tools. Mobile support in Tauri 2 must be checked in P0, and P8/P9 may require more
native code than expected.
