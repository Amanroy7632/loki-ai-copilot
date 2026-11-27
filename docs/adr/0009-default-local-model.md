# ADR-0009: Default local model qwen3:4b; bonsai-2:4b as the low-memory option

- **Status:** Superseded by ADR-0010
- **Date:** 2026-11-27
- **Ticket:** P0-T03 (feeds P2-T05)
- **Deciders:** implementation agent, from measured spike results; owner may override

## Context

VOICE_AND_MODELS says to start 8 GB machines on a quantized 1–3B instruct model and to recommend
only models with **verified** tool calling. The P0-T03 spike (spikes/ollama/README.md) measured two
Apache-2.0 candidates on the 8 GB reference dev Mac.

## Decision

- Default recommendation for 8 GB and up: **`qwen3:4b`** (Q4_K_M, about 3.2 GB loaded). It slightly
  exceeds the 1–3B guideline, but it measured 30 tok/s, a 0.08 s warm first token, and 20/20
  well-formed tool calls.
- Low-memory alternative: **`bonsai-2:4b`** (Q2_0 ternary, about 1.9 GB loaded, 21 tok/s, 20/20 tool calls).
- Requests use `think: false` and an explicit `num_ctx` (4096 default).
- `fixtures/models.json` is the allowlist; a model is recommended only if `tool_calls_verified`
  is true, which needs a recorded spike or benchmark run.

## Alternatives considered

| Option | Why not chosen |
| --- | --- |
| llama3.2:3b | Llama 3.2 Community License adds redistribution and naming conditions; not measured |
| qwen3:1.7b | Not measured yet; candidate for under-8 GB machines once verified |
| bonsai-2:4b as the default | About 30% slower and newer (less proven); kept as the low-memory choice |

## Consequences

Machines with less than 8 GB get no verified recommendation until a smaller model is measured.
Revisit when newer small models appear, or if qwen3's reasoning-in-content issue (seen once
after a tool result) proves common in the P3 tool loop.
