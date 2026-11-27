# P2: Local chat (v0.2)

| | |
| --- | --- |
| **Effort** | 2–3 weeks |
| **Depends on** | P1 gate |
| **Release** | v0.2: real local model chat in the CLI |
| **Crates** | loki-llm, loki-agent, loki-runtime, loki-cli |

## Goal

Chat with a real local model through Ollama: streaming, cancellation, a context budget and model
onboarding. All of it works fully offline once the model is installed, and there is no path to
cloud inference.

## Tickets

### P2-T01: Ollama adapter
- **Do:**
  - Use `reqwest` with `rustls`.
  - Stream `/api/chat` as NDJSON, mapping it to `ChatEvent`s, including tool calls.
  - Base URL from config, default `http://127.0.0.1:11434`.
  - Health check via `/api/version` and `/api/tags` (the model is present).
  - Map errors to typed `LlmError` variants: not running, model missing, bad response.
- **Acceptance:** Integration tests run against a mock HTTP server (`wiremock`), covering streaming, a tool-call response, a malformed line and server down. One manual test against real Ollama is recorded.

### P2-T02: Cancellation
- **Do:** Cancelling the token aborts the HTTP request and ends the stream. Partial output is persisted and marked as stopped.
- **Acceptance:** The test asserts the stream ends within 200 ms of cancel against a slow mock server, and the partial message is saved with `stopped=true`. Retry works.

### P2-T03: Context budget
- **Do:**
  - Use a token estimator (chars/4 until a tokenizer ADR).
  - Build the context in this order: system prompt, then retrieved items (none yet), then the most recent turns that fit, always including the latest user turn.
  - Set the budget from the provider's context limit minus a reserve for the response.
- **Acceptance:** Unit tests cover the trimming order, a single message over budget (truncated with a marker), and an empty history.

### P2-T04: Agent orchestrator v1 (loki-agent)
- **Do:** Run a turn: input, context, provider stream, persist, emit events. Use traits only; the runtime injects the provider and repositories. There are no tools yet: tool-call events are logged and answered with "tools not available yet".
- **Acceptance:** Tests drive it with MockProvider and an in-memory or temp DB.

### P2-T05: Model onboarding
- **Do:**
  - Detect arch, RAM, free disk and GPU (`sysinfo`), and recommend a model per the VOICE_AND_MODELS table, choosing from an allowlist file (`fixtures/models.json`, reviewed in P0) with license info.
  - `loki models recommend` and `loki models pull <id>` show progress, cancel cleanly and show the license notice first.
  - Verify the digest. Ollama verifies its own layers; record that, and add SHA-256 for any direct download.
- **Acceptance:** Recommendation unit tests run across RAM tiers. A pull with cancel was demonstrated, and there is a clear error when Ollama isn't running.

### P2-T06: Network policy
- **Do:**
  - Add `NetworkMode { LocalOnly, ToolsAllowed }` in runtime, and one shared HTTP client factory that enforces it.
  - LocalOnly allows loopback only, plus an explicit, user-initiated model download.
  - Add a test or lint ensuring no other crate builds its own `reqwest::Client`.
- **Acceptance:** Tests check that LocalOnly blocks a request to an external host, allows `127.0.0.1`, and that a redirect from loopback to external is blocked.

### P2-T07: CLI wiring
- **Do:** `loki chat --provider ollama|mock`, plus `/stop`, `/retry` and `/model`. Report provider health at startup.
- **Acceptance:** A manual session is recorded in the handoff.

## Phase gate

- [ ] With networking disabled at the OS level, after the model is installed, a chat works end to end. Record how this was tested.
- [ ] There is no cloud provider anywhere in the code, and SEC-07 (local-only blocks non-loopback) passes.
- [ ] Cancel stops generation promptly, and partial output is handled.
- [ ] Benchmarks recorded in `docs/benchmarks/p2-inference.md`.

## Out of scope

Tools, voice and UI. Embedded llama.cpp, which needs a later ADR.
