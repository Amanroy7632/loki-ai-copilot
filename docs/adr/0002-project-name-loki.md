# ADR-0002: Project name "Loki"

- **Status:** Accepted
- **Date:** 2026-11-27
- **Ticket:** setup

## Context

The source blueprint calls the product "NOVA AI". The owner chose the name **Loki**.

## Decision

- Product name: **Loki**. Crate prefix: `loki-`. Binary: `loki`. Env var prefix: `LOKI_`.
- The default wake phrase (if the wake word ships) is "Hey Loki".
- Crates are `publish = false`, so there is no clash on crates.io.

## Consequences

- "Loki" is also the name of a well-known log-aggregation project (Grafana Loki) and of
  fictional characters. Before any **public** release, P11 must include a trademark and
  name-conflict check, and settle the app identifier (e.g. `com.<owner>.loki`).
- Every reference to NOVA in the blueprint maps to Loki in these docs.
