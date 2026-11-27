# Loki

Loki is a local-first, voice-enabled personal copilot. It talks with you, remembers what you allow it to, researches the web when you opt in, and runs **approved** actions on your own devices. By default it needs no cloud account and runs no remote inference.

> **Status:** project scaffold and plan only. Phase **P0 (feasibility)** has not started.
> The Rust scaffold was written on a machine without a Rust toolchain, so it has **not been compiled yet**. That is the first task in P0 (see [docs/STATUS.md](docs/STATUS.md)).

## What Loki is, and is not

| Loki v1 will | Loki v1 will not |
| --- | --- |
| Chat with streaming answers, stop and retry, saved locally | Train its own foundation model |
| Use push-to-talk speech locally (optional wake word, desktop only) | Listen all the time on iOS, or listen secretly anywhere |
| Launch allowed apps, search and read approved folders, report system info | Run arbitrary shell commands written by the model |
| Search the web with cited sources, after you opt in | Make purchases or destructive changes unattended |
| Live in the tray or menu bar, keep reminders, keep an audit history | Get around OS security (UAC, macOS privacy prompts, Wayland) |
| Pair a phone to your desktop securely | Run as a public, multi-user cloud service |

## Start here

- **AI coding agents** (Claude, GPT, Gemini, or any other LLM): read **[AGENTS.md](AGENTS.md)** first and follow it exactly.
- **Human developers:** start with [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for setup, then [docs/ROADMAP.md](docs/ROADMAP.md) for the phase plan, then [docs/STATUS.md](docs/STATUS.md) to see where work stopped.

## Repository layout

```text
loki/
  AGENTS.md              operating manual for AI coding agents (read first)
  CLAUDE.md              points Claude Code at AGENTS.md
  SECURITY.md            how to report issues; summary of the trust model
  Cargo.toml             Rust workspace, shared dependency versions and lints
  rust-toolchain.toml    pinned Rust toolchain (exact version pinned in P0-T01)
  pnpm-workspace.yaml    JS workspace (desktop app + shared UI, from P5)
  crates/
    loki-core/           domain types and contracts; no OS, UI, IO or async
    loki-llm/            model provider trait; mock and Ollama adapters
    loki-tools/          tool trait, registry, schema validation, built-in tools
    loki-policy/         policy engine, grants and approvals (fail closed)
    loki-memory/         SQLite repositories, migrations, FTS5 retrieval
    loki-voice/          audio capture, VAD, speech-to-text, TTS, voice state machine
    loki-platform/       OS adapters behind capability traits (cfg-gated)
    loki-agent/          orchestration loop and context building
    loki-runtime/        composition root: config, jobs, lifecycle, wiring
    loki-cli/            headless development harness (`loki` binary)
  apps/desktop/          Tauri 2 shell (created in P5)
  apps/mobile/           mobile targets (P8 and P9)
  packages/ui/           shared React components (P5)
  packages/contracts/    TypeScript types generated from loki-core (P1-T03)
  native/                Swift and Kotlin bridges (P6 to P9)
  migrations/            SQL migrations, forward-only and transactional
  fixtures/              test data (fake documents, scripted model replies)
  tests/                 cross-crate integration and security tests
  spikes/                throwaway P0 experiments, outside the workspace
  scripts/               check.sh / check.ps1 (fmt + clippy + test)
  docs/                  the plan, architecture, contracts, phases, ADRs
```

## Quick commands

These need the Rust toolchain. Install it as described in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

```sh
./scripts/check.sh            # macOS / Linux: fmt check + clippy + tests
pwsh ./scripts/check.ps1      # Windows: the same checks
cargo run -p loki-cli         # headless harness
```

## Documentation map

| Document | Answers |
| --- | --- |
| [docs/STATUS.md](docs/STATUS.md) | Where are we now? What is the next ticket? Which decisions are open? |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Which phases, in what order, and with which gates? |
| [docs/phases/](docs/phases/) | Exact tickets for each phase, P0 to P11 |
| [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md) | What must Loki do, and what is out of scope? |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | How is it built? Data flow, processes, crate dependency rules |
| [docs/CONTRACTS.md](docs/CONTRACTS.md) | Tool, provider, policy and platform interfaces; the initial tool registry |
| [docs/SECURITY_MODEL.md](docs/SECURITY_MODEL.md) | Threats, required controls, and the release-blocking security tests |
| [docs/DATA_MODEL.md](docs/DATA_MODEL.md) | SQLite schema, memory retrieval, retention and deletion |
| [docs/VOICE_AND_MODELS.md](docs/VOICE_AND_MODELS.md) | Voice state machine, model onboarding, provider rules |
| [docs/PLATFORMS.md](docs/PLATFORMS.md) | What each OS allows, native adapters, packaging |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | Setup, commands, coding standards, git conventions |
| [docs/adr/](docs/adr/) | Architecture decisions and why they were made |

## Origin

This plan comes from the *NOVA AI Cross-platform Engineering Blueprint v1.0* (27 November 2026), with the product renamed **Loki**. Everything needed from that blueprint is in `docs/`, so the PDF isn't required. If you have it, you can keep it at `docs/spec/` for reference.
