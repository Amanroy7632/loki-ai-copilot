# migrations

This folder holds SQLite migrations, numbered `0001_init.sql`, `0002_...sql`. The first one is created in P1-T05.

- **Forward-only.** Never edit a migration that has been merged; add a new one.
- Each migration runs in a transaction.
- Every migration is tested on an empty DB and on a DB at the previous version.
- The target schema is in [docs/DATA_MODEL.md](../docs/DATA_MODEL.md).
