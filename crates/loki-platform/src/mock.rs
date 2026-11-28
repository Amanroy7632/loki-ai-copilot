//! `MockPlatform`: a scripted adapter for tests on every host. Never used in production.

use std::collections::BTreeMap;
use std::sync::Mutex;

use loki_core::{Capability, CapabilityStatus};

use crate::{AppEntry, LaunchOutcome, Platform, PlatformError, SystemInfo};

/// Test double with configurable apps and capabilities. Records every launch.
#[derive(Debug)]
pub struct MockPlatform {
    apps: Vec<AppEntry>,
    capabilities: Mutex<BTreeMap<Capability, CapabilityStatus>>,
    /// What the "OS prompt" answers per capability (absent: the status doesn't change).
    permission_answers: BTreeMap<Capability, CapabilityStatus>,
    permission_requests: Mutex<Vec<Capability>>,
    opened_settings: Mutex<Vec<Capability>>,
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
            capabilities: Mutex::new(capabilities),
            permission_answers: BTreeMap::new(),
            permission_requests: Mutex::new(Vec::new()),
            opened_settings: Mutex::new(Vec::new()),
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
        if let Ok(capabilities) = self.capabilities.get_mut() {
            capabilities.insert(capability, status);
        }
        self
    }

    /// Scripts the answer to the OS permission prompt for `capability` (e.g. the user clicks
    /// Allow: `Supported`; Don't Allow: `Denied`). After the prompt, the status is that answer.
    #[must_use]
    pub fn with_permission_answer(
        mut self,
        capability: Capability,
        answer: CapabilityStatus,
    ) -> Self {
        self.permission_answers.insert(capability, answer);
        self
    }

    /// Capabilities whose OS prompt was requested, in order.
    pub fn permission_requests(&self) -> Vec<Capability> {
        self.permission_requests
            .lock()
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    /// Capabilities whose OS settings page was opened, in order.
    pub fn opened_settings(&self) -> Vec<Capability> {
        self.opened_settings
            .lock()
            .map(|r| r.clone())
            .unwrap_or_default()
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
        self.capabilities
            .lock()
            .map(|c| c.clone())
            .unwrap_or_default()
    }

    fn request_permission(&self, capability: Capability) -> CapabilityStatus {
        if let Ok(mut requests) = self.permission_requests.lock() {
            requests.push(capability);
        }
        let Ok(mut capabilities) = self.capabilities.lock() else {
            return CapabilityStatus::Unavailable {
                reason: String::from("mock state unavailable"),
            };
        };
        let asked = matches!(
            capabilities.get(&capability),
            Some(CapabilityStatus::PermissionRequired { .. })
        );
        if asked && let Some(answer) = self.permission_answers.get(&capability) {
            capabilities.insert(capability, answer.clone());
        }
        capabilities
            .get(&capability)
            .cloned()
            .unwrap_or(CapabilityStatus::Unavailable {
                reason: String::from("not reported by this platform"),
            })
    }

    fn open_permission_settings(&self, capability: Capability) -> Result<(), PlatformError> {
        if let Ok(mut opened) = self.opened_settings.lock() {
            opened.push(capability);
        }
        Ok(())
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
