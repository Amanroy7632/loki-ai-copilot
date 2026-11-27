# Platforms: capabilities, native adapters and distribution

> P0-T08 must verify every row against current official documentation and record minimum OS versions.
> Until then, this table is the **design intent**, not verified fact.

## Capability matrix

| Platform | Supported design and limitations |
| --- | --- |
| **macOS** | Tray/menu bar, login item, local inference, microphone, app launch, Accessibility and Apple Events with consent. Screen capture requires permission. No listening during normal sleep. |
| **Windows** | Tray, per-user startup, local inference, UI Automation, app launch, microphone. Elevated apps and the secure desktop are off limits; **don't bypass UAC**. |
| **Linux** | Tray where the desktop supports it, XDG autostart or a systemd user service, D-Bus, portals. Wayland restricts global input and capture, so use compositor-approved APIs. Test GNOME and KDE separately. |
| **Android** | Foreground chat/voice, intents, share targets, scoped storage. A user-started microphone foreground service only where the OS and Play policies allow it, with a persistent notification. No unrestricted cross-app control. |
| **iOS / iPadOS** | Foreground chat/voice, App Intents, Shortcuts, share extension, document picker. **No** general background daemon, persistent wake word, or control of other apps. |

## Native adapter tasks

| Adapter | Implementation |
| --- | --- |
| macOS | NSWorkspace for app discovery and launch. Accessibility (AXUIElement) for scoped actions. Apple Events permission. SMAppService for the login item. ScreenCaptureKit only if that feature is enabled. |
| Windows | App discovery via Start-menu shortcuts and `shell:AppsFolder` (AppUserModelIDs). Launch via ShellExecute or app activation. UI Automation for accessible controls. A supported per-user startup mechanism. Native toast notifications. TTS via WinRT `SpeechSynthesizer` or SAPI. Secrets via Credential Manager or DPAPI. |
| Linux | `.desktop` entries plus `gio`/`xdg-open`. D-Bus (zbus). XDG portals. Desktop-dependent tray and autostart. Secret Service for secrets. speech-dispatcher for TTS. |
| Android | Package-visibility declarations, intents, Storage Access Framework, runtime mic permission, a foreground service only for permitted use cases, Keystore. |
| iOS | App Intents, Shortcuts, URL schemes, document picker, share extension, AVAudioSession lifecycle, Keychain. No app enumeration or UI automation. |

## Capability contract

Every adapter returns a `CapabilityStatus` for each capability: `supported`,
`permission_required`, `denied` or `unavailable`, each with a reason. The UI uses this to enable,
explain or hide actions. Every adapter has a **mock** for tests. Avoid screen coordinates; use
semantic accessibility controls.

## Background behaviour

- **Desktop:** startup is opt-in. Tray status, a single-instance lock, sleep/wake recovery, and an explicit Quit.
- **Mobile:** foreground first; only OS-managed background tasks. Persist task state, and never
  assume execution continues uninterrupted.

## Distribution

| Target | Build and release |
| --- | --- |
| macOS | Apple Silicon first; Intel only if tested. `.app` / `.dmg`. Developer ID signing and notarization for public distribution. |
| Windows | x64 first; ARM64 later if supported. MSI or NSIS installer. Code signing recommended (SmartScreen). |
| Linux | AppImage and/or `.deb`. Test the target distributions and desktop environments. |
| Android | APK for testing; signed AAB for the store. Review the foreground-service policy. |
| iOS | Built with Xcode on macOS. Provisioning, TestFlight, App Store. Review entitlements and background rules. |

- CI uses **native runners** for each OS. Don't assume one host can build every target.
- Keep model weights **out of the installer** where practical; download them during onboarding.
- Use signed update manifests, staged rollouts and rollback.
- Document uninstall, data export and model cleanup per platform.
