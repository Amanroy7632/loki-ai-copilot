# apps/desktop

This folder will hold the Tauri 2 desktop shell (React + TypeScript + Vite). It is **created in P5-T01**; see [docs/phases/P05-desktop-app.md](../../docs/phases/P05-desktop-app.md).

Rules:
- Tauri commands are thin and delegate to `loki-runtime`.
- IPC uses only the generated types from `packages/contracts`.
- Use least-privilege Tauri ACL and a strict CSP.
