# ADR-0006: ChatProvider returns a boxed stream; errors are `Err` items

- **Status:** Accepted
- **Date:** 2026-11-27
- **Ticket:** P1-T04
- **Deciders:** implementation agent

## Context

CONTRACTS.md asked P1-T04 to choose how `ChatProvider` streams. The runtime selects a provider
from config, so the trait must be object-safe (`Arc<dyn ChatProvider>`). The phase file lists
`ChatEvent = Token | ToolCall | Done | Error`, while the CONTRACTS design note proposed
`BoxStream<Result<ChatEvent, LlmError>>`. Those two overlap: errors would have two channels.

## Decision

- `fn stream_chat(&self, ChatRequest, CancellationToken) -> BoxStream<'static, Result<ChatEvent, LlmError>>`
  (a plain method, no `async-trait`). `fn health(&self) -> BoxFuture<'_, ProviderHealth>`.
  `fn context_limit(&self) -> u32`. `fn name(&self) -> &str`.
- `ChatEvent = Token { text } | ToolCall(ToolCallDraft) | Done(Usage)`. **Errors travel only as
  `Err(LlmError)`**; there is no `Error` variant.
- Stream protocol: content items, then exactly one terminal item (`Ok(Done)` or `Err`), then end.
  Cancellation ends the stream promptly with `Err(LlmError::Cancelled)`.
- `ToolCallDraft { tool, arguments }` carries no request ID or deadline; the agent assigns those.
- Cancellation uses `tokio_util::sync::CancellationToken` (re-exported from `loki-llm`).

## Alternatives considered

| Option | Pros | Cons | Why not chosen |
| --- | --- | --- | --- |
| `ChatEvent::Error` variant, infallible stream | Matches the phase-file wording | Two error paths; `?` doesn't work on items | Ambiguous for consumers |
| `async-trait` / native `async fn` in traits | Familiar | Native AFIT isn't object-safe; async-trait adds a dep and still needs a stream for tokens | Boxed stream already solves it |
| Channel (`mpsc::Receiver`) return | Simple | Forces a spawned task per request; cancellation by drop is less explicit | Streams compose better |

## Consequences

One allocation per request for the box (negligible next to inference). The agent's
`stream_reply` enforces the protocol and turns a stream ending without a terminal item into
`LlmError::Protocol`. CONTRACTS.md is updated to this shape.
