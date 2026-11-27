# ADR-0007: Hand-rolled SQLite migrations tracked by `PRAGMA user_version`

- **Status:** Accepted
- **Date:** 2026-11-27
- **Ticket:** P1-T05 (resolves D-03)
- **Deciders:** implementation agent

## Context

`loki-memory` needs forward-only, transactional migrations from `/migrations/NNNN_*.sql`, tested
on an empty DB and on the previous version. Options were `rusqlite_migration`, `refinery`, or a
small runner on `PRAGMA user_version`.

## Decision

- A ~30-line runner in `loki-memory/src/db.rs`. Migrations are embedded with `include_str!` in a
  `const` list; migration *N* is list index *N-1*, and `SCHEMA_VERSION` is the list length.
- Each pending migration runs in a `BEGIN IMMEDIATE` transaction together with
  `PRAGMA user_version = N`, so a crash can't leave a half-applied version.
- A DB whose `user_version` is **newer** than the build is refused (`SchemaTooNew`) and not touched.
- Connections set `foreign_keys = ON`, `journal_mode = WAL` (verified), `busy_timeout = 5000`.

## Alternatives considered

| Option | Pros | Cons | Why not chosen |
| --- | --- | --- | --- |
| rusqlite_migration | Same `user_version` approach, maintained | Extra dependency whose rusqlite version must track ours | The logic is tiny; one fewer version-lock |
| refinery | Feature-rich, multiple DBs | Own history table, macros, heavier | More than we need |

## Consequences

No dependency to keep in lock-step with `rusqlite`. We own correctness: tests cover the empty DB,
idempotent re-open, newer-version refusal and data surviving re-open. Pre-migration backups
(DATA_MODEL "Recovery") are not implemented yet; add them before the first `0002` migration.
