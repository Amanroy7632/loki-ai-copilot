//! OS autostart orchestration. The desktop supplies the Tauri adapter; tests supply a mock.

use std::sync::{Arc, Mutex};

use loki_core::{AutostartStatus, CapabilityStatus};

/// Narrow OS boundary. Implementations must not enable registration just by being constructed.
pub trait AutostartBackend: Send + Sync + 'static {
    /// Reads actual OS registration.
    fn is_enabled(&self) -> Result<bool, String>;
    /// Changes registration only in response to the user's explicit toggle.
    fn set_enabled(&self, enabled: bool) -> Result<(), String>;
}

/// Serializes OS reads/writes on a blocking worker and verifies the requested state afterwards.
#[derive(Clone)]
pub struct AutostartService {
    backend: Arc<Mutex<Box<dyn AutostartBackend>>>,
    unavailable: Option<String>,
}

impl AutostartService {
    /// Creates an inert service. A reason disables all OS calls (e.g. development builds).
    pub fn new(backend: impl AutostartBackend, unavailable: Option<String>) -> Self {
        Self {
            backend: Arc::new(Mutex::new(Box::new(backend))),
            unavailable,
        }
    }

    /// Current OS state and any reason it cannot be read. Errors never masquerade as "off".
    pub async fn status(&self) -> AutostartStatus {
        if let Some(reason) = &self.unavailable {
            return AutostartStatus {
                enabled: None,
                availability: CapabilityStatus::Unavailable {
                    reason: reason.clone(),
                },
            };
        }
        match self.access(None).await {
            Ok(enabled) => AutostartStatus {
                enabled: Some(enabled),
                availability: CapabilityStatus::Supported,
            },
            Err(reason) => AutostartStatus {
                enabled: None,
                availability: CapabilityStatus::Denied { reason },
            },
        }
    }

    /// Applies an opt-in/out action and returns the OS-verified state. Failure is visible.
    pub async fn set_enabled(&self, enabled: bool) -> Result<AutostartStatus, String> {
        if let Some(reason) = &self.unavailable {
            return Err(reason.clone());
        }
        let actual = self.access(Some(enabled)).await?;
        Ok(AutostartStatus {
            enabled: Some(actual),
            availability: CapabilityStatus::Supported,
        })
    }

    async fn access(&self, desired: Option<bool>) -> Result<bool, String> {
        let backend = Arc::clone(&self.backend);
        tokio::task::spawn_blocking(move || {
            let backend = backend
                .lock()
                .map_err(|_| "autostart state is unavailable".to_owned())?;
            if let Some(desired) = desired {
                backend.set_enabled(desired)?;
            }
            let actual = backend.is_enabled()?;
            if desired.is_some_and(|desired| actual != desired) {
                return Err("the OS did not apply the requested start-at-login setting".into());
            }
            Ok(actual)
        })
        .await
        .map_err(|error| error.to_string())?
    }
}
