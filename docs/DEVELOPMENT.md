# Development guide

## 1. Prerequisites

Install these from the **official** instructions and record the versions in `docs/p0/environment.md` (P0-T01):

| Tool | Why | Check |
| --- | --- | --- |
| Rust via `rustup` | Whole core. `rust-toolchain.toml` selects the version automatically. | `rustc --version`, `cargo --version` |
| Node.js LTS | UI build (from P5) | `node --version` |
| pnpm | JS workspace | `pnpm --version` |
| Tauri 2 prerequisites | Desktop shell (from P5) | see https://v2.tauri.app/start/prerequisites/ |
| Ollama | Local inference (from P2) | `ollama --version` |
| Git | Version control | `git --version` |

Per OS:
- **macOS:** `xcode-select --install`.
- **Windows:** Visual Studio Build Tools with "Desktop development with C++", and WebView2 (preinstalled on Windows 11).
- **Linux:** the WebKitGTK and build packages listed in the Tauri prerequisites.

## 2. Everyday commands

```sh
# All checks (run before every commit; CI runs the same)
./scripts/check.sh          # macOS / Linux
pwsh ./scripts/check.ps1    # Windows (PowerShell 7+) or: powershell -File scripts/check.ps1

# Individually
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p loki-cli -- --help

# One crate / one test
cargo test -p loki-core
cargo test -p loki-core task::tests::approval_path_is_legal

# From P5 (desktop app)
pnpm install
./scripts/dev.sh               # Ollama + desktop app with hot reload (see --help)
pnpm --filter desktop tauri dev
pnpm --filter desktop tauri build
```

## 3. Coding standards

### Rust

- **Errors:** library crates define typed errors with `thiserror`. Only binaries (`loki-cli`, the
  Tauri app) may use `anyhow`. Never `unwrap()`/`expect()` outside tests; clippy enforces this.
- **Docs:** every public item has a doc comment explaining *why it exists* and any invariants.
  The `missing_docs` lint is on.
- **Logging:** use `tracing` with structured fields. Don't use `println!` except for CLI user
  output. Never log secrets, file contents or full prompts at `info` or above.
- **Async:** Tokio. No blocking calls inside async functions; use `spawn_blocking`. Never hold
  locks or DB handles across `.await`. Every long task takes a `CancellationToken`.
- **Unsafe:** denied workspace-wide. FFI modules in `loki-platform` and `loki-voice` may
  `#![allow(unsafe_code)]` at module level, with a `// SAFETY:` comment on every block.
- **Types over strings:** use newtypes for IDs and enums for states. Parse at the edges, and keep
  invalid states unrepresentable.
- **Module size:** a file over ~400 lines, or a function over ~60 lines, is a smell. Split it by responsibility.
- **Platform code:** gated by `#[cfg(target_os = "...")]` inside `loki-platform` only, with a mock for every trait.
- **Config, not constants:** limits (tool calls per turn, result caps, timeouts) live in config
  with documented defaults.

### TypeScript / React (from P5)

- `strict: true`, no `any`, ESLint and Prettier in CI.
- Use generated types from `packages/contracts` only. Don't hand-write IPC types.
- Components show capability status; never assume a feature exists.
- Components have no business logic; they call the runtime through typed commands.
- Accessibility: labelled controls, keyboard operable, visible focus, and respect for reduced motion.

## 4. Testing

| Level | Location | Notes |
| --- | --- | --- |
| Unit | `#[cfg(test)] mod tests` beside the code | Pure logic: policy, schemas, state transitions |
| Crate integration | `crates/<crate>/tests/` | Uses mocks and a temp data dir |
| Cross-crate / E2E | `tests/` (workspace-level test crate, added in P1-T09) | CLI-driven scenarios |
| Security | `tests/security/` | The SEC-xx list in SECURITY_MODEL. These are release blocking. |
| Native/manual | `docs/checklists/` | Real OS permissions, sleep/resume, installers |
| Benchmarks | `docs/benchmarks/` | Record the hardware, OS, model, numbers and date |

Test names describe behaviour: `denies_unknown_tool`, `approval_is_single_use`. Mocks carry `Mock` in their name.

## 5. Git conventions

- **Branches:** `p3/t02-policy-engine`.
- **Commits:** Conventional Commits with the ticket ID, for example:
  `feat(policy): P3-T02 fail-closed evaluate()`
  `fix(memory): P1-T05 enable foreign keys on open`
  `docs(adr): 0004 choose ts-rs for contract generation`
- One ticket per commit or PR. A PR description follows `docs/templates/handoff.md`.
- Never commit secrets, models, audio, databases or build output (see `.gitignore`).
- Don't rewrite shared history.

## 6. Adding a dependency

1. Check the latest version, maintenance status and license. MIT, Apache-2.0, BSD and Zlib are
   fine. Anything else needs approval and an entry in `docs/p0/licenses.md`.
2. Add it once under `[workspace.dependencies]` in the root `Cargo.toml` with an exact minor
   version, and use `dep.workspace = true` in the crate.
3. Commit the updated `Cargo.lock`.
4. If it is large, native, or touches security, write an ADR.

## 7. Writing an ADR

Copy `docs/adr/0000-template.md` to the next number. Fill in the context, the decision, the
alternatives considered and the consequences. ADRs are immutable once accepted. To change one,
write a new ADR that supersedes it.
