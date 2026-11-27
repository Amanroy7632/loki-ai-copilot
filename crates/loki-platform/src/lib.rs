//! # loki-platform
//!
//! OS abstraction: capability discovery and native adapters behind cfg gates.
//!
//! **Status:** stub. Nothing is implemented yet. Tickets that fill this crate in:
//! P3-T06..T07 (reference OS), P6 (other desktops), P8 (Android), P9 (iOS).
//!
//! ## Responsibilities
//!
//! - Define the `Platform` trait: `capabilities()`, `list_apps()`, `launch_app(id)`, `system_info()`, notifications, TTS, secret store, autostart.
//! - Provide one module per OS (`macos`, `windows`, `linux`, `android`, `ios`), each `#[cfg(target_os = ...)]`.
//! - Provide `MockPlatform` for tests on every host.
//! - Report `CapabilityStatus` with a human-readable reason for anything not supported.
//!
//! ## Must not
//!
//! - Bypass OS permission systems (UAC, TCC, portals, Android/iOS rules).
//! - Rely on screen coordinates when semantic accessibility APIs exist.
//! - Use `unsafe` without a module-level `#![allow(unsafe_code)]` and a `// SAFETY:` comment per block.
//!
//! ## Allowed internal dependencies
//!
//! `loki-core`. Adding another edge needs an ADR (see `docs/ARCHITECTURE.md`).
