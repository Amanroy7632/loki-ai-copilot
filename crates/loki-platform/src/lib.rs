//! # loki-platform
//!
//! OS abstraction: capability discovery and native adapters behind cfg gates.
//!
//! **Status:** P3-T07: the [`Platform`] trait, [`MockPlatform`], the macOS adapter (reference OS,
//! ADR-0003) and a portable fallback for other OSs (no app launch until P6).
//!
//! ## Responsibilities
//!
//! - Define the `Platform` trait: `capabilities()`, `list_apps()`, `launch_app(id)`, `system_info()`.
//! - Provide one module per OS, each `#[cfg(target_os = ...)]`.
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

use std::collections::BTreeMap;
use std::sync::Arc;

use loki_core::{Capability, CapabilityStatus};
use serde::Serialize;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
mod macos_mic;
pub mod mock;
pub mod portable;
pub mod system;

pub use mock::MockPlatform;

/// One launchable application, as discovered on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppEntry {
    /// Stable ID the model must use (macOS: the bundle identifier, e.g. `com.apple.calculator`).
    pub app_id: String,
    /// Name shown to the user and the model.
    pub display_name: String,
}

/// What happened when launching an app. "Verified" means the OS confirmed it (the process is
/// running), not merely that the launch command returned success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LaunchOutcome {
    /// The app's process was observed running after the launch.
    pub verified: bool,
    /// It was already running before (the launch only brought it to the front).
    pub already_running: bool,
    /// Human-readable detail.
    pub detail: String,
}

/// OS, CPU, memory and disk summary. Deliberately **no** hostname, username or serial numbers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemInfo {
    /// OS name and version, e.g. `macOS 27.0.1`.
    pub os: String,
    /// CPU architecture, e.g. `aarch64`.
    pub arch: String,
    /// CPU brand string.
    pub cpu: String,
    /// Logical CPU count.
    pub cpu_cores: usize,
    /// Total RAM, bytes.
    pub ram_total_bytes: u64,
    /// Available RAM, bytes.
    pub ram_available_bytes: u64,
    /// Total size of the system disk, bytes, if known.
    pub disk_total_bytes: Option<u64>,
    /// Free space on the system disk, bytes, if known.
    pub disk_available_bytes: Option<u64>,
}

/// Why a platform call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlatformError {
    /// The capability isn't supported here.
    #[error("unavailable: {0}")]
    Unavailable(String),
    /// The OS refused (e.g. a privacy permission is missing).
    #[error("permission required: {0}")]
    PermissionRequired(String),
    /// No discovered app has this ID.
    #[error("unknown app_id '{0}'")]
    UnknownApp(String),
    /// The OS call failed.
    #[error("{0}")]
    Failed(String),
}

/// The OS adapter. Methods are **blocking**; async callers wrap them in `spawn_blocking`.
pub trait Platform: Send + Sync {
    /// Short name for logs, e.g. `macos`, `mock`.
    fn name(&self) -> &str;

    /// Status of every capability. Anything missing must be treated as unavailable.
    fn capabilities(&self) -> BTreeMap<Capability, CapabilityStatus>;

    /// Discovers launchable apps.
    fn list_apps(&self) -> Result<Vec<AppEntry>, PlatformError>;

    /// Launches a **discovered** app and verifies it started. Unknown IDs are refused.
    fn launch_app(&self, app_id: &str) -> Result<LaunchOutcome, PlatformError>;

    /// OS, CPU, RAM and disk summary.
    fn system_info(&self) -> Result<SystemInfo, PlatformError>;

    /// Asks the OS for the permission behind `capability`, showing the system's own prompt if
    /// the user was never asked, and returns the status afterwards. Only called when the user
    /// turns the capability on. It never works around a refusal: a denied permission stays
    /// denied until the user changes it in the OS settings. Default: no OS permission is
    /// involved, so this is the current status.
    fn request_permission(&self, capability: Capability) -> CapabilityStatus {
        self.capabilities()
            .get(&capability)
            .cloned()
            .unwrap_or(CapabilityStatus::Unavailable {
                reason: String::from("not reported by this platform"),
            })
    }

    /// Opens the OS settings page where the user controls this capability's permission.
    fn open_permission_settings(&self, capability: Capability) -> Result<(), PlatformError> {
        Err(PlatformError::Unavailable(format!(
            "no OS settings page for {} here",
            capability.as_str()
        )))
    }
}

/// The adapter for the OS this binary was built for.
pub fn native() -> Arc<dyn Platform> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(macos::MacPlatform::new())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Arc::new(portable::PortablePlatform::new())
    }
}

/// Statuses shared by every adapter in P3: features that arrive in later phases.
pub(crate) fn later_phase_statuses() -> BTreeMap<Capability, CapabilityStatus> {
    let unavailable = |reason: &str| CapabilityStatus::Unavailable {
        reason: reason.to_owned(),
    };
    BTreeMap::from([
        (Capability::Clock, CapabilityStatus::Supported),
        (Capability::SystemInfo, CapabilityStatus::Supported),
        (Capability::FileRead, CapabilityStatus::Supported),
        (
            Capability::Network,
            unavailable("internet tools arrive in P7"),
        ),
        (
            Capability::Microphone,
            unavailable("microphone permission handling for this OS arrives in P6"),
        ),
        (
            Capability::Notifications,
            unavailable("notifications arrive in P5"),
        ),
        (
            Capability::DeveloperTools,
            unavailable("developer tools arrive in P7 or later"),
        ),
    ])
}
