# Architecture

## Summary

There is one Rust workspace for all logic, plus one shared React/TypeScript UI, delivered through
**Tauri 2**. Native adapters handle OS capabilities. The product is desktop-first: the reference
OS comes first (ADR-0003), then the other desktops. Android and iOS are constrained companions.
There is no mandatory cloud account and no remote inference.

## Runtime data flow

```text
 Voice / Text / Shortcut
          │
   Shared Tauri UI  ◄──── typed IPC ────►  loki-runtime
          │                                    │
   Native lifecycle                       loki-agent (orchestrator)
                                  ┌────────────┼─────────────┐
                              loki-llm     loki-memory    loki-policy
                                                               │
                                                          loki-tools (registry + executor)
                                                               │
                                        loki-platform adapters / web / developer tools
                                                               │
                                                  verified result + audit record
                                                               │
                                                    response stream / TTS
```

## Execution lifecycle (one turn)

1. **Input:** text, transcribed voice, or a shortcut.
2. **Normalize:** trim, attach the session, and stamp the time.
3. **Load bounded context:** system prompt, recent turns, and retrieved memories with provenance, all within the token budget.
4. **The model proposes** either a text response or a typed tool call.
5. **Validate schema:** unknown tool or unknown/invalid argument means a `Denied` result back to the model.
6. **Policy evaluation:** `Allow`, `Deny` or `RequireApproval`, failing closed.
7. **User approval** if needed. It is bound to the exact arguments, the request ID and an expiry.
8. **Execute** with a deadline, a cancellation token and an output-size cap.
9. **Record the outcome** in a redacted audit row and the task-state transition.
10. **Return the tool result to the model** as labelled, untrusted data. Repeat, up to 6 tool calls per turn.
11. **Stream the final response** to the UI, and **speak it** if voice is enabled.

## Process model

- **MVP:** one Tauri process running async Rust tasks (Tokio). The model runtime (Ollama) is a
  separate process, managed or detected.
- Closing the window **hides** it. An explicit **Quit** cancels running work, releases the
  microphone and exits.
- **Later (desktop only):** a per-user background agent that survives UI restarts, talking to
  the UI over authenticated local IPC. This needs an ADR before it is built.
- **Mobile:** the OS controls the lifecycle. Persist task state, and never assume uninterrupted execution.

## Concurrency rules

- Bounded channels for audio frames. When the buffer is full, drop the oldest frames and count them; don't block capture.
- A semaphore limits concurrent tool jobs.
- Each task has its own cancellation token, and child tokens per turn and per tool call.
- Only one active voice session at a time.
- A configurable inference queue (default: one request at a time).
- **Never hold a database connection, transaction or mutex guard across `.await`.**
- Run blocking work (SQLite, whisper inference, file walks) in `spawn_blocking` or on dedicated threads.
- On startup, recover incomplete tasks as `Interrupted`. **Never replay their side effects.**

## Crate dependency rules

```text
loki-core      ◄── every crate (core depends on no internal crate)
loki-llm       ──► core
loki-policy    ──► core
loki-memory    ──► core
loki-voice     ──► core
loki-platform  ──► core
loki-tools     ──► core, platform, policy                (policy: ADR-0011, the `Allowed` permit)
loki-agent     ──► core, llm, tools, policy, memory     (traits only, no concrete adapters)
loki-runtime   ──► everything above                      (composition root)
loki-cli       ──► runtime, core
apps/desktop   ──► runtime, core                         (Tauri commands are thin)
```

These rules apply to every crate:

1. **Core has no UI or OS dependencies**, and no IO, database or async runtime either.
2. **The agent depends on traits, not concrete providers.** The runtime injects the implementations.
3. **Policy runs before tool execution.** The executor's API requires an `Allow` decision or a valid approval token.
4. **Platform adapters implement capabilities behind `#[cfg(target_os = ...)]`**, and every host also gets a mock.
5. **Tauri commands delegate to `loki-runtime`.** UI handlers contain no business logic.
6. **TypeScript contracts are generated from Rust** (P1-T03, ADR pending) and checked in CI.

Adding an edge to this graph needs an ADR. Workspace dependencies are declared once in the root `Cargo.toml`.

## Configuration

- Configuration lives in per-user application data paths (the `directories` crate or Tauri path
  APIs). **Never hard-code home directories.**
- The config holds: model provider, model ID, allowed roots, wake-word toggle, network policy,
  retention, and tool grants.
- The config file (`config.json`, ADR-0008) carries a `version`. Migrations are functions
  `vN -> vN+1`, written atomically (write a temp file, fsync, rename). A corrupt file is moved
  aside and replaced by defaults with a warning; a newer-version file is refused untouched.
- Path precedence: `--data-dir` > `LOKI_DATA_DIR` > OS default. An override puts data, `config/`
  and `logs/` under one root.
- Secrets are **not** stored in config. They go in the OS secret store: Keychain, DPAPI/Credential
  Manager, Secret Service, or Android Keystore.

## Shared-code strategy

Shared across platforms: domain types, the agent, policy, the memory schema, provider interfaces,
and most of the UI. Native code is unavoidable for permission flows, the audio lifecycle, OS tools
and distribution. Capabilities are discovered at runtime. Don't promise a fixed code-sharing percentage.
