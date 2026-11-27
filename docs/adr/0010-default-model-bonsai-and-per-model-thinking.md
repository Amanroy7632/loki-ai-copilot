# ADR-0010: Default model bonsai-2:4b; thinking mode set per model

- **Status:** Accepted (supersedes ADR-0009)
- **Date:** 2026-11-27
- **Ticket:** P2-T05 (from the P2-T01/T07 manual run)
- **Deciders:** implementation agent, from measurements; owner may override

## Context

ADR-0009 picked `qwen3:4b` from the P0-T03 spike, which measured speed and tool calls but did not
inspect plain chat output. The first real chat run (P2) showed that on Ollama 0.40.18, `qwen3:4b`
with `think: false` writes its whole reasoning into the reply, ending with a stray `</think>`.
That text was shown and stored. A direct comparison (one-sentence question, num_ctx 4096, M2 8 GB):

| Model | think | Reply | Total time |
| --- | --- | --- | --- |
| qwen3:4b | false | 2,623 chars, reasoning leaked into `content` | 23 s |
| qwen3:4b | true | 148 chars, clean; reasoning separate in `thinking` | 13 s |
| bonsai-2:4b | false | 116 chars, clean | 2 s |
| bonsai-2:4b | true | 133 chars, clean, after 14k chars of thinking | 160 s |

## Decision

- Default recommendation: **`bonsai-2:4b`** with `think: false`: clean replies, about 2 s, about 1.9 GB in memory.
- `qwen3:4b` stays allowlisted as the alternative, always with `think: true`. Ollama then returns
  its reasoning separately and the adapter drops it, so the reply stays clean (slower first token).
- `fixtures/models.json` gets a per-model `think` field. The runtime reads it; unreviewed models
  run with `think: false`.
- An allowlist entry now requires a clean-reply check in addition to the tool-call measurement.

## Alternatives considered

| Option | Why not chosen |
| --- | --- |
| Keep qwen3:4b and strip text up to `</think>` | The leaked reasoning has no opening tag, so it can't be filtered while streaming without holding back every token; fragile |
| qwen3:4b with think on as the default | Correct, but about 10 s before any visible text on 8 GB |

## Consequences

The recommendation logic and its tests change (all RAM tiers of 6 GB and up -> bonsai-2:4b). If a
future Ollama fixes qwen3's `think: false`, re-measure and revisit. Output cleanliness becomes part
of every model review.
