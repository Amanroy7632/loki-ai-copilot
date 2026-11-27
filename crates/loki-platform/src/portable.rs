//! Fallback adapter for OSs without a native adapter yet (Windows and Linux until P6).
//! App launch is reported `Unavailable` rather than half-working.

use std::collections::BTreeMap;

use loki_core::{Capability, CapabilityStatus};

use crate::{AppEntry, LaunchOutcome, Platform, PlatformError, SystemInfo};

/// Portable adapter: clock, system info and file reading only.
#[derive(Debug, Default)]
pub struct PortablePlatform;

impl PortablePlatform {
    /// A new adapter.
    pub fn new() -> Self {
        Self
    }
}

const NO_APPS: &str = "app launch on this OS arrives in P6";

impl Platform for PortablePlatform {
    fn name(&self) -> &str {
        "portable"
    }

    fn capabilities(&self) -> BTreeMap<Capability, CapabilityStatus> {
        let mut capabilities = crate::later_phase_statuses();
        capabilities.insert(
            Capability::AppLaunch,
            CapabilityStatus::Unavailable {
                reason: String::from(NO_APPS),
            },
        );
        capabilities
    }

    fn list_apps(&self) -> Result<Vec<AppEntry>, PlatformError> {
        Err(PlatformError::Unavailable(String::from(NO_APPS)))
    }

    fn launch_app(&self, _app_id: &str) -> Result<LaunchOutcome, PlatformError> {
        Err(PlatformError::Unavailable(String::from(NO_APPS)))
    }

    fn system_info(&self) -> Result<SystemInfo, PlatformError> {
        crate::system::read_system_info()
    }
}
