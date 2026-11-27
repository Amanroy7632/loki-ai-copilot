# P1: Foundation (v0.1 CLI)

| | |
| --- | --- |
| **Effort** | 2–3 weeks |
| **Depends on** | P0 gate |
| **Release** | v0.1: headless CLI with mock chat |
| **Crates** | loki-core, loki-llm, loki-memory, loki-runtime, loki-cli |

## Goal

Build a solid, tested skeleton: typed contracts, a provider trait with a mock, SQLite persistence,
configuration, logging, a CLI harness and CI. After P1, every later phase only adds behaviour.

## Tickets

### P1-T01: Workspace skeleton verified
- **Do:** Confirm the drafted workspace (10 crates, lints, toolchain) compiles on all three desktop OSs in CI. Remove anything P0 showed to be wrong.
- **Acceptance:** CI is green on ubuntu, macos and windows runners.

### P1-T02: Finalize core contracts
- **Do:**
  - Review the drafted `loki-core` types against CONTRACTS.md.
  - Add `ConversationId`, `MessageId`, `Role` (system/user/assistant/tool) and `Message { id, conversation_id, role, content, created_at }`.
  - Decide the ID generation strategy (UUID v7 recommended). The generator lives in runtime; core stays dependency-light.
- **Acceptance:** Every type has docs and a serde round-trip test, unknown-field rejection is tested where applicable, and CONTRACTS.md matches the code.

### P1-T03: TypeScript contract generation
- **Do:**
  - Choose `ts-rs`, `specta` or `schemars` and write an ADR (D-02).
  - Generate the types into `packages/contracts/src/generated/`.
  - Add a CI step that regenerates them and fails on any diff.
- **Acceptance:** The generated TS compiles with `tsc --strict`, and CI detects stale contracts.

### P1-T04: Provider trait and mock provider (loki-llm)
- **Do:**
  - Define `ChatProvider` (see the CONTRACTS.md design note: an object-safe boxed stream) and `ChatEvent` (Token, ToolCall, Done, Error).
  - Add `LlmError` (thiserror), with cancellation via `tokio_util::sync::CancellationToken`.
  - Add `MockProvider`, which replays scripted events from fixtures and can be told to emit a tool call, an error or a slow stream.
- **Acceptance:** Tests cover streaming order, cancellation mid-stream (the stream ends promptly), and error propagation.

### P1-T05: SQLite and migrations (loki-memory)
- **Do:**
  - Use `rusqlite` with the `bundled` feature, and choose the migration approach (D-03, ADR).
  - Write `migrations/0001_init.sql` with conversations, messages and settings (DATA_MODEL.md).
  - Open with foreign keys, WAL and busy_timeout.
  - Add repositories: create/list conversations, append/list messages.
  - Run DB work off the async runtime.
- **Acceptance:** Tests use a temp directory and check that migrations apply to an empty DB and are idempotent on re-open, that the foreign key cascade works, and that parameterized queries are used. No `format!` of SQL values (grep check in the test).

### P1-T06: Configuration and paths (loki-runtime)
- **Do:**
  - Resolve per-user data, config and log dirs (the `directories` crate), with an override via `--data-dir` or the env var `LOKI_DATA_DIR`.
  - Define a versioned `Config { version, provider, model_id, allowed_roots, network_mode, retention, wake_word }`.
  - Write atomically (temp file, then rename), and add a migration hook `vN -> vN+1`.
- **Acceptance:** Tests cover the default config, round trip, a corrupted file (back it up, recreate the default, warn) and a version migration.

### P1-T07: Logging and redaction
- **Do:** Add `tracing` + `tracing-subscriber`, a rolling file appender in the logs dir, and the env filter `LOKI_LOG`. Add a `redact()` helper for tokens, emails and paths outside the allowed roots.
- **Acceptance:** Redaction unit tests pass, and logs appear in the data dir, not the CWD.

### P1-T08: CLI mock chat (loki-cli)
- **Do:**
  - Use `clap` for the commands `loki chat [--conversation <id>]`, `loki history list` and `loki history show <id>`, with the global `--data-dir`.
  - `chat` is a REPL over `MockProvider` that streams tokens. Ctrl+C stops generation, not the app; `/retry` and `/quit` are supported.
- **Acceptance:** A chat is persisted, and after restarting the CLI, `history show` prints it. An integration test drives this with a temp data dir.

### P1-T09: CI and supply chain
- **Do:**
  - Complete `.github/workflows/ci.yml`: fmt, clippy `-D warnings`, test on three OSs, caching.
  - Add `deny.toml` (cargo-deny: licenses, advisories, bans) and a `cargo deny check` job.
  - Create the workspace-level `tests/` crate if it's needed for E2E.
- **Acceptance:** CI is green, and an intentionally vulnerable or disallowed-license dependency would fail it (state how this was checked).

## Phase gate

- [ ] A mock chat persists and reloads across CLI restarts (P1-T08 test).
- [ ] `cargo test --workspace` passes in CI on all three desktop OSs.
- [ ] Contracts are generated, and the CI staleness check works.
- [ ] STATUS.md, CONTRACTS.md and DATA_MODEL.md match the code.

## Out of scope

Real models (P2), tools (P3), UI (P5).
