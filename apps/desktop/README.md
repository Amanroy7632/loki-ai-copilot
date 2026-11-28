# apps/desktop

The Loki desktop app: a Tauri 2 shell around a React + TypeScript (strict) + Vite UI (P5).

```sh
pnpm install                          # once, at the repo root
pnpm --filter desktop tauri dev       # run the app (Vite on localhost:1420 + the Rust shell)
pnpm --filter desktop test            # Vitest component tests (Tauri IPC mocked)
LOKI_DATA_DIR=/tmp/loki-dev pnpm --filter desktop tauri dev   # throwaway data folder
```

Rules:
- Tauri commands are thin and delegate to `loki-runtime` (`src-tauri/src/commands.rs`).
- IPC uses only the generated types from `packages/contracts`.
- Least-privilege ACL: every command is declared in `src-tauri/build.rs` and granted in
  `src-tauri/capabilities/default.json`; no core plugin permissions unless a ticket needs them.
- Strict CSP in `tauri.conf.json` (a separate `devCsp` only adds Vite's hot-reload socket).
- The app identifier `dev.loki.placeholder` is temporary (decision D-08).
- Icons are generated from `src-tauri/app-icon.svg` with `pnpm --filter desktop tauri icon src-tauri/app-icon.svg` (placeholder artwork).
