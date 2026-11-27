# P0: Feasibility

| | |
| --- | --- |
| **Effort** | 1–2 weeks |
| **Depends on** | nothing |
| **Release** | none (spikes and decisions) |
| **Produces** | `docs/p0/*.md` reports, `spikes/*` (throwaway, outside the Cargo workspace), ADRs |

## Goal

Before building, prove that the risky parts work on the hardware actually available: local
inference with tool calling, audio capture with local STT, Tauri packaging, and native permission
APIs. Then record the decisions.

## Rules for spikes

- Each spike lives in `spikes/<name>/` with a `README.md` that holds the exact commands to
  reproduce it, the result, and the hardware/OS it ran on.
- Spikes are **not** workspace members and are never imported by the product. Code worth keeping
  is rewritten properly in its phase.
- Record the numbers you actually measured, not expected ones.

## Tickets

### P0-T01: Toolchain, versions, and a building scaffold
- **Do:**
  - Install rustup, Node LTS, pnpm and the Tauri prerequisites for the dev OS.
  - Pin the exact Rust version in `rust-toolchain.toml`, and `packageManager` in `package.json`.
  - Run `scripts/check.*` on the existing scaffold and fix any compile, fmt or clippy issues. The
    scaffold was written without a compiler.
- **Output:** `docs/p0/environment.md` with OS, CPU, RAM, GPU, disk and every tool version.
- **Acceptance:** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` all pass locally. The output is pasted in the handoff.

### P0-T02: Reference OS decision
- **Do:** Decide which desktop OS is the reference implementation: the one P3–P5 target and where the lifecycle gates run. Fill in and accept ADR-0003.
- **Acceptance:** ADR-0003 is `Accepted`, and STATUS.md's "Reference OS" is filled in. If the choice isn't macOS, P3/P5 ticket wording that names macOS is understood to mean the reference OS, and P6 covers the remaining desktops.

### P0-T03: Local inference spike
- **Do:**
  - Install Ollama and pull 1–2 small instruct models suited to the RAM table in VOICE_AND_MODELS.
  - Call `/api/chat` with streaming and with a `tools` definition (e.g. `time.now`).
  - Measure cold load, first-token latency, tokens/s and peak RAM.
  - Check that the model returns a well-formed tool call at least 9 times in 10.
  - Test cancellation (drop the connection mid-stream).
- **Output:** `spikes/ollama/`, `docs/p0/inference.md`.
- **Acceptance:** Streaming and tool calling are demonstrated, the numbers are recorded, and a recommended default model is named, with its license.

### P0-T04: Audio and STT spike
- **Do:**
  - Use CPAL to capture 5 seconds of mono audio from the default mic, resample it to 16 kHz, and transcribe it with whisper.cpp (via a Rust binding, e.g. `whisper-rs`) using a small model.
  - Measure transcription time.
  - Try the native TTS for one sentence.
- **Output:** `spikes/audio/`, `docs/p0/audio.md`.
- **Acceptance:** The transcript is printed, latency recorded, and a mic-permission prompt observed (or the OS behaviour documented). No audio file is left behind.

### P0-T05: Tauri 2 shell and packaging spike
- **Do:**
  - Run `pnpm create tauri-app` with React + TypeScript in `spikes/tauri-shell/`.
  - Add a tray icon, close-to-hide, and a single-instance plugin.
  - Build an installer for the reference OS.
- **Output:** `docs/p0/packaging.md`, with installer size, build time and signing requirements and costs.
- **Acceptance:** The installer installs and launches; the tray works; closing the window hides the app; Quit exits.

### P0-T06: Native permissions and app-discovery spike
- **Do:**
  - On the reference OS, enumerate installed apps and launch one.
  - Query microphone permission status.
  - Toggle login-startup through the supported API.
- **Output:** `spikes/platform/`, `docs/p0/platform.md`.
- **Acceptance:** Each API is demonstrated or documented as not possible, with the reason.

### P0-T07: License review
- **Do:** List the license of: Tauri, every planned crate, Ollama, llama.cpp, whisper.cpp, each candidate model (LLM and STT), the VAD and wake-word candidates, and the TTS voices.
- **Output:** `docs/p0/licenses.md`: a table of component, license, redistribution allowed (y/n) and notes.
- **Acceptance:** No component with unclear or incompatible redistribution terms is planned for a release without a recorded decision.

### P0-T08: Verify the platform capability matrix
- **Do:** Check each row of `docs/PLATFORMS.md` against current official docs, and add minimum OS versions and links.
- **Acceptance:** PLATFORMS.md updated, and the "design intent" banner replaced with "verified on <date>".

### P0-T09: Feasibility report and P1 plan
- **Do:** Summarize T03–T08: what works, what doesn't, the risks, and any changes to the plan. Write ADRs for the decisions taken (default model, Tauri plugins, etc.). Adjust P1 tickets if needed.
- **Output:** `docs/p0/feasibility-report.md`.
- **Acceptance:** The report exists, its decisions are in ADRs, and STATUS.md points to P1-T01.

## Phase gate

- [ ] Every spike has a README with reproducible commands, and someone else could re-run it.
- [ ] Inference, audio, packaging and permission smoke tests are recorded with real numbers.
- [ ] ADR-0003 (reference OS) is accepted, and the license review is complete.
- [ ] The scaffold builds and all checks pass (P0-T01).

## Out of scope

Production code in `crates/` beyond fixing the scaffold to compile. UI design. Mobile.
