//! `MockPlatform`: a scripted adapter for tests on every host. Never used in production.

use std::collections::BTreeMap;
use std::sync::Mutex;

use loki_core::{Capability, CapabilityStatus};

use crate::{AppEntry, LaunchOutcome, Platform, PlatformError, SystemInfo};

/// Test double with configurable apps and capabilities. Records every launch.
#[derive(Debug)]
pub struct MockPlatform {
    apps: Vec<AppEntry>,
    capabilities: BTreeMap<Capability, CapabilityStatus>,
    launches: Mutex<Vec<String>>,
    verify_launches: bool,
}

impl Default for MockPlatform {
    fn default() -> Self {
        let mut capabilities = crate::later_phase_statuses();
        capabilities.insert(Capability::AppLaunch, CapabilityStatus::Supported);
        Self {
            apps: vec![AppEntry {
                app_id: String::from("com.example.calculator"),
                display_name: String::from("Calculator"),
            }],
            capabilities,
            launches: Mutex::new(Vec::new()),
            verify_launches: true,
        }
    }
}

impl MockPlatform {
    /// Replaces the discovered apps.
    #[must_use]
    pub fn with_apps(mut self, apps: Vec<AppEntry>) -> Self {
        self.apps = apps;
        self
    }

    /// Overrides one capability's status.
    #[must_use]
    pub fn with_capability(mut self, capability: Capability, status: CapabilityStatus) -> Self {
        self.capabilities.insert(capability, status);
        self
    }

    /// Makes launches report "not verified" (the OS gave no confirmation).
    #[must_use]
    pub fn with_unverified_launches(mut self) -> Self {
        self.verify_launches = false;
        self
    }

    /// App IDs launched so far, in order.
    pub fn launches(&self) -> Vec<String> {
        self.launches.lock().map(|l| l.clone()).unwrap_or_default()
    }
}

impl Platform for MockPlatform {
    fn name(&self) -> &str {
        "mock"
    }

    fn capabilities(&self) -> BTreeMap<Capability, CapabilityStatus> {
        self.capabilities.clone()
    }

    fn list_apps(&self) -> Result<Vec<AppEntry>, PlatformError> {
        Ok(self.apps.clone())
    }

    fn launch_app(&self, app_id: &str) -> Result<LaunchOutcome, PlatformError> {
        if !self.apps.iter().any(|app| app.app_id == app_id) {
            return Err(PlatformError::UnknownApp(app_id.to_owned()));
        }
        if let Ok(mut launches) = self.launches.lock() {
            launches.push(app_id.to_owned());
        }
        Ok(LaunchOutcome {
            verified: self.verify_launches,
            already_running: false,
            detail: String::from(if self.verify_launches {
                "mock launch"
            } else {
                "mock launch, unconfirmed"
            }),
        })
    }

    fn system_info(&self) -> Result<SystemInfo, PlatformError> {
        Ok(SystemInfo {
            os: String::from("MockOS 1.0"),
            arch: String::from("test"),
            cpu: String::from("Mock CPU"),
            cpu_cores: 4,
            ram_total_bytes: 8 << 30,
            ram_available_bytes: 4 << 30,
            disk_total_bytes: Some(500 << 30),
            disk_available_bytes: Some(100 << 30),
        })
    }
}
