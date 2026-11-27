# Project status

> **Agents:** read this first, work on the **Next ticket**, and update this file in the same
> commit as your work. Keep entries short and factual. Write "not run" or "failed" when that is
> what happened.

## Current position

**Parallel P4 worktree:** On 2026-11-27 the owner explicitly requested starting Phase 4 without
disturbing Phase 3. Branch `p4/t01-voice-state-machine`, worktree `../loki-p4-voice`, starts from
committed baseline `b0e8682`; the uncommitted P3 work in `../loki` is not included or changed.
This overrides the normal phase-order rule for this independent ticket, without passing any
earlier gates. The mainline status below is inherited from that baseline (including stale Git
blockers); it is not a current report on the separate P3 checkout.

### P4 branch progress

- [x] P4-T01 Voice state machine: pure lifecycle, typed events, exhaustive transition table
  (96 state/event pairs), mute invariant and sequence tests. `cargo test -p loki-voice --locked`
  → 6 passed, 0 failed. Full repository checks passed (168 tests), plus TypeScript checking.
- [ ] P4-T02 Audio capture: next ticket; CPAL/rubato version and license review, synthetic
  source tests, and a manual reference-OS capture are required.
- [ ] P4-T03..T09: not started; the P4 gate remains open.

| | |
| --- | --- |
| **Current phase** | P2: Local chat ([phase file](phases/P02-local-chat.md)), started on the owner's instruction while the P0 and P1 gates are still open (2026-11-27). |
| **Next ticket** | **P2 gate**: the offline test (needs the owner to turn networking off), `docs/benchmarks/p2-inference.md`, and the P2-T05 real pull-with-cancel demo. CI verification for P1-T01/T03/T09 is still blocked (no git or remote). |
| **Last gate passed** | none |
| **Reference OS** | **macOS** (Apple Silicon first), ADR-0003 |

## Phase progress

| Phase | State |
| --- | --- |
| P0 Feasibility | in progress (resumed by the owner after P1 was written). P0-T01 done; the spikes, ADR-0003 and the full license review are not. |
| P1 Foundation | code complete; **all checks pass locally on macOS**; CI not yet run anywhere. Gate not passed. |
| P2 Local chat | code complete; all checks pass locally (161 tests); manual chat against real Ollama recorded. Gate not passed (see Next ticket). |
| P3–P11 | not started |

## P0 tickets

- [x] P0-T01 Toolchain, versions, and a building scaffold (`docs/p0/environment.md`)
- [x] P0-T02 Reference OS decision: **macOS** (ADR-0003, owner decision)
- [x] P0-T03 Local inference spike (Ollama + tool calling): `spikes/ollama/`, `docs/p0/inference.md`, ADR-0009
- [ ] P0-T04 Audio and STT spike (CPAL + whisper.cpp)
- [ ] P0-T05 Tauri 2 shell and packaging spike
- [ ] P0-T06 Native permissions and app-discovery spike
- [ ] P0-T07 License review (partial: Rust crates in `docs/p0/licenses.md`)
- [ ] P0-T08 Verify the platform capability matrix
- [ ] P0-T09 Feasibility report and P1 plan adjustments

## P1 tickets

- [ ] P1-T01 Workspace skeleton verified: builds, fmt/clippy/test pass **locally on macOS**; toolchain pinned. *CI on ubuntu/macos/windows not run.*
- [x] P1-T02 Core contracts: `ConversationId`, `MessageId`, `Role`, `Message`, `Conversation`; round-trip + unknown-field tests for every type; UUID v7 (ADR-0004)
- [ ] P1-T03 TS contract generation (ADR-0005): generated, `tsc --strict` passes, staleness check proven locally. *"CI detects stale contracts" not observed in CI.*
- [x] P1-T04 `ChatProvider` + `MockProvider` (ADR-0006)
- [x] P1-T05 SQLite + migrations (ADR-0007)
- [x] P1-T06 Config and paths (ADR-0008)
- [x] P1-T07 Logging and redaction
- [x] P1-T08 CLI mock chat
- [ ] P1-T09 CI and supply chain: workflow + `deny.toml` written; `cargo deny check` passes locally and was shown to fail on a disallowed license and on a vulnerable crate. *CI not run.*

## P2 tickets

