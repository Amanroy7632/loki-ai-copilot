# P5-T03 chat UI: manual run

Run: `LOKI_DATA_DIR=$(mktemp -d) pnpm --filter desktop tauri dev` (mock provider until a model is chosen;
with `loki models use bonsai-2:4b` and Ollama running, replies come from the real model).

- [ ] Type a message, press Enter: the reply streams in word by word; Stop shows while it streams.
- [ ] Press Stop (or Escape in the message box) mid-reply: "Stopped" notice; the partial reply is kept and marked.
- [ ] Retry: the last reply is replaced by a new one.
- [ ] The new conversation appears at the top of the list and is highlighted.
- [ ] Rename (✎): Enter saves, Escape cancels; focus returns to the conversation.
- [ ] Delete (×): a confirmation appears with focus on Cancel; Delete removes it.
- [ ] Keyboard only: Tab reaches New chat, the list, and the message box; Up/Down move in the list.
- [ ] VoiceOver (Cmd+F5): the list is announced as "Conversations", new messages are read once complete.
- [ ] Quit and reopen: conversations and messages are still there.

## Last run

Not yet run interactively. On 2026-11-28 the agent only verified that the app starts (process stays up,
no errors) with all 9 commands registered; it cannot operate the window.

# P5-T04 approval dialog: manual run

Setup (real model): `LOKI_DATA_DIR=.dev-data cargo run -q -p loki-cli -- permissions grant app_launch`,
then `./scripts/dev.sh --model bonsai-2:4b`. Ask "Open the Calculator app."

- [ ] The dialog shows `app.open`, "Changes something", the `app_id` argument and a countdown from 2:00.
- [ ] Focus is on Deny; pressing Enter denies, the chat shows `app.open: cancelled`, Calculator doesn't open.
- [ ] Ask again; Tab to Approve and press Enter (or click): Calculator opens, `app.open: succeeded`.
- [ ] Ask again and leave it: at 0:00 it says "Expired"/closes, and nothing opens.
- [ ] Escape denies. Tab and Shift+Tab stay on the two buttons.
- [ ] VoiceOver: the dialog is announced with its title and risk explanation.
- [ ] `loki audit list` (same data dir) shows one row per attempt with the right outcome.

## Last run

Not yet run interactively (the agent can't operate the window). Covered by Vitest component tests
and Rust broker tests; see the P5-T04 handoff.

# P5-T05 permissions screen: manual run

Run `./scripts/dev.sh --fresh --model bonsai-2:4b` and open **Permissions**.

- [ ] Every capability is listed; Internet, Notifications and Developer tools say why they're unavailable, and Allow is disabled.
- [ ] Allow Clock; in Chat ask "What time is it in Tokyo?": it answers. Revoke Clock; ask again: the tool is denied (`time.now: denied`).
- [ ] Type `docs`, Choose folder…: the native macOS folder dialog opens; pick a folder; it appears with its path.
- [ ] Cancel the dialog once: "No folder was chosen.", nothing added.
- [ ] Allow reading `docs`; in Chat ask to find a file there: it works without restarting the app.
- [ ] Remove `docs` (confirm, focus starts on Cancel): reading is revoked; re-adding `docs` starts as "Reading not allowed".
- [ ] `loki permissions list` (same data dir) matches the screen.

## Last run

Not yet run interactively (the agent can't operate the window). The agent verified the app starts
with the dialog plugin and that the generated capability grants only Loki's own commands.
