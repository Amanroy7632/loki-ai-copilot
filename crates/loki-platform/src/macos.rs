//! macOS adapter (reference OS, ADR-0003).
//!
//! - **Discovery:** `.app` bundles in `/Applications`, `/System/Applications`, `~/Applications`
//!   and one level of subfolders (e.g. `Utilities`). The `app_id` is the bundle identifier from
//!   `Contents/Info.plist`; the name is `CFBundleDisplayName`, `CFBundleName`, or the folder name.
//! - **Launch:** `/usr/bin/open -b <bundle id>` with an argument vector (never a shell), and only
//!   for an ID found by discovery. Launch Services handles it, so normal macOS security
//!   (Gatekeeper, TCC prompts) applies unchanged.
//! - **Verification:** the launch counts as verified only once a process whose executable lives
//!   inside the bundle is observed (polled for up to 10 s).

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use loki_core::{Capability, CapabilityStatus};

use crate::{AppEntry, LaunchOutcome, Platform, PlatformError, SystemInfo};

const VERIFY_TIMEOUT: Duration = Duration::from_secs(10);
const VERIFY_POLL: Duration = Duration::from_millis(250);

/// A discovered bundle, with the path kept private to the adapter (never shown to the model).
#[derive(Debug, Clone)]
struct Bundle {
    entry: AppEntry,
    path: PathBuf,
}

/// The macOS adapter.
#[derive(Debug, Default)]
pub struct MacPlatform;

impl MacPlatform {
    /// A new adapter.
    pub fn new() -> Self {
        Self
    }

    fn search_dirs() -> Vec<PathBuf> {
        let mut dirs = vec![
            PathBuf::from("/Applications"),
            PathBuf::from("/System/Applications"),
        ];
        if let Some(user) = directories::UserDirs::new() {
            dirs.push(user.home_dir().join("Applications"));
        }
        dirs
    }

    fn discover() -> Vec<Bundle> {
        let mut bundles = Vec::new();
        let mut seen = HashSet::new();
        for dir in Self::search_dirs() {
            for path in app_bundles(&dir, 1) {
                if let Some(entry) = read_bundle(&path)
                    && seen.insert(entry.app_id.clone())
                {
                    bundles.push(Bundle { entry, path });
                }
            }
        }
        bundles.sort_by(|a, b| {
            a.entry
                .display_name
                .to_lowercase()
                .cmp(&b.entry.display_name.to_lowercase())
        });
        bundles
    }
}

/// `.app` directories in `dir`, descending `depth` levels into non-bundle folders.
fn app_bundles(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir()
            || path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            continue;
        }
        if path.extension().is_some_and(|ext| ext == "app") {
            found.push(path);
        } else if depth > 0 {
            found.extend(app_bundles(&path, depth - 1));
        }
    }
    found
}

fn read_bundle(path: &Path) -> Option<AppEntry> {
    let info = plist::Value::from_file(path.join("Contents").join("Info.plist")).ok()?;
    let dict = info.as_dictionary()?;
    let text = |key: &str| {
        dict.get(key)
            .and_then(plist::Value::as_string)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let app_id = text("CFBundleIdentifier")?.to_owned();
    let display_name = text("CFBundleDisplayName")
        .or_else(|| text("CFBundleName"))
        .map(str::to_owned)
        .or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))?;
    Some(AppEntry {
        app_id,
        display_name,
    })
}

/// Whether a process whose executable is inside `bundle` is running.
fn bundle_running(bundle: &Path) -> bool {
    let Ok(mut system) = sysinfo::System::new() else {
        return false;
    };
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let bundle = bundle
        .canonicalize()
        .unwrap_or_else(|_| bundle.to_path_buf());
    system
        .processes()
        .values()
        .any(|process| process.exe().is_some_and(|exe| exe.starts_with(&bundle)))
}

impl Platform for MacPlatform {
    fn name(&self) -> &str {
        "macos"
    }

    fn capabilities(&self) -> BTreeMap<Capability, CapabilityStatus> {
        let mut capabilities = crate::later_phase_statuses();
        capabilities.insert(Capability::AppLaunch, CapabilityStatus::Supported);
        capabilities
    }

    fn list_apps(&self) -> Result<Vec<AppEntry>, PlatformError> {
        Ok(Self::discover()
            .into_iter()
            .map(|bundle| bundle.entry)
            .collect())
    }

    fn launch_app(&self, app_id: &str) -> Result<LaunchOutcome, PlatformError> {
        // Only IDs that discovery found right now. This also rules out anything that isn't a
        // plain bundle identifier reaching the `open` command.
        let bundle = Self::discover()
            .into_iter()
            .find(|bundle| bundle.entry.app_id == app_id)
            .ok_or_else(|| PlatformError::UnknownApp(app_id.to_owned()))?;
        let already_running = bundle_running(&bundle.path);

        let output = Command::new("/usr/bin/open")
            .arg("-b")
            .arg(&bundle.entry.app_id)
            .output()
            .map_err(|error| PlatformError::Failed(format!("could not run open: {error}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(PlatformError::Failed(format!(
                "open -b {} failed: {}",
                bundle.entry.app_id,
                stderr.trim()
            )));
        }

        let started = Instant::now();
        while started.elapsed() < VERIFY_TIMEOUT {
            if bundle_running(&bundle.path) {
                return Ok(LaunchOutcome {
                    verified: true,
                    already_running,
                    detail: if already_running {
                        format!(
                            "{} was already running and was brought to the front",
                            bundle.entry.display_name
                        )
                    } else {
                        format!("{} is running", bundle.entry.display_name)
                    },
                });
            }
            std::thread::sleep(VERIFY_POLL);
        }
        Ok(LaunchOutcome {
            verified: false,
            already_running,
            detail: format!(
                "macOS accepted the launch of {}, but its process was not seen within {} s",
                bundle.entry.display_name,
                VERIFY_TIMEOUT.as_secs()
            ),
        })
    }

    fn system_info(&self) -> Result<SystemInfo, PlatformError> {
        crate::system::read_system_info()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_system_apps_with_bundle_ids() {
        let apps = MacPlatform::new().list_apps().unwrap();
        assert!(!apps.is_empty(), "no apps discovered");
        // Calculator ships with every macOS install.
        assert!(
            apps.iter().any(|a| a.app_id == "com.apple.calculator"),
            "{:?}",
            &apps[..apps.len().min(5)]
        );
        let ids: HashSet<&str> = apps.iter().map(|a| a.app_id.as_str()).collect();
        assert_eq!(ids.len(), apps.len(), "IDs are unique");
    }

    #[test]
    fn unknown_app_is_refused_without_running_anything() {
        let error = MacPlatform::new()
            .launch_app("com.example.does-not-exist")
            .unwrap_err();
        assert_eq!(
            error,
            PlatformError::UnknownApp("com.example.does-not-exist".into())
        );
        // A crafted ID that would be dangerous in a shell is just "unknown".
        let error = MacPlatform::new()
            .launch_app("com.apple.calculator; rm -rf ~")
            .unwrap_err();
        assert!(matches!(error, PlatformError::UnknownApp(_)));
    }

    #[test]
    fn reads_a_bundle_from_a_fake_info_plist() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Fake.app").join("Contents");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(
            app.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.example.fake</string>
<key>CFBundleName</key><string>Fake App</string>
</dict></plist>"#,
        )
        .unwrap();
        let found = app_bundles(dir.path(), 1);
        assert_eq!(found.len(), 1);
        let entry = read_bundle(&found[0]).unwrap();
        assert_eq!(entry.app_id, "com.example.fake");
        assert_eq!(entry.display_name, "Fake App");
    }
}