- [x] P2-T01 Ollama adapter (`loki-llm::ollama`): 12 wiremock tests (streaming, tool call, malformed line, missing `done`, error line, model missing, server down, health, pull) + manual run against Ollama 0.40.18. reqwest without TLS (all P2 traffic is loopback); rustls arrives with P7.
- [x] P2-T02 Cancellation: the stream ends < 200 ms after cancel mid-body (raw stalling-server test); a partial reply is stored with `stopped = true` (migration 0002); `/retry` replaces it
- [x] P2-T03 Context budget (`loki-agent::context`): trimming order, oversized message truncated with a marker, empty history
- [x] P2-T04 Orchestrator v1 over `ChatStore` + `IdSource` traits; tests with MockProvider and an in-memory store
- [ ] P2-T05 Model onboarding: `loki models recommend/list/pull/use`, allowlist with licenses (ADR-0010), RAM-tier tests, license prompt before pull. *Pull-with-cancel against a real download not demonstrated yet* (it downloads GBs; owner to confirm)
- [x] P2-T06 Network policy (`loki-runtime::net`): loopback IP literals only for local services, loopback-only redirects (SEC-07 test), no proxy, and a test that no other crate builds an HTTP client
- [x] P2-T07 CLI: `--provider ollama|mock`, `--model`, `/stop`, `/retry`, `/model`, health at startup; manual session recorded in the progress log

## Open decisions

| ID | Decision needed | Needed by | Notes |
| --- | --- | --- | --- |
| D-01 | **Reference OS**: the blueprint says macOS; the P1 work ran on macOS (Apple M2, 8 GB) | P0-T02 | Draft ADR-0003 lists the options. ADR-0003 says the setup machine was Windows 10; that no longer seems true |
| ~~D-02~~ | ~~How TS contracts are generated~~ | | **Decided:** ts-rs, ADR-0005 |
| ~~D-03~~ | ~~Migration library~~ | | **Decided:** hand-rolled `user_version` runner, ADR-0007 |
| D-04 | VAD engine and wake-word engine (licenses matter) | P4-T03 / P4-T08 | |
| D-05 | Project license | before any public release | No `license` field set yet |
| D-06 | Web search provider (self-hosted SearXNG, an API with a key, ...) | P7-T01 | The key is kept in the OS secret store |
| D-07 | **Accept MPL-2.0 `option-ext`** (pulled in by `directories`)? | before any release | Allowed by a crate-scoped exception in `deny.toml` for now. If rejected: replace `directories` (hand-rolled paths, or Tauri path APIs from P5) and delete the exception. See `docs/p0/licenses.md` |

## Blockers

- **The git repository is broken**: `.git/` has `HEAD` and `config` but no `objects/` or `refs/`
  (looks like empty directories were lost in a copy), so every `git` command fails with "not a
  git repository". Nothing has ever been committed. Repairing it with `git init` (safe: there is
  no history) was **declined by the owner on 2026-11-27**, so P1 work is **uncommitted** and the
  per-ticket commits AGENTS.md asks for were not made. Owner: repair or re-clone, then commit.
- **No GitHub remote / CI run**: P1-T01, P1-T03 and P1-T09 acceptance require CI results. Needs a
  pushed repository.

## Progress log

Newest first. Format: `YYYY-MM-DD | ticket | what changed | commands run → result`.

- 2026-11-27 | P4-T01 (isolated branch) | Added pure voice lifecycle, typed events, 96-pair transition matrix and mute/cancel/device-loss sequence tests. Owner authorized parallel P4 start without changing P3. | `cargo test -p loki-voice --locked` → 6 passed; `./scripts/check.sh` → passed outside sandbox (168 workspace tests, fmt, clippy, generated contracts, cargo deny); sandbox attempt could not bind mock-server ports. `tsc --noEmit -p packages/contracts/tsconfig.json` using the existing compiler in `../loki` → passed. See `docs/handoffs/p4-t01.md`.

