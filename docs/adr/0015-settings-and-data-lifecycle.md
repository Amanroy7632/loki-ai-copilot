# ADR-0015: Settings apply to the live runtime; data control follows the installed schema

- **Status:** Accepted
- **Date:** 2026-11-28
- **Ticket:** P5-T06 (available functionality; later-ticket dependencies remain open)
- **Deciders:** implementation agent, continuing the owner's requested next phases

## Context

Claude's P5-T05 handoff selects settings/data control next. Config already stores provider, model,
network mode and retention, but retention was not enforced. The current DB has history and audit;
personal memory and its FTS index belong to P7. Start-at-login integration belongs to P5-T07.
The acceptance text mentions FTS before the phase that creates it.

## Decision

- Add new, generated settings/export/audit payloads in loki-core; existing contract shapes,
  config version, migrations and dependency graph stay intact. No new dependencies.
- Validate the model and retention, construct the candidate provider/tool host/network policy,
  save config atomically on a blocking thread, then switch the live runtime. Session overrides
  are shown as effective settings; explicitly saving persists that choice. No download or
  inference request is made by saving. Ollama health is checked separately by app status.
- Retention uses conversation `updated_at`, strictly older than the cutoff. It runs at startup
  and on settings save. There is no background retention scheduler yet; the UI says when it runs.
- Settings mutations and bulk deletion require exclusive runtime access. Desktop commands use
  `try_write`, returning a busy error immediately instead of queuing a writer behind a reply
  that might itself be waiting for a Stop or approval command to acquire a read lock.
- Export takes one SQLite read snapshot and writes versioned JSON through the runtime to a
  native-dialog destination. The webview supplies neither path nor export bytes. `create_new`
  refuses all existing files, including database/config files, even if the native dialog offers
  replacement. No intermediate exported file is created; partial new exports are removed on error.
- Bulk history deletion cascades messages, enables SQLite secure deletion, vacuums, and checks
  `wal_checkpoint(TRUNCATE)` for busy readers. Audit/grants stay separate. External exports,
  pre-migration backups, filesystem snapshots and OS backups are outside this operation.
- Personal memory, wake word and autostart are explicitly unavailable, with runtime reasons.
  Export records `memory_available: false` and an empty memory array. Do not introduce the P7
  schema just to make an empty FTS test pass. A regression assertion fails when memory/FTS tables
  are introduced, requiring their lifecycle support to be implemented then.
- Deletion and a changed finite retention period require an explicit UI confirmation, default
  Cancel. Successful changes discard mounted chat state and refresh the effective provider.
- Save/clear errors have a typed `state_changed` flag. A post-write retention error or post-delete
  cleanup failure refreshes cached chat/provider state while keeping the error visible. A busy
  or validation error never clears a running conversation. Settings are reloaded after save errors.

## Alternatives and consequences

Saving configuration for a later restart would leave the displayed and effective policy out of
sync. Giving JS filesystem access would widen IPC unnecessarily. Advancing memory/FTS and OS
autostart inside this ticket would mix work assigned to later tickets.

P5-T06 remains **partial** until P7 memory export/deletion/FTS validation and P5-T07 autostart are
connected. Native save-dialog interaction also needs a manual run. If retention cleanup fails
after config is saved, the policy remains saved and the error is shown; startup retries it.
If WAL cleanup fails after logical deletion, the error explicitly says history was deleted and
asks the user to close other instances and clear again. No guarantee is made about recovery
from SSD wear leveling or external backups.
