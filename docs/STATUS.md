# Project status

> **Agents:** read this first, work on the **Next ticket**, and update this file in the same
> commit as your work. Keep entries short and factual. Write "not run" or "failed" when that is
> what happened.

## Current position

| | |
| --- | --- |
| **Current phase** | P5: Desktop ([phase file](phases/P05-desktop-app.md)), with P4 voice work open in parallel. Owner asked to continue from Claude's handoff (2026-11-28). Earlier phase gates remain open. |
| **Next ticket** | **P5-T09 Security configuration** (P5-T07 still needs the owner's run of `docs/checklists/desktop-lifecycle.md`). P5-T06 memory/FTS acceptance depends on P7; P4-T04..T09 remain open. Earlier CI/manual gates and D-05/D-07 remain unresolved. |
| **Last gate passed** | none |
| **Reference OS** | **macOS** (Apple Silicon first), ADR-0003 |

## Phase progress

| Phase | State |
| --- | --- |
| P0 Feasibility | in progress (resumed by the owner after P1 was written). P0-T01 done; the spikes, ADR-0003 and the full license review are not. |
| P1 Foundation | code complete; **all checks pass locally on macOS**; CI not yet run anywhere. Gate not passed. |
| P2 Local chat | code complete; all checks pass locally (161 tests); manual chat against real Ollama recorded. Gate not passed (see Next ticket). |
| P3 Tools and policy | code complete; all checks pass locally (238 tests); native macOS smoke test passed (real model, approved app launch). Gate not passed: SEC tests not yet run in CI. |
| P4 Voice | P4-T01..T03 implemented: state machine, explicit microphone capture, VAD and silence timeout (33 voice tests). No STT/TTS integration yet. Gate not passed. |
| P5 Desktop | P5-T01..T05 implemented; P5-T06 settings/history controls implemented (memory/FTS waits for P7); P5-T07 tray/lifecycle implemented, checklist pending; P5-T08 voice UI done (push-to-talk waits for speech-to-text). Native interaction and phase gate remain open. |
| P6–P11 | not started |

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

## P3 tickets

- [x] P3-T01 Tool trait and registry: name rule, duplicate check, strict-schema check (closed objects, bounded strings/arrays/numbers, no `$ref`), `jsonschema` validation (no remote resolution)
- [x] P3-T02 Policy engine: pure fail-closed `evaluate()`; table test over all 200 Risk × status × grant × mode combinations, with exactly 6 allowed and all listed
- [x] P3-T03 Approvals: `args_hash` = sha256 of canonical `{tool, arguments}` (checked against `shasum` vectors); single use through a conditional `UPDATE … RETURNING`; preview shows tool, risk and every argument
- [x] P3-T04 Executor, tasks and audit: `Tool::execute` requires `loki_policy::Allowed` (private, not `Clone`, bound to request + hash; ADR-0011, owner-approved crate edge); semaphore, deadline, cancel, 64 KiB cap; legal transitions only; startup marks unfinished tasks `interrupted`
- [x] P3-T05 Agent tool loop: validate → policy → `[y/N]` approval (default no; only a line typed after the prompt counts) → execute → audit; results wrapped `<tool_result untrusted="true">`; 6 calls per turn (config), then the model is told
- [x] P3-T06 `time.now` (jiff, IANA zones) and `system.info` (no host/user); manual run recorded in `docs/checklists/p3-native-smoke.md`
- [x] P3-T07 Platform trait, `MockPlatform`, macOS adapter (bundle discovery, `open -b`, process-verified launch), `app.list` + `app.open`; native smoke test passed
- [x] P3-T08 `files.search`/`files.read`: canonical roots, opaque file IDs, re-resolution on every read, no symlink following, hidden/system files skipped, text types only, 256 KiB cap
- [x] P3-T09 `loki audit list [--tool] [--since]` / `audit show`; test proves no file contents in output, DB, WAL or logs
- [x] P3-T10 `tests/security` (crate `loki-security-tests`): SEC-01..06 and SEC-10, named by ID. *Run locally; CI not yet.*

P3 gate: [x] an allowed tool works end to end from the CLI; [x] denied, malformed, unknown, replayed and path-escaping requests can't execute (SEC tests pass locally); [x] every execution has an audit row, and startup recovery never replays; [x] warm dispatch overhead measured: p95 0.76 ms release / 1.0 ms debug, target < 250 ms (`docs/benchmarks/p3-dispatch.md`); [ ] SEC tests passing **in CI**.

## P4 tickets

- [x] P4-T01 Voice state machine: pure lifecycle with typed events; 117 state/event combinations
  (9 states incl. both muted variants × 13 events) tested, plus mute, cancellation, device loss,
  failure, silence and speaking sequences. Review fix (2026-11-27): mute now keeps the cleanup
  barrier (`Muted { cleanup_pending }`) and a `Failed` event covers STT, agent and TTS errors.
  Ten voice tests. Source commit `7861b7b`; see `docs/handoffs/p4-t01.md`.
- [x] P4-T02 Audio capture: CPAL default input, bounded oldest-frame replacement with counting,
  mono conversion, continuous Rubato resampling to 16 kHz, typed errors and owning stream guard.
  11 new tests; manual capture recorded in `docs/benchmarks/p4-capture.md` (0 dropped frames).
- [x] P4-T03 VAD and silence timeout: earshot 1.2.2 behind `SpeechDetector` (ADR-0013); 16 ms frames over
  any chunking; 1.2 s silence timeout, 30 s cap, 400 ms speech confirmation (measured: earshot's
  noise-onset transient reaches 352 ms). Synthetic vowels and noise generated in Rust at test time
  (bit-identical to the earlier WAV/Python fixtures, which were git-ignored and are removed).
- [ ] P4-T04..T09: not started. No STT, TTS or CLI voice integration yet.

The owner explicitly authorized starting P4 before earlier gates passed, overriding the normal
phase-order rule. P3 implementation and its recorded validation remain intact.

## P5 tickets

- [x] P5-T01 Scaffold: `apps/desktop` (Tauri 2.12.5, React 19.3.1, TS strict, Vite 8.3.8) and `packages/ui`; ESLint 10 + Prettier + Vitest 5; JS job in CI; Linux WebKitGTK build deps in CI. `pnpm --filter desktop tauri dev` runs (app stays up, Vite serves, runtime opened). Least-privilege ACL from the start: one app command (`app_status`), one capability, no core plugins; strict CSP. *CI not yet run.*
- [x] P5-T02 Runtime API and event streaming: `TurnEvent` contract over one Tauri Channel per turn; runtime `chat_turn` / `stop_turn` / `respond_approval` (approval broker: single-use answers, waits at most until expiry); 7 thin commands, each in the app manifest and the capability. Acceptance: Vitest test receives the mock provider's streamed reply through a real `Channel`; Rust tests drive `chat_turn` with MockProvider (event order, approve/reject, stop).
- [x] P5-T03 Chat UI: streaming messages (aria-busy while streaming, `log` region), Stop (button or Escape), Retry, conversation list with New, select, rename (Enter/Escape, focus restored) and delete (inline confirm, focus on Cancel), Up/Down navigation, visible focus. 20 Vitest component tests on a fake backend that streams through real Channels. Runtime + commands for rename/delete. *Interactive manual run pending the owner* (`docs/checklists/p5-chat-manual.md`); the agent verified the app starts.
- [x] P5-T04 Approval dialog: tool, plain-language risk, every argument (invisible/bidi characters shown as `\u{…}`), countdown; focus on Deny; Escape denies; Tab trapped; auto-repeat ignored; a Sensitive action can only be approved by keyboard after the user moved focus to Approve. `respond_approval` now carries the shown arguments: the broker refuses a "yes" that is expired or doesn't match (delivered as a refusal). Acceptance: `approving_is_bound_to_the_shown_arguments`, `a_large_integer_rounded_by_the_webview_does_not_match`, `an_expired_approval_is_refused_but_a_no_is_always_accepted`, `mock_approval_for_different_shown_arguments_runs_nothing` (Rust) and 13 Vitest tests in `ApprovalDialog.test.tsx`. *Interactive run pending the owner* (`docs/checklists/p5-chat-manual.md`).
- [x] P5-T05 Permissions screen: every capability with status and reason (unavailable ones can't be allowed), Allow/Revoke, allowed folders added through the native picker opened from Rust (no `dialog:` permission for the webview, ADR-0014), Allow/Revoke reading per folder, Remove with confirmation. Runtime fixes found on the way: root changes now reach the file tools without a restart, and removing or repointing a root revokes its grants. Acceptance: `mock_revoking_a_grant_immediately_denies_the_next_call` (same runtime: Succeeded, revoke, Denied) plus 4 more runtime tests and 9 Vitest tests. *Interactive run pending the owner* (`docs/checklists/p5-chat-manual.md`).
- [ ] P5-T06 **partial**: provider/model/network mode/retention settings apply to the live runtime; native-dialog JSON history export (new file only), confirmed clear-history with DB/WAL cleanup, and redacted audit view. Typed generated contracts and 9 new Rust / 9 new component tests, including state refresh after partial cleanup failure. Memory, wake word and autostart are explicitly unavailable. FTS/memory acceptance awaits P7 (autostart shipped in P5-T07); native save-dialog checks not run. See ADR-0015 and `docs/handoffs/p5-t06.md`.
- [ ] P5-T07 **implemented, acceptance pending the owner**: tray menu (show/hide, mute, emergency stop, quit), close hides the window, Quit refuses new turns, cancels work and waits up to 2 s for partial replies/audit, single-instance lock (verified: a second launch hands off and exits in ~2 s), opt-in start at login (unavailable in dev builds and with overrides; shows the OS's real state). Started by Codex; finished, tested and fixed in review (ADR-0016). Acceptance is the manual `docs/checklists/desktop-lifecycle.md` run, not done by the agent.
- [x] P5-T08 Voice UI: header voice bar on every screen with the listening indicator (shown whenever the runtime reports `mic_open` = `VoiceState::requires_capture()`), state display, hold-to-talk button and shortcut (Ctrl+Shift+Space in the window), and Mute (also in the tray, whose label and tooltip follow the voice state). Runtime `VoiceControl` owns the state machine and streams status; emergency stop and quit cancel voice. **Push-to-talk is shown unavailable, with the reason, until speech-to-text exists (P4-T04)**; mute works now (ADR-0017). Acceptance: `fixtures/voice-states.json` is generated from `VoiceState::ALL` by `voice_indicator_fixture_matches_the_state_machine`, and Vitest checks the indicator for every row (18 cases). *Interactive run pending the owner.*
- [ ] P5-T09..T10: not started.

## Open decisions

| ID | Decision needed | Needed by | Notes |
| --- | --- | --- | --- |
| ~~D-01~~ | ~~Reference OS~~ | | **Decided:** macOS, ADR-0003 |
| ~~D-02~~ | ~~How TS contracts are generated~~ | | **Decided:** ts-rs, ADR-0005 |
| ~~D-03~~ | ~~Migration library~~ | | **Decided:** hand-rolled `user_version` runner, ADR-0007 |
| D-04 | ~~VAD engine~~ **decided: earshot, ADR-0013**; wake-word engine still open | P4-T08 | Re-evaluate VAD on real recordings in P4-T09 |
| D-05 | Project license | before any public release | No `license` field set yet |
| D-06 | Web search provider (self-hosted SearXNG, an API with a key, ...) | P7-T01 | The key is kept in the OS secret store |
| D-07 | **Accept MPL-2.0 `option-ext`** (pulled in by `directories`), **Tauri's MPL-2.0 Servo crates** (`cssparser`, `cssparser-macros`, `selectors`, `dtoa-short`) **MIT-0 `borrow-or-share`** (pulled in by `jsonschema`) **and ISC `libloading`** (Linux tray backend, P5-T07)? | before any release | Allowed by a crate-scoped exception in `deny.toml` for now. If rejected: replace `directories` (hand-rolled paths, or Tauri path APIs from P5) and delete the exception. See `docs/p0/licenses.md` |
| D-08 | **App identifier** (bundle ID, e.g. `com.<owner>.loki`): `tauri.conf.json` uses the placeholder `dev.loki.placeholder` | before the first installer (P5 gate) | ADR-0002 deferred it to P11; the P5 gate needs an installer. Loki's data dir comes from `directories`, not this ID, so changing it later doesn't move user data |

## Blockers

- **CI evidence still outstanding in these notes**: P1-T01, P1-T03, P1-T09 and P3-T10 need CI
  results. Local history now includes the P3 merge (`531d39c`) and work continues on `p5/desktop`.
  Remote CI results were not inspected in this session; the previous "not pushed" note was stale.
- **P5-T06 later-ticket dependencies**: the current schema has no personal memory or FTS tables;
  export/delete/FTS verification must be completed when P7 adds them. Autostart shipped in P5-T07.
  Do not tick the full acceptance until those integrations and native checks are recorded.

## Progress log

Newest first. Format: `YYYY-MM-DD | ticket | what changed | commands run → result`.

- 2026-11-28 | UI redesign (owner request, before P5-T09) | New desktop design system: light/dark tokens, icon rail (Chat/Permissions/Settings) and top bar (model status, voice), welcome screen with suggestion cards (they fill the box, never auto-send), avatars, safe Markdown replies (code blocks with Copy, lists, headings, quotes; no HTML, links not clickable), thinking dots and streaming caret, tool-activity chips, Copy reply, conversation search, auto-growing composer with icon buttons, restyled approval card and permissions/settings pages. Inline SVG icons only (strict CSP, no remote assets). Fixed a P5-T05 bug: the hidden chat section stayed visible above Permissions/Settings (`display:flex` beat `hidden`). | `./scripts/check.sh` → all passed (318 Rust + 90 JS tests; 10 new: Markdown safety and rendering, suggestions, search, thinking state). Visually checked with headless Chrome screenshots of welcome, chat, approval, permissions and settings in light and dark against the fake backend.
- 2026-11-28 | P5-T08 | Voice UI + runtime `VoiceControl` (`loki-runtime::voice`), `VoicePhase`/`VoiceStatus` contracts, `VoiceState::ALL` in loki-voice, 5 thin commands (status stream over a Channel, no extra permission), tray Mute made real, emergency stop/quit cancel voice. ADR-0017. Also restored the P5-T07 and P5-T06-fix STATUS entries that a script failed to write in the previous commit. | `./scripts/check.sh` → all passed (318 Rust + 80 JS tests). Mutation: hiding the indicator while Recording fails 4 Vitest tests. Real app starts (20 s, Vite 200), capability grants 27 Loki commands. Window/tray interaction not run (agent can't operate them).
- 2026-11-28 | P5-T07 | Review of the Codex P5-T07 work in progress, then finished: fixed the clippy error that broke the desktop build; locked nix 0.31.3 and zcheapstr 1.2.0 (0.31.4 / 1.4.0 were under two weeks old); crate-scoped ISC exception for `libloading` (Linux tray, D-07); `AutostartStatus` generated to TS; Settings "Start at login" toggle wired to the OS state (removed the hard-coded `SettingsSnapshot.autostart`); 5 runtime lifecycle/autostart tests and 3 component tests added (there were none). ADR-0016. | `./scripts/check.sh` → all passed (310 Rust + 57 JS tests), cargo deny ok. Real app: a second launch exits by itself in ~2 s with one process left, Ctrl+C clean. Tray clicks, close-to-hide, Cmd+Q drain and reboot/autostart not run (need the owner).
- 2026-11-28 | P5-T06 fix | Review of the Codex P5-T06 commit. Two bugs fixed: (1) Settings showed and saved this run's provider override, so saving any setting in a `--mock` run or before onboarding wrote the mock to config permanently; now shows the saved provider and switches only on a real change. (2) A retention failure at startup aborted `Runtime::open`, so the app refused to start; now a startup warning, retried later. | Regression tests `mock_saving_settings_never_persists_a_session_provider_override` (failed before the fix) and `mock_a_retention_failure_at_startup_is_a_warning_not_a_failed_start`. Fix verified alone in a clean worktree: fmt, clippy, 304 Rust tests passed.

- 2026-11-28 | P5-T06 (partial) | Continued Claude's P5-T05 handoff. Live model/provider/network/retention settings, new-file JSON export through Rust's native dialog, confirmed history deletion with vacuum/WAL cleanup, audit view and typed partial-failure refresh. No new deps/migrations; P3/P4 intact. ADR-0015 records memory/FTS (P7) and autostart (P5-T07) dependencies. | Full `check.sh` with installed Node 24 → all passed (302 Rust + 54 JS tests); production desktop frontend build passed; ACL has 20 Loki commands and no plugin grants. Native/browser interaction unavailable and not claimed. Unrelated package author edit preserved outside the ticket commit.

- 2026-11-28 | P5-T05 | Permissions screen + 6 thin commands, `PermissionRow`/`AllowedRootInfo` contract types (moved/added in loki-core), tauri-plugin-dialog 2.8.4 (tauri-plugin locked to 2.7.2 for the two-week rule; ADR-0014). Fixed: roots added at runtime didn't reach the file tools until restart; a removed root's grant carried over to a new folder with the same ID. UI bug found by a test: React reused the focused "Remove folder" button as the confirm row's "Remove" (focus on the destructive action); fixed with keys. | `./scripts/check.sh` → all passed (293 Rust + 45 JS tests), `cargo deny` ok, no new exceptions. Mutation: without the tool reload, the runtime-root test fails. `dev.sh --mock --fresh` → app up in 17 s, Vite 200, runtime opened; generated capability grants only Loki's 15 commands. Interactive run not done.
- 2026-11-28 | P5-T04 | Approval dialog (`apps/desktop/src/approval/`), `respond_approval(approval_id, approved, shown_arguments)` with broker-side binding and expiry checks, fake backend approvals. Contract change to a P5-T02 command (one caller, this app), documented in CONTRACTS.md and SECURITY_MODEL.md. | `./scripts/check.sh` → all passed (287 Rust + 36 JS tests). Mutation checks: making the broker accept every "yes" fails 3 Rust tests; removing the Sensitive keyboard guard or the auto-repeat guard fails their Vitest tests. Interactive run not done (agent can't operate the window).
- 2026-11-28 | dev | `scripts/dev.sh` (+ `pnpm dev`, `dev:mock`, `dev:cli`): one-command local run of Ollama (loopback, `OLLAMA_NO_CLOUD=1`, stopped on exit only if the script started it) and the desktop app or CLI, with data in a gitignored `.dev-data/`. New `LOKI_PROVIDER=mock\|ollama` override for the desktop app (`loki_runtime::config::provider_from_env`). | `dev.sh --cli --fresh --model bonsai-2:4b` → started Ollama, real bonsai-2:4b reply, Ollama stopped on exit. `dev.sh --mock` → loki-desktop up in 5 s, Vite 200; Ctrl+C → no processes left, port 1420 free. `--check` variants, unknown flag (exit 2) and `--fresh --real-data` (refused) checked.
- 2026-11-28 | P5-T03 | Chat UI (ConversationList, MessageList, Composer, ChatView), `streamTurn` (waits for both the command result and the terminal event, since Tauri doesn't order them), runtime `rename_conversation` / `delete_conversation` + commands. | `./scripts/check.sh` → all passed (282 Rust + 23 JS tests). `tauri dev` starts, Vite 200. Interactive run not done (no way to operate the window from the agent).
- 2026-11-28 | P5-T02 | Streaming runtime API and desktop commands (see CONTRACTS.md). Verified in Tauri 2.12 source: with an app ACL manifest every custom command is ACL-checked, and the channel data-fetch command is exempt, so streaming needs no extra permission. | `./scripts/check.sh` → all passed (280 Rust + 11 JS tests). `tauri dev` starts with all 7 permissions generated.
- 2026-11-28 | P5-T01 | Desktop scaffold (Tauri 2 stable; 3.x is alpha), `packages/ui` (StatusPill), root ESLint/Prettier, `AppStatus` contract type, `Runtime::open_for_app` (labelled mock fallback before onboarding) and `Runtime::status`. Fixed: `open` now rejects a missing model before installing the global logger (the first `tauri dev` run panicked on a second logger init). New D-07 crates (Tauri's MPL-2.0 Servo deps) and D-08 (app identifier placeholder). All versions at least two weeks old. | `./scripts/check.sh` → all passed (273 Rust + 7 JS tests, eslint, prettier, tsc, cargo deny). `tauri dev` with a throwaway data dir: app ran 18 s, Vite HTTP 200, log `runtime opened provider="mock"`, no errors.
- 2026-11-28 | P4-T03 | Finished the in-progress VAD. Found: (1) its `.wav` fixtures were git-ignored (`*.wav`), so CI could not compile the tests; (2) they came from a Python script; (3) the noise test failed even with those fixtures, because earshot scores noise onsets above 0.5 for up to 352 ms, longer than the 80 ms confirmation. Ported the generator to Rust (0 differing samples vs the WAVs), removed the WAVs and script, raised the default confirmation to 400 ms, added continuous-noise-bed tests, wrote the missing ADR-0013. Reviewed P4-T02 (`c040e5e`): correct; standalone fmt/clippy/259 tests/cargo deny pass. | `cargo test -p loki-voice` → 33 passed (VAD 12); measurements in ADR-0013.
- 2026-11-27 | P4-T02 | Explicit capture, synthetic-source trait/tests, bounded queue and 16 kHz mono FFT conversion; ADR-0012, exact-pinned reviewed dependencies, Linux ALSA CI prerequisite. Native probe: 143872 frames at 48 kHz → 47958 at 16 kHz, no drops, no audio saved. First native attempt returned Backend; retry succeeded. | Voice tests: 21 passed; clippy passed. Initial full run hit stale Claude scratch-path test artifacts; fresh `CARGO_TARGET_DIR=/private/tmp/loki-p4-verified-target ./scripts/check.sh` → all passed, 259 tests, contracts/tsc/cargo deny passed. See `docs/handoffs/p4-t02.md`.

- 2026-11-27 | P4-T01 review | Reviewed the P4-T01 state machine. Fixed: muting during active work skipped the cancel path's cleanup barrier, so after Unmute a new session (second mic stream) could start before the old work was released. Now `Muted { cleanup_pending: true }`; `CleanupComplete` clears it; Unmute while pending goes to `Cancelled`. Added `Failed` (Transcribing/Thinking/Speaking → Idle) so errors aren't reported as user cancels. Not changed: text-only replies still pass through Speaking (decide in P4-T06); no serde on VoiceState yet (needed in P5). | `cargo test -p loki-voice` → 10 passed; clippy clean; full `./scripts/check.sh` → all passed, 248 tests.
- 2026-11-27 | P4-T01 integration | Copied the isolated voice state machine from `7861b7b` into main on owner request; reconciled status with completed P3. File-hash comparison confirmed P3 implementation unchanged; existing staging state preserved. | `./scripts/check.sh` outside sandbox → all passed, 244 tests, fmt/clippy, generated contracts, strict TypeScript and cargo deny. P4-T02 is next; no audio capture or CLI voice wiring yet.

- 2026-11-27 | P3-T01..T10 | Tools, policy, approvals, executor, audit, macOS adapter, file tools, CLI (`permissions`, `roots`, `apps`, `audit`, approval prompt), `tests/security`. Owner decisions: reference OS **macOS** (ADR-0003), crate edge tools → policy (ADR-0011). Migration 0003; config v3 (`tools` limits). New deps checked: jsonschema (MIT, no default features), sha2, jiff, plist; MIT-0 `borrow-or-share` given a crate-scoped exception (D-07). Fixed a P2 REPL bug: a closed stdin made the reply loop spin. | `./scripts/check.sh` → all passed, 238 tests, cargo deny ok after the exception. Manual (real bonsai-2:4b, real macOS): model called `time.now` → correct Tokyo time; "Open the Calculator app" → `app.list`, approval prompt, `y` → Calculator launched and verified (206 ms); audit rows for all three. Dispatch overhead p95 0.76 ms.
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