- 2026-11-27 | P2-T01..T07 | Ollama adapter, cancellation with partial save (migration 0002 + pre-migration backup), context budget, orchestrator, model onboarding, network policy, CLI. Config v2 (Ollama URL, context sizes; v1→v2 migration keeps the provider). New installs default to `ollama`. **Default model changed to bonsai-2:4b (ADR-0010, supersedes ADR-0009)**: qwen3:4b leaked its reasoning into replies with `think: false`; thinking is now per model. | `./scripts/check.sh` → all passed, 161 tests, cargo deny ok. Manual: `loki models recommend` → bonsai-2:4b; `loki models use bonsai-2:4b`; `loki chat` → clean one-sentence answers (bonsai about 2 s), `/model qwen3:4b` switched mid-session with clean output; `history show` correct; 0 rows contain `</think>`.
- 2026-11-27 | P0-T03 | Rust spike `spikes/ollama` (reqwest, no proxy) against Ollama 0.40.18 with `OLLAMA_NO_CLOUD=1`. Models qwen3:4b and bonsai-2:4b (both Apache-2.0, verified with `ollama show --license`). Default model qwen3:4b (ADR-0009); allowlist `fixtures/models.json`. A first Python version was stopped on owner request (spikes are Rust). | `ollama-spike qwen3:4b` → 30 tok/s, 0.08 s warm first token, 3.2 GB, tools 20/20, false positives 0/10; `ollama-spike bonsai-2:4b` → 21 tok/s, 1.9 GB, tools 20/20; server log shows `stop: cancel task` on client disconnect.
- 2026-11-27 | P0-T01 | Installed Ollama 0.40.18 (`brew install ollama`; no service registered, not running). Checked the Tauri 2 macOS prerequisites against the official docs: all present (Xcode 26.6, CLT, Rust 1.97.1, Node 24 LTS). `docs/p0/environment.md` completed. | `cargo fmt --check` → ok; `cargo clippy --workspace --all-targets -- -D warnings` → 0 warnings; `cargo test --workspace` → 98 passed, 0 failed.
- 2026-11-27 | P1-T09 | `.github/workflows/ci.yml` (fmt, clippy incl. `ts` feature, test on 3 OSs with `--locked`, rust-cache; contracts job; cargo-deny job), `deny.toml`, `check.sh`/`check.ps1` extended. No `tests/` crate needed yet. | `cargo deny check` → `advisories ok, bans ok, licenses ok, sources ok`. Negative checks: removing the `option-ext` exception → `licenses FAILED` (MPL-2.0 rejected, exit 4); scratch crate with `smallvec = "=0.6.13"` + our `deny.toml` → `advisories FAILED` (RUSTSEC-2021-0003, exit 1). CI **not run**.
- 2026-11-27 | P1-T08 | `loki chat [--conversation]`, `history list`, `history show`, global `--data-dir`; REPL with `/retry`, `/quit`, `/help`; Ctrl+C cancels the reply only. Turn logic in `loki-agent::stream_reply`; `Runtime` API in `loki-runtime`. | `cargo test -p loki-cli` → 7 passed (5 consecutive runs), incl. persistence across restarts and a real SIGINT test (Unix only).
- 2026-11-27 | P1-T07 | `tracing` to daily-rolling files in `<data>/logs`, `LOKI_LOG` filter, `Redactor` applied to every log line (tokens, key=value secrets, JWT/sk-/ghp_/AKIA, long secrets, emails, paths outside allowed roots). | `cargo test -p loki-runtime` → redaction and log-location tests pass.
- 2026-11-27 | P1-T06 | `Paths` (flag > env > OS default), versioned `Config` (JSON, strict), atomic save, corrupt-file recovery, v0→v1 migration, newer-version refusal (ADR-0008). | `cargo test -p loki-runtime` → 29 passed.
- 2026-11-27 | P1-T05 | `migrations/0001_init.sql`, `Database` (FK/WAL/busy_timeout, migration runner), conversation/message repository, `MemoryStore` (spawn_blocking) (ADR-0007). | `cargo test -p loki-memory` → 13 passed, incl. cascade, idempotent re-open, `format!` grep.
- 2026-11-27 | P1-T04 | `ChatProvider`, `ChatEvent`, `LlmError`, `MockProvider` (scripted JSON + echo), fixtures in `fixtures/mock_provider/` (ADR-0006). | `cargo test -p loki-llm` → 11 passed, incl. mid-stream cancellation < 1 s.
- 2026-11-27 | P1-T03 | ts-rs behind `loki-core/ts`; `scripts/gen-contracts.sh [--check]`; `packages/contracts` with `tsc --strict` (ADR-0005). | `gen-contracts.sh --check` → up to date; doc-comment change → stale, exit 1; `pnpm --filter @loki/contracts typecheck` → exit 0.
- 2026-11-27 | P1-T02 | New IDs, `Role`, `Message`, `Conversation`; `deny_unknown_fields` on `Decision`/`CapabilityStatus`; round-trip tests for every type (ADR-0004). | `cargo test -p loki-core` → 34 passed.
- 2026-11-27 | P1-T01 | Rust pinned to 1.97.1 (`rust-version` 1.97), `packageManager: pnpm@10.15.1`, rustfmt fix in `task.rs`, `check.sh` made executable. Workspace dependencies declared once in the root `Cargo.toml`. | `./scripts/check.sh` → all checks passed on macOS 27 / M2. CI **not run**.
- 2026-11-27 | owner | Owner asked to start Phase 1 directly; P0 skipped (AGENTS.md says to finish P0 first; owner instruction wins). Owner declined the `git init` repair. | none
- 2026-11-27 | setup | Created the repository: docs (plan, architecture, contracts, security, data model, phases P0–P11, ADRs 0001–0003, templates), Cargo workspace with 10 crates (`loki-core` with contract types and unit tests; the other crates are documented stubs), CI workflow, and check scripts. | **No commands run.** Rust isn't installed, so `cargo check`/`cargo test` were **not run**. TOML files parsed with Python `tomllib`, and doc links were checked.
