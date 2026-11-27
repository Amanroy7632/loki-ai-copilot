# ADR-0004: UUID v7 text IDs, generated in loki-runtime

- **Status:** Accepted
- **Date:** 2026-11-27
- **Ticket:** P1-T02
- **Deciders:** implementation agent (recommended by P1-T02; owner may revisit)

## Context

Conversations, messages and (from P3) requests need globally unique IDs that work offline,
across devices (P10 pairing), and as SQLite `TEXT` primary keys. `loki-core` must stay
dependency-light, so it shouldn't pull in an ID library.

## Decision

- IDs are **UUID v7**, stored and serialized as lowercase hyphenated text (36 chars).
- Core keeps IDs as opaque string newtypes (`ConversationId`, `MessageId`, `RequestId`, ...).
- Generation lives in `loki-runtime::ids` (`uuid` crate, `v7` feature). Repositories take IDs as
  parameters; they never generate them.

## Alternatives considered

| Option | Pros | Cons | Why not chosen |
| --- | --- | --- | --- |
| UUID v4 | Ubiquitous | Random order fragments the PK index; no time locality | v7 has the same uniqueness plus time order |
| ULID | Sortable, compact | Extra crate, less standard than RFC 9562 | UUID v7 is standardized and in `uuid` |
| SQLite `INTEGER` rowid | Smallest | Not unique across devices; leaks counts | Breaks P10 sync/pairing |

## Consequences

IDs sort by creation time (millisecond precision), which helps debugging and index locality.
IDs embed a creation timestamp; that is acceptable because rows store `created_at` anyway.
Reversing would need a data migration for every table.
