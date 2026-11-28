# P5-T06 settings and data controls — native checks

Use an isolated throwaway data directory and the mock provider. Never clear real history to run
this checklist. Automated coverage is in `SettingsView.test.tsx`, `loki-runtime/tests/settings.rs`
and `loki-memory/tests/data_control.rs`.

**2026-11-28:** native interactive checks below **not run**. Browser discovery returned no
available browser, and no native-window automation was used. Unit/component tests do not prove
the macOS save dialog works interactively.

- [ ] Start `pnpm dev:mock`; open Settings. Model, provider, mode and retention match runtime.
- [ ] Enter an installed local model and choose Ollama. Save; header updates, the next reply uses
  that model. A missing model/server shows its health error; no cloud fallback/download occurs.
- [ ] Start a reply, open Settings, try Save/Clear. Both report busy; return to Chat and Stop.
  If an approval is pending it remains answerable. Retry the settings action after the turn ends.
- [ ] Change finite retention; confirm defaults to Cancel. Enter/Escape cancel without deleting.
- [ ] Export: cancel the native dialog; no success is claimed. Choose a new `.json` path; inspect
  conversation titles/messages and `memory_available: false`. Choose that path again; Loki refuses
  replacement and the existing bytes are unchanged.
- [ ] Clear history: Cancel first; history remains. Confirm deletion; return to Chat and verify
  titles/messages are gone. Reopen Loki and verify no history. Audit and grants remain.
- [ ] Test saving with a destination that is not writable: show an error, no success notice.
- [ ] Navigate all enabled controls by keyboard; test narrow window size and screen-reader labels.
- [ ] After P5-T07: enable/disable start at login against the OS, including reboot.
- [ ] After P7: export personal memory, delete it, query FTS for the deleted text, and verify no
  derived data remains. Extend the data-control test that currently asserts the absence of FTS.
