# P6: Remaining desktop platforms (v0.6)

| | |
| --- | --- |
| **Effort** | 3–5 weeks |
| **Depends on** | P5 gate |
| **Release** | v0.6: the remaining desktop OSs (Windows/Linux, or macOS if the reference OS was Windows) |

## Tickets

### P6-T01: Platform adapter for desktop OS #2
- App discovery and launch, system info, TTS, secret store, autostart and notifications, following PLATFORMS.md.
- **Acceptance:** Capability statuses are correct, with reasons for anything unsupported.

### P6-T02: Platform adapter for desktop OS #3
- The same scope as T01. On Linux, test **GNOME and KDE separately**, on both X11 and Wayland where relevant.

### P6-T03: Path-safety tests per OS
- Windows: junctions, UNC paths, `\\?\` paths, 8.3 names, case-insensitivity. macOS: aliases, case-insensitive APFS. Linux: bind mounts, `/proc` paths.
- **Acceptance:** SEC-02 and SEC-03 pass on every OS in CI.

### P6-T04: Native builds and installers
- MSI/NSIS for Windows, AppImage/deb for Linux, and dmg for macOS, built on native CI runners.
- **Acceptance:** An installer smoke test per OS: install, launch, quit, uninstall.

### P6-T05: Lifecycle checklist per OS
- Run `docs/checklists/desktop-lifecycle.md` on each OS.

## Phase gate

- [ ] The same core test suite passes on all desktop OSs.
- [ ] Unsupported tools are clearly disabled, with reasons, and never shown as working.
- [ ] Installer smoke tests are recorded.
