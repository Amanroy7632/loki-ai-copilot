# ADR-0014: Allowed folders are picked by a Rust-side native dialog

- **Status:** Accepted
- **Date:** 2026-11-28
- **Ticket:** P5-T05
- **Deciders:** implementation agent (owner asked to proceed to the next ticket)

## Context

P5-T05 needs an allowed-roots picker using the native folder dialog. The usual Tauri pattern
grants the webview `dialog:allow-open`; the page opens the dialog and passes the chosen path to
an "add root" command. That command would accept any path from the page, so a compromised page
(XSS despite the CSP) could add `/` or `~/.ssh` as a root without the user picking it.

## Decision

- Add **tauri-plugin-dialog 2.8.4** (Apache-2.0 OR MIT; 3.x is alpha, like Tauri 3) with default
  features (`gtk3`, so Linux builds have a backend). Its build dependency **tauri-plugin** is
  locked to **2.7.2**, because 2.7.3 and 2.7.4 are less than two weeks old.
- The webview gets **no `dialog:` permission**. The only way to add a folder is the Loki command
  `add_root(id)`: it checks the ID first (`Runtime::validate_root_id`), opens the native picker
  from Rust, and passes the path the OS returned to `Runtime::add_root`. No command accepts a
  path from the page.
- Grants follow the folder the user saw. Removing a root, or pointing its ID at a different
  folder, revokes every grant with that scope (previously the grant stayed and would cover a new
  folder added under the same ID). Root changes rebuild the tool host, so a running app sees them.

## Consequences

- The plugin also pulls in `tauri-plugin-fs` 2.7.1 and `rfd` 0.17.2 as libraries. The fs plugin
  is not registered, and the dialog plugin's own JS commands stay denied by the ACL.
- Adding a root takes the runtime write lock, so it waits for a running chat turn to finish.
- Mobile (P8/P9) will need its own picker path; the plugin supports it, but the flow is untested.
