# fixtures

Test data shared across crates:
- `models.json`: the allowlist of recommended models with license info (P2-T05)
- `mock_provider/`: scripted model responses for `MockProvider` (P1-T04). Format: `{"steps": [...]}`
  with step types `token`, `tool_call`, `delay`, `done`, `error`; see `crates/loki-llm/src/mock.rs`
- `files/`: fake documents for file-tool tests (P3-T08)
- `web/`: prompt-injection pages (P7-T06)
- `audio/`: synthetic or consented recordings (P4)

**Never** put real personal data, secrets or copyrighted model weights here.
