# ADR-0008: Config as versioned JSON; one override root for all paths

- **Status:** Accepted
- **Date:** 2026-11-27
- **Ticket:** P1-T06
- **Deciders:** implementation agent

## Context

P1-T06 needs per-user data/config/log dirs with a `--data-dir` / `LOKI_DATA_DIR` override, a
versioned `Config`, atomic writes, recovery from corruption, and `vN -> vN+1` migrations.

## Decision

- **Format:** JSON (`config.json`), via the existing `serde_json` dependency. The UI (P5) will be
  the normal editor, so TOML's comment support isn't worth a new dependency (and a rewrite would
  drop comments anyway).
- **Strict:** `deny_unknown_fields`. An unparseable or unknown-field file is moved to
  `config.json.corrupt-<unix_ms>`, defaults are written, and a warning is shown.
- **Versioning:** `version` field. A file without one is version 0. Migration steps operate on the
  JSON object; the pre-migration file is kept as `config.json.v<N>.bak`. A newer version is
  refused and left untouched.
- **Atomic write:** temp file in the same dir, `sync_all`, `rename`.
- **Paths:** precedence `--data-dir` > `LOKI_DATA_DIR` > `directories::ProjectDirs("", "", "Loki")`.
  With an override, everything lives under one root (`<root>`, `<root>/config`, `<root>/logs`).
  Default logs go under the data-local dir (`.../logs`).
- **Defaults:** provider `mock` (the only P1 provider; the CLI labels its replies as mock),
  `network_mode = local_only`, wake word off, no allowed roots, keep history.

## Alternatives considered

| Option | Pros | Cons | Why not chosen |
| --- | --- | --- | --- |
| TOML | Human-friendly, comments | New dep; comments lost on rewrite | UI is the editor |
| Lenient parsing (ignore unknown keys) | Typos don't reset config | A typo silently has no effect, e.g. on a security setting | Fail visibly instead |
| Settings table in SQLite only | One store | Config must load before the DB opens (it chooses the data dir) | Ordering |

## Consequences

The `ProjectDirs` qualifier/organization are empty until the app identifier is decided (ADR-0002,
P11); changing them later moves the default data dir and needs a migration step. P2 will change
the default provider to `ollama`; existing files keep their explicit choice.
