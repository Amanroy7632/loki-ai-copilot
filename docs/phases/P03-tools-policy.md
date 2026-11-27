# P3: Tools, policy and approvals (v0.3)

| | |
| --- | --- |
| **Effort** | 3–4 weeks |
| **Depends on** | P2 gate |
| **Release** | v0.3: the CLI can use approved tools on the reference OS |
| **Crates** | loki-tools, loki-policy, loki-platform, loki-memory, loki-agent, loki-runtime |
| **Must read** | CONTRACTS.md, SECURITY_MODEL.md |

## Goal

The model can propose tool calls. Loki validates them, applies policy, asks for approval when
needed, executes with limits, verifies the outcome and audits everything. **Everything else is
denied.**

## Tickets

### P3-T01: Tool trait and registry
- **Do:**
  - Define the `Tool` trait (`spec()`, `execute()`).
  - The registry rejects duplicate names and names not matching `^[a-z]+(\.[a-z_]+)+$`.
  - Validate arguments with the `jsonschema` crate.
  - A registry test asserts that **every** spec has `additionalProperties: false` and bounded strings and arrays.
- **Acceptance:** Unit tests cover duplicates, a bad name, schema violations and unknown fields.

### P3-T02: Policy engine
- **Do:**
  - Implement a pure `evaluate()` in `loki-policy`.
  - The order is: unknown tool → Deny; capability not `Supported` → Deny with the reason; no grant for the capability or scope → Deny; Network risk in `LocalOnly` → Deny; `LocalChange`/`Sensitive` → RequireApproval; otherwise Allow.
  - Any internal error → Deny.
- **Acceptance:** A table-driven test covers every Risk × CapabilityStatus × grant × network-mode combination. **There is no path to Allow that the table doesn't list.**

### P3-T03: Approvals
- **Do:**
  - Approval = `{ request_id, args_hash, expires_at, used_at }`, where `args_hash = sha256(canonical JSON of {tool, arguments})`, with sorted keys and no whitespace.
  - Approvals are single use and persisted in the `approvals` table.
  - The approval preview shows the tool, the risk and **every** argument.
- **Acceptance:** SEC-05 tests: reuse, expiry, changed arguments, and a different request ID all fail.

### P3-T04: Executor, tasks and audit
- **Do:**
  - A semaphore with configurable concurrency, a deadline and a cancellation token per call, and a result cap of 64 KiB by default.
  - Persist `TaskState` transitions through `can_transition_to`, and write a `tool_runs` row with a redacted summary.
  - On startup, mark non-terminal tasks `Interrupted`.
  - **The executor's API requires proof of policy**: an `Allowed` token type that only `loki-policy` can construct, or a consumed approval.
- **Acceptance:** Tests cover timeout, cancel, oversize result, illegal transition rejected, startup recovery (no replay), and that the executor can't be called without the policy token (a compile-time guarantee is preferred).

### P3-T05: Agent tool loop
- **Do:** Model tool call, then schema validation, policy, approval (CLI prompt `[y/N]`, default no) and execute; the result goes back to the model wrapped as `<tool_result untrusted="true">…</tool_result>` or a provider equivalent. Allow at most 6 calls per turn, then tell the model the limit was reached.
- **Acceptance:** MockProvider-driven tests cover an allowed call, a denied call (the model receives a `Denied` result), a rejected approval, and the limit being hit.

### P3-T06: `time.now` and `system.info`
- **Do:** Both are ReadOnly tools. `system.info` uses `sysinfo` and returns OS, CPU, RAM and disk only, with no hostname or username unless the user opts in.
- **Acceptance:** Unit tests, plus a manual CLI run recorded.

### P3-T07: App discovery and `app.open` (reference OS)
- **Do:**
  - Add the `Platform` trait in `loki-platform`, plus `MockPlatform` and the reference-OS adapter.
  - The adapter discovers apps into a registry of `app_id` and display name, and `app.open` accepts only discovered IDs.
  - Verify the launch (the process appears or the OS confirms it); if it can't be verified, say so.
- **Acceptance:** An approved installed app launches; an unknown `app_id` is rejected; a denied request doesn't launch; an audit row is written. Tested with mock unit tests plus a manual native smoke test. This matches the example ticket in the blueprint.

### P3-T08: `files.search` and `files.read`
- **Do:**
  - Allowed roots come from config, as `root_id` → canonical path.
  - Canonicalize every candidate path and check it is inside the canonical root **after** symlink resolution.
  - Refuse `..`, absolute paths, UNC paths and device paths.
  - Search returns opaque `file_id`s and root-relative display paths.
  - `files.read` supports plain text, Markdown, code and CSV, up to `max_bytes`.
  - Skip hidden and system files by default.
- **Acceptance:** SEC-02 and SEC-03 tests pass, including a symlink (or a junction on Windows) that points outside the root. The search limit is enforced.

### P3-T09: Audit CLI
- **Do:** `loki audit list [--tool] [--since]` and `loki audit show <task_id>`, showing redacted data only.
- **Acceptance:** Test checks that the output contains no file contents.

### P3-T10: Security test suite
- **Do:** Create `tests/security/` with SEC-01 to SEC-06 and SEC-10 as automated tests.
- **Acceptance:** All pass in CI, and each test is named after its SEC ID.

## Phase gate

- [ ] An allowed tool works end to end from the CLI.
- [ ] Denied, malformed, unknown, replayed and path-escaping requests **cannot execute** (the SEC tests pass).
- [ ] Every execution has an audit row, and startup recovery never replays.
- [ ] Warm dispatch overhead measured (NFR-03 target < 250 ms).

## Out of scope

Web tools (P7), developer tools beyond the read-only spec (`dev.docker_status` can be P7 or later), other OS adapters (P6).
