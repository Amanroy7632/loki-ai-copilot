# ADR-0016: Desktop lifecycle: tray, single instance, quit drains work, opt-in autostart

- **Status:** Accepted
- **Date:** 2026-11-28
- **Ticket:** P5-T07
- **Deciders:** started by a Codex session, finished and reviewed by the implementation agent

## Context

P5-T07 asks for a tray menu (show/hide, mute, emergency stop, Quit), close-to-hide, Quit that
cancels work and releases the mic, a single-instance lock and an opt-in autostart toggle.
Two processes on one SQLite database, or a login item pointing at a development build, would be
real harm. A Quit that kills a turn mid-write could lose the partial reply or leave a task
unfinished.

## Decision

- Tauri's `tray-icon` feature, **tauri-plugin-single-instance 2.5.5** (registered first, so a
  second process never opens the runtime; it focuses the first and exits) and
  **tauri-plugin-autostart 2.7.0** (launch argument `--autostart`, which keeps the window hidden).
  No plugin JS permission is granted to the webview; autostart is reached only through Loki's
  `get_autostart` / `set_autostart` commands.
- **Autostart is off until the user turns it on.** It is unavailable in development builds and
  with `LOKI_DATA_DIR` / `LOKI_PROVIDER` overrides. The status is the OS's actual registration,
  verified after every change; unknown is shown as unknown, never as "off".
- **Lifecycle control lives in loki-runtime** (`LifecycleControl`) and works while a turn holds
  the runtime lock. Emergency stop cancels running turns and pending approvals. Quit (tray, Cmd+Q,
  or the OS) refuses new turns, cancels running work and waits up to 2 s for partial replies and
  audit rows to be stored before exiting; anything unfinished is marked interrupted at startup
  (P3-T04), never replayed. Closing the window only hides it.
- The tray's Mute item is shown disabled until voice ships (P5-T08), never as working.

## Consequences

- Settings no longer carries an `autostart` status (one source of truth: `AutostartStatus`).
- The Linux tray adds the ISC-licensed `libloading` (exception pending D-07).
- The acceptance is the manual `docs/checklists/desktop-lifecycle.md` run on the reference OS;
  automated tests cover the runtime side (stop, quit, approval during quit, autostart states).
