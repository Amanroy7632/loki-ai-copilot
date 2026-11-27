# P5: Desktop app (v0.5 MVP)

| | |
| --- | --- |
| **Effort** | 3–4 weeks |
| **Depends on** | P4 gate |
| **Release** | v0.5: installable desktop MVP on the reference OS |
| **Creates** | `apps/desktop` (Tauri 2 + React + TS + Vite), `packages/ui` |

## Rules for this phase

- Tauri commands are **thin**: deserialize, call `loki-runtime`, serialize. Business logic in a command is a review blocker.
- Use only the generated types from `packages/contracts`.
- Use Tauri's capability/permission (ACL) config with least privilege, and a strict CSP. No remote content in the webview.

## Tickets

### P5-T01: Scaffold the app
- Create `apps/desktop` (Tauri 2, React, TS strict, Vite) and `packages/ui`, with ESLint, Prettier and Vitest, and pnpm scripts. Add the JS jobs to CI.
- **Acceptance:** `pnpm --filter desktop tauri dev` runs, and the JS lint and tests pass in CI.

### P5-T02: Runtime API and event streaming
- Expose the runtime API as Tauri commands, and stream tokens and tool/voice events via Tauri channels or events, with typed payloads.
- **Acceptance:** A UI test with the mock provider receives a streamed reply.

### P5-T03: Chat UI (FR-01)
- Streaming messages, stop, retry, a conversation list, new chat, rename and delete. Keep it accessible: keyboard navigation, focus management, and screen-reader labels.
- **Acceptance:** Vitest component tests, plus a manual run.

### P5-T04: Approval dialog (FR-04)
- Show the tool, a plain-language risk explanation, **every argument** and the expiry countdown. Default focus is on "Deny". Enter doesn't approve a Sensitive action without explicit focus.
- **Acceptance:** Tests check that approving binds to the shown arguments, and that an expired approval is refused.

### P5-T05: Permissions and capabilities screen (FR-09)
- Show each capability's status and reason, the grant and revoke actions, and an allowed-roots picker using the native folder dialog. Unsupported capabilities are shown as unavailable, with the reason.
- **Acceptance:** Revoking a grant immediately denies the next call (test).

### P5-T06: Settings and data control (FR-09)
- Model and provider, network mode, retention, wake word (if it shipped), and start at login. Actions: export memory and history (JSON), delete memory, clear history (which also clears FTS), and an audit view.
- **Acceptance:** Export then delete then verify: the DB and FTS hold no deleted content (test).

### P5-T07: Tray and background (FR-06)
- Tray or menu-bar menu: show/hide, mute, emergency stop and Quit. Closing the window hides it; Quit cancels work, releases the mic and exits. Add a single-instance lock and an opt-in autostart toggle (Tauri plugins).
- **Acceptance:** Run the manual checklist `docs/checklists/desktop-lifecycle.md`.

### P5-T08: Voice UI
- A push-to-talk button and shortcut, an always-visible listening indicator while the mic is open, a state display, and mute.
- **Acceptance:** The indicator is visible in every mic-open state (test against the state machine).

### P5-T09: Security configuration
- Tauri ACL (only the needed commands), CSP, no `dangerousRemoteDomainIpcAccess`-style exceptions, and devtools disabled in release.
- **Acceptance:** Config reviewed against the Tauri security docs, with notes in the handoff.

### P5-T10: Lifecycle verification
- Close and reopen, quit, reboot (autostart on and off), sleep and resume during idle, and during generation and recording.
- **Acceptance:** All checklist items pass on the reference OS, with the results recorded.

## Phase gate

- [ ] A signed (or locally trusted) installer installs and runs on the reference OS.
- [ ] Close, reopen, quit, reboot and sleep tests pass (checklist recorded).
- [ ] FR-01, FR-04 (UI), FR-06 and FR-09 are demonstrated.

**Only after this gate may mobile work (P8/P9) begin.**
