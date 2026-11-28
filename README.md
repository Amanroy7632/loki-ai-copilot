# Loki

Loki is a local-first, voice-enabled personal copilot. It talks with you, remembers what you allow it to, researches the web when you opt in, and runs **approved** actions on your own devices. By default it needs no cloud account and runs no remote inference.

> **Status:** P3 (tools, policy, approvals, audit) is written and passes all checks locally; the P1–P3 gates still need CI.
> P0: T01–T03 done. P3 and P4-T01 are on branch `p3/tools-policy`. See [docs/STATUS.md](docs/STATUS.md).

## What Loki is, and is not

| Loki v1 will | Loki v1 will not |
| --- | --- |
| Chat with streaming answers, stop and retry, saved locally | Train its own foundation model |
| Use push-to-talk speech locally (optional wake word, desktop only) | Listen all the time on iOS, or listen secretly anywhere |
| Launch allowed apps, search and read approved folders, report system info | Run arbitrary shell commands written by the model |
| Search the web with cited sources, after you opt in | Make purchases or destructive changes unattended |
| Live in the tray or menu bar, keep reminders, keep an audit history | Get around OS security (UAC, macOS privacy prompts, Wayland) |
| Pair a phone to your desktop securely | Run as a public, multi-user cloud service |

## Getting started (macOS, today)

What works right now: the `loki` CLI chatting with a **local model through Ollama**, with
streaming, stop, retry and saved history. It works fully offline once a model is downloaded and
never uses cloud inference. An early desktop chat window (P5) also runs; tools, voice and the rest
of the desktop app come in later phases.

### Quick start: everything with one command

`scripts/dev.sh` starts Ollama (loopback only, cloud features off) if it isn't running, installs
the JS dependencies if needed, and opens the desktop app with hot reload. Ctrl+C stops everything
it started; an Ollama server that was already running is left alone. Data goes into `.dev-data/`
in the repo (gitignored), so your normal Loki data isn't touched.

```sh
pnpm dev                                       # = ./scripts/dev.sh: desktop app + Ollama
./scripts/dev.sh --model bonsai-2:4b           # also select that model for .dev-data/
./scripts/dev.sh --model bonsai-2:4b --pull    # download it first if missing (license shown first)
pnpm dev:mock                                  # no Ollama: the labelled mock model
pnpm dev:cli                                   # the terminal chat instead of the window
./scripts/dev.sh --fresh                       # start from an empty .dev-data/ (deletes only that)
./scripts/dev.sh --real-data                   # use your normal per-user Loki data
./scripts/dev.sh --check                       # print the plan, start nothing
```

Flags combine, e.g. `./scripts/dev.sh --cli --fresh --model bonsai-2:4b`. The first run compiles
the Rust workspace, which takes a few minutes. Run it from Git Bash on Windows. With
`LOKI_PROVIDER=mock` set, the desktop app always uses the mock model.

The steps below do the same by hand, with the installed `loki` command.

### 1. One-time setup

```sh
xcode-select --install                         # if Xcode / Command Line Tools are missing
brew install ollama                            # local model server
cargo install --path crates/loki-cli --locked  # the `loki` command, into ~/.cargo/bin
```

### 2. Start Ollama (loopback only, cloud features off)

```sh
./scripts/ollama-serve.sh      # leave this terminal running
```

Don't use `brew services start ollama` or the Ollama app if you want Loki's privacy settings;
those run with cloud features **on** by default.

### 3. Pick and download a model

```sh
loki models recommend          # checks RAM/disk, suggests a reviewed model (8 GB Mac: bonsai-2:4b)
loki models pull bonsai-2:4b   # shows the license first, then downloads (~1.6 GB; Ctrl+C cancels)
loki models use bonsai-2:4b    # saves it as your default
```

### 4. Chat

```sh
loki chat                        # Ctrl+C or /stop stops a reply, /retry regenerates, /quit exits
                                 # /model shows the model, /model qwen3:4b switches for this session
loki history list                # saved conversations: <id>  <title>
loki history show <id>           # every message (stopped replies are marked [stopped])
loki chat --conversation <id>    # continue one
loki --provider mock chat        # no model needed: canned echo replies, for testing
```

Your data lives in `~/Library/Application Support/Loki/` (`loki.db`, `config/config.json`,
`logs/`). Logs are redacted; `LOKI_LOG=debug` makes them chattier. `--data-dir <dir>` (or
`LOKI_DATA_DIR`) uses a separate folder, e.g. for experiments.

### 5. Let Loki use tools (P3)

Nothing is allowed by default. Grant what you want, and Loki asks before anything that changes your Mac:

```sh
loki permissions list                       # capabilities, their status, your grants
loki permissions grant clock                # time.now
loki permissions grant system_info          # system.info (OS, CPU, RAM, disk; no host or user names)
loki permissions grant app_launch           # app.list, and app.open (always asks [y/N] first)
loki roots add docs ~/Documents             # a folder file tools may read (the model only sees "docs")
loki permissions grant file_read --scope docs
loki audit list                             # what ran, when, with redacted summaries
loki audit show <task_id>
```

In chat, try "What time is it in Tokyo?" or "Open the Calculator app." Revoke with
`loki permissions revoke <capability> [--scope <id>]`.

### 6. Before you commit

```sh
./scripts/check.sh               # fmt, clippy, tests, contracts, tsc, cargo deny
```

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
  rust-toolchain.toml    pinned Rust toolchain (1.97.1)
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
  scripts/               check.sh / check.ps1, dev.sh, gen-contracts.sh, ollama-serve.sh
  docs/                  the plan, architecture, contracts, phases, ADRs
```

## Quick commands

These need the Rust toolchain. Install it as described in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

```sh
./scripts/check.sh            # macOS / Linux: fmt check + clippy + tests
pwsh ./scripts/check.ps1      # Windows: the same checks
cargo run -p loki-cli         # headless harness
./scripts/dev.sh              # local end-to-end run: Ollama + desktop app (--cli, --mock)
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
