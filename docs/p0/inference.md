# P0-T03: Local inference (Ollama)

Measured 2026-11-27 on a MacBook Air M2 with 8 GB, macOS 27.0.1, Ollama 0.40.18 (Metal).
Raw method, commands and the full table are in [spikes/ollama/README.md](../../spikes/ollama/README.md).

| | qwen3:4b | bonsai-2:4b |
| --- | --- | --- |
| License (`ollama show --license`) | Apache-2.0 | Apache-2.0 |
| Loaded memory (num_ctx 4096) | 3.2 GB | 1.7–1.9 GB |
| Cold load / cold first token | 2.6 s / 2.7 s | 3.4 s / 3.9 s |
| Warm first token (median) | 0.08 s | 0.11 s |
| Generation speed (median) | 30 tok/s | 21 tok/s |
| Well-formed tool calls (`time.now` / `time_now`) | 10/10 / 10/10 | 10/10 / 10/10 |
| Spurious tool calls (10 no-tool prompts) | 0 | 0 |
| Cancellation (connection drop) stops the server | yes (`stop: cancel task`) | yes |

**Acceptance (P0-T03):** streaming and tool calling demonstrated; numbers recorded; default model
named: **qwen3:4b, Apache-2.0** (ADR-0009), with bonsai-2:4b as the low-memory alternative.

**Plan changes:** see "Findings" in the spike README. In short: send dotted tool names unchanged;
cancel by dropping the connection; don't trust `think: false` to keep reasoning out of `content`;
require `OLLAMA_NO_CLOUD=1` and also refuse cloud/remote models in the adapter.

**Update (P2, ADR-0010):** the default is now **bonsai-2:4b**. qwen3:4b leaks reasoning into
replies with `think: false` on this Ollama version, so it is only used with `think: true`.
