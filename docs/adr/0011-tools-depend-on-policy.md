# ADR-0011: loki-tools depends on loki-policy (policy proof at compile time)

- **Status:** Accepted
- **Date:** 2026-11-27
- **Ticket:** P3-T04
- **Deciders:** owner (approved the new edge), implementation agent

## Context

P3-T04: "The executor's API requires proof of policy: an `Allowed` token type that only
`loki-policy` can construct … (a compile-time guarantee is preferred)." With the original graph
(`loki-tools -> core, platform`), tools can't name a type from `loki-policy`, so the guarantee could
only hold by convention.

## Decision

- Add the edge **`loki-tools -> loki-policy`**. `loki-policy` depends only on `loki-core`: no cycle.
- `loki_policy::Allowed` has private fields and no public constructor. Only `loki-policy`
  creates it: from an `Allow` decision, or from a consumed, matching, unexpired approval. It is
  bound to the request ID and `args_hash`, and it is not `Clone`.
- `Tool::execute` takes `&Allowed`. The executor checks that the permit matches the request
  before calling the tool. A direct call to a tool without a permit does not compile.
- `NetworkMode` moves from `loki-runtime` into `loki-core`, because `evaluate()` needs it and
  policy may only see core types.

## Alternatives considered

| Option | Why not chosen |
| --- | --- |
| Executor in `loki-agent`, no new edge | The executor would check, but `Tool::execute` would still be callable directly |
| Token type in `loki-core` | Core can't restrict construction to one downstream crate |

## Consequences

ARCHITECTURE.md's graph gains one edge. `loki-policy` must stay IO-free so `loki-tools` doesn't
inherit dependencies through it.
