# Development environment (P0-T01)

Recorded 2026-11-27 on the dev machine. Every check in `scripts/check.sh` passes here.

## Hardware and OS

| Item | Value |
| --- | --- |
| Machine | MacBook Air, Apple M2 (4 performance + 4 efficiency cores), integrated GPU (Metal) |
| RAM | 8 GB unified memory |
| Disk | 460 GiB volume, 48 GiB free |
| OS | macOS 27.0.1 (build 26A434), arm64 |

8 GB of unified memory limits local models to the small end of the VOICE_AND_MODELS RAM table.
P0-T03 must measure this.

## Tools

| Tool | Version | How installed | Needed for |
| --- | --- | --- | --- |
| rustup | 1.29.0 | official installer | everything |
| Rust (rustc / cargo) | 1.97.1, pinned in `rust-toolchain.toml` (installs automatically) | rustup | everything |
| Installed Rust targets | `aarch64-apple-darwin` | rustup | iOS targets are added in P9 |
| cargo-deny | 0.22.1 | `cargo install cargo-deny --locked` | supply-chain checks |
| Node.js | 24.8.0 (24.x is the "Krypton" LTS line; latest 24.x on 2026-11-27 is 24.22.3, and 26.x "Lithium" is also LTS) | Homebrew | contracts (P1), UI (P5) |
| pnpm | 10.15.1, pinned in `package.json` `packageManager` | npm global | JS workspace |
| TypeScript | 6.0.3 (devDependency of `packages/contracts`) | pnpm | contract type-check |
| Xcode | 26.6 (build 17F113) | App Store | Tauri desktop; iOS (P9) |
| Command Line Tools | 27.0.0; Apple clang 21.0.0 | `xcode-select --install` | native builds, bundled SQLite |
| Homebrew | 6.0.18 | official installer | Ollama, CocoaPods |
| CocoaPods | present (`/opt/homebrew/bin/pod`) | Homebrew | iOS only (P9) |
| Ollama | 0.40.18 (MIT) | `brew install ollama` | local inference (P0-T03, P2) |
| git | 2.50.1 (Apple Git-155) | Xcode CLT | version control |

## Tauri 2 prerequisites (macOS)

Checked against <https://v2.tauri.app/start/prerequisites/> on 2026-11-27: desktop needs the Xcode
Command Line Tools (or Xcode) and Rust, plus Node LTS for a JS frontend. The minimum OS is macOS 10.15.
All of these are present. iOS additionally needs full Xcode (present), CocoaPods (present) and the
Rust iOS targets (not added yet: P9).

## Ollama notes

- Installed as a plain binary. **No background service** is registered (`brew services` shows
  `none`), and nothing is listening on port 11434 until someone runs `ollama serve`. Start it only
  when needed: `ollama serve` in a terminal, or `brew services start ollama` for login start.
- No models pulled yet (that is P0-T03).

## Acceptance output (P0-T01)

```text
$ cargo fmt --all -- --check                               -> ok
$ cargo clippy --workspace --all-targets -- -D warnings    -> Finished, 0 warnings
$ cargo test --workspace                                   -> 98 passed, 0 failed
$ ./scripts/check.sh                                       -> All checks passed. (also contracts, tsc, cargo deny)
```

## Not done / follow-ups

- Node 24.8.0 is behind on 24.x security releases. Upgrading (`brew upgrade node`) affects every
  project on this Mac, so that's the owner's call.
- Windows and Linux toolchains are not set up here; CI covers them once the repo is pushed.
