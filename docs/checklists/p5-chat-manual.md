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
