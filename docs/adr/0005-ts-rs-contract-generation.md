# ADR-0005: Generate TypeScript contracts with ts-rs

- **Status:** Accepted
- **Date:** 2026-11-27
- **Ticket:** P1-T03 (resolves D-02)
- **Deciders:** implementation agent

## Context

The desktop UI (P5) must use types generated from `loki-core`, never hand-written IPC types.
Candidates: `ts-rs`, `specta`, or JSON Schema via `schemars` plus a TS generator.
Requirements: honour serde attributes (`rename_all`, internally tagged enums, `transparent`,
`skip_serializing_if`), keep normal builds free of the generator, deterministic output for a CI
staleness check, permissive license.

## Decision

- **ts-rs 12** (MIT), behind an optional `ts` feature of `loki-core`. Each contract type has
  `#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]`.
- Features: `serde-json-impl` (maps `serde_json::Value` to `JsonValue`) and `no-serde-warnings`.
- 64-bit integers that are JSON numbers (`UnixMillis`, `duration_ms`) are exported as `number`
  via `#[ts(type = "number")]`, not ts-rs's default `bigint`, because that's what JSON.parse gives.
- `scripts/gen-contracts.sh` writes `packages/contracts/src/generated/` (one file per type plus a
  sorted `index.ts`). `--check` regenerates into a temp dir and `diff -r`s; CI and `check.sh` run it.
- `packages/contracts` (`@loki/contracts`) type-checks the output with `tsc --strict`
  (plus `exactOptionalPropertyTypes`, `noUncheckedIndexedAccess`). TypeScript pinned to 6.0.3.

## Alternatives considered

| Option | Pros | Cons | Why not chosen |
| --- | --- | --- | --- |
| specta (+ tauri-specta) | Also generates Tauri command bindings | Couples contracts to the Tauri command layer; larger surface; 2.x API churn | Commands come in P5; can be added then without changing these types |
| schemars + json-schema-to-typescript | Schemas reusable for tool input validation | Two toolchains (Rust + Node generator); output less idiomatic | More moving parts for P1 |

## Consequences

Product builds never compile ts-rs. Adding a contract type means adding the `cfg_attr` line and
regenerating. If P5 needs typed Tauri commands, revisit `specta` in a new ADR; the core types
stay the same. The staleness check is a POSIX script; on Windows run it from Git Bash.
