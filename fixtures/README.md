# fixtures

Test data shared across crates:
- `models.json`: the allowlist of recommended models with license info (P2-T05)
- `mock_provider/`: scripted model responses for `MockProvider` (P1-T04). Format: `{"steps": [...]}`
  with step types `token`, `tool_call`, `delay`, `done`, `error`; see `crates/loki-llm/src/mock.rs`
- `files/`: fake documents for file-tool tests (P3-T08)
- `web/`: prompt-injection pages (P7-T06)
- `audio/`: synthetic or consented recordings (P4). The P4-T03 VAD signals are generated in Rust inside `crates/loki-voice/tests/vad.rs` (no binary files, no Python); `*.wav` is git-ignored, so recordings would need an explicit `.gitignore` exception
- `voice-states.json`: every voice state machine state with the status the UI receives (P5-T08); generated and checked by `cargo test -p loki-runtime --test voice` (regenerate with `LOKI_UPDATE_FIXTURES=1`), read by the desktop indicator test

**Never** put real personal data, secrets or copyrighted model weights here.
