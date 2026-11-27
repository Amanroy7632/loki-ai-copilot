//! Model onboarding (P2-T05): detect the machine, recommend from the reviewed allowlist.
//!
//! The allowlist is `fixtures/models.json`, embedded at build time so it can't be swapped at
//! runtime. A model is recommended only if `tool_calls_verified` is true (VOICE_AND_MODELS), which
//! requires a recorded measurement (ADR-0009). Downloads go through Ollama's `/api/pull`, and
//! Ollama verifies every layer's sha256 digest itself; Loki makes no direct downloads, so it has no
//! checksum of its own to add.

use serde::Deserialize;

/// The reviewed allowlist, embedded.
const ALLOWLIST_JSON: &str = include_str!("../../../fixtures/models.json");

const GIB: u64 = 1024 * 1024 * 1024;

/// One allowlisted model.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ModelEntry {
    /// Ollama tag, e.g. `qwen3:4b`.
    pub id: String,
    /// Human-readable name.
    pub display_name: String,
    /// SPDX license identifier, shown before any download.
    pub license: String,
    /// Where to read the license.
    pub license_url: String,
    /// Approximate download size.
    pub download_bytes: u64,
    /// Measured memory while loaded, if measured.
    pub loaded_bytes: Option<u64>,
    /// Smallest machine RAM to recommend it for.
    pub min_ram_gb: u64,
    /// Tool calling measured as reliable (spike or benchmark on record).
    pub tool_calls_verified: bool,
    /// Whether to run it with Ollama `think: true`. Some models (qwen3 on Ollama 0.40.18) leak
    /// their reasoning into the reply when thinking is turned off; with it on, Ollama returns the
    /// reasoning separately and Loki discards it.
    pub think: bool,
    /// Pointer to the measurement.
    pub evidence: Option<String>,
    /// Caveats.
    pub notes: String,
}

#[derive(Debug, Deserialize)]
struct Allowlist {
    models: Vec<ModelEntry>,
}

/// The embedded allowlist, in preference order (first = preferred).
pub fn allowlist() -> Vec<ModelEntry> {
    // The file is checked by `allowlist_parses_and_is_well_formed`, so failure here is a build bug.
    serde_json::from_str::<Allowlist>(ALLOWLIST_JSON)
        .map(|list| list.models)
        .unwrap_or_default()
}

/// Looks up an allowlisted model by its exact tag.
pub fn find(id: &str) -> Option<ModelEntry> {
    allowlist().into_iter().find(|entry| entry.id == id)
}

/// What onboarding knows about this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineProfile {
    /// CPU architecture, e.g. `aarch64`.
    pub arch: String,
    /// OS name and version.
    pub os: String,
    /// Total RAM in bytes.
    pub total_ram: u64,
    /// RAM currently available in bytes.
    pub available_ram: u64,
    /// Free space on the disk holding the data dir, if known.
    pub free_disk: Option<u64>,
    /// Best-effort GPU description. `sysinfo` doesn't report GPUs, so this is inferred.
    pub gpu: String,
}

impl MachineProfile {
    /// Total RAM in whole GiB, rounded to the nearest (an "8 GB" Mac reports 8).
    pub fn ram_gb(&self) -> u64 {
        (self.total_ram + GIB / 2) / GIB
    }
}

/// Detects the machine. `disk_path` should be the data dir (models live with Ollama, usually on
/// the same disk).
///
/// Detection failures don't abort: unknown RAM is reported as 0 (so nothing is recommended)
/// and unknown disk space as `None`.
pub fn detect(disk_path: &std::path::Path) -> MachineProfile {
    let (total_ram, available_ram) = sysinfo::System::new()
        .map(|mut system| {
            system.refresh_memory();
            (system.total_memory(), system.available_memory())
        })
        .unwrap_or((0, 0));
    let free_disk = sysinfo::Disks::new_with_refreshed_list()
        .ok()
        .and_then(|disks| {
            disks
                .iter()
                .filter(|disk| disk_path.starts_with(disk.mount_point()))
                .max_by_key(|disk| disk.mount_point().as_os_str().len())
                .map(sysinfo::Disk::available_space)
        });
    let arch = std::env::consts::ARCH.to_owned();
    let gpu = if cfg!(target_os = "macos") && arch == "aarch64" {
        String::from("Apple Silicon GPU (Metal, unified memory)")
    } else {
        String::from("unknown (not detected; Ollama reports what it uses)")
    };
    MachineProfile {
        os: sysinfo::System::long_os_version().unwrap_or_else(|_| std::env::consts::OS.to_owned()),
        arch,
        total_ram,
        available_ram,
        free_disk,
        gpu,
    }
}

/// The result of [`recommend`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recommendation {
    /// The model to suggest, if any qualifies.
    pub primary: Option<ModelEntry>,
    /// Other verified models that also fit.
    pub alternatives: Vec<ModelEntry>,
    /// Allowlisted models that would fit by RAM but aren't verified yet (shown, never recommended).
    pub unverified: Vec<ModelEntry>,
    /// Why (or why nothing) was recommended.
    pub reason: String,
}

/// Picks models for `profile` from `allowlist` (in preference order): verified, fits in RAM, and
/// fits on disk with 20% headroom when free disk is known.
pub fn recommend(profile: &MachineProfile, allowlist: &[ModelEntry]) -> Recommendation {
    let ram_gb = profile.ram_gb();
    let fits_disk = |entry: &ModelEntry| {
        profile
            .free_disk
            .is_none_or(|free| entry.download_bytes.saturating_mul(6) / 5 <= free)
    };
    let fitting: Vec<&ModelEntry> = allowlist
        .iter()
        .filter(|entry| entry.min_ram_gb <= ram_gb)
        .collect();
    let mut verified = fitting
        .iter()
        .filter(|entry| entry.tool_calls_verified && fits_disk(entry))
        .map(|entry| (*entry).clone());
    let primary = verified.next();
    let alternatives: Vec<ModelEntry> = verified.collect();
    let unverified = fitting
        .iter()
        .filter(|entry| !entry.tool_calls_verified)
        .map(|entry| (*entry).clone())
        .collect();
    let reason = match &primary {
        Some(entry) => format!(
            "{} GB RAM: {} is the preferred verified model that fits (needs {} GB).",
            ram_gb, entry.id, entry.min_ram_gb
        ),
        None if fitting.iter().any(|e| e.tool_calls_verified) => String::from(
            "A verified model fits in RAM, but there isn't enough free disk space for it.",
        ),
        None => format!(
            "No verified model fits in {ram_gb} GB of RAM yet. Loki only recommends models whose tool calling was measured."
        ),
    };
    Recommendation {
        primary,
        alternatives,
        unverified,
        reason,
    }
}

/// `true` for Ollama cloud (remote) model tags such as `gpt-oss:20b-cloud` or `glm-5.3:cloud`.
/// Loki never sends prompts to them (no cloud inference), whatever the server allows.
pub fn is_cloud_model(id: &str) -> bool {
    let tag = id.rsplit_once(':').map_or("", |(_, tag)| tag);
    tag == "cloud" || tag.ends_with("-cloud")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(ram_gb: u64, free_disk_gb: Option<u64>) -> MachineProfile {
        MachineProfile {
            arch: "aarch64".into(),
            os: "test".into(),
            total_ram: ram_gb * GIB,
            available_ram: ram_gb * GIB / 2,
            free_disk: free_disk_gb.map(|gb| gb * GIB),
            gpu: "test".into(),
        }
    }

    #[test]
    fn allowlist_parses_and_is_well_formed() {
        let list = allowlist();
        assert!(!list.is_empty());
        for entry in &list {
            assert!(
                !entry.license.is_empty() && entry.license_url.starts_with("https://"),
                "{}",
                entry.id
            );
            assert!(!is_cloud_model(&entry.id), "{} is a cloud model", entry.id);
            if entry.tool_calls_verified {
                assert!(
                    entry.evidence.is_some(),
                    "{} is verified without evidence",
                    entry.id
                );
            }
        }
    }

    #[test]
    fn recommends_by_ram_tier() {
        let list = allowlist();
        let pick = |ram| {
            recommend(&profile(ram, Some(100)), &list)
                .primary
                .map(|e| e.id)
        };
        assert_eq!(pick(4), None);
        assert_eq!(pick(6).as_deref(), Some("bonsai-2:4b"));
        assert_eq!(pick(8).as_deref(), Some("bonsai-2:4b"));
        assert_eq!(pick(16).as_deref(), Some("bonsai-2:4b"));
        assert_eq!(pick(32).as_deref(), Some("bonsai-2:4b"));
    }

    #[test]
    fn unverified_models_are_listed_but_never_recommended() {
        let rec = recommend(&profile(16, Some(100)), &allowlist());
        assert!(rec.unverified.iter().any(|e| e.id == "qwen3:8b"));
        assert!(
            rec.primary
                .iter()
                .chain(&rec.alternatives)
                .all(|e| e.tool_calls_verified)
        );
    }

    #[test]
    fn eight_gb_offers_qwen3_as_the_alternative_with_thinking_on() {
        let rec = recommend(&profile(8, Some(100)), &allowlist());
        let ids: Vec<&str> = rec.alternatives.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["qwen3:4b"]);
        assert!(rec.alternatives[0].think, "qwen3 must run with thinking on");
        assert!(!rec.primary.unwrap().think);
    }

    #[test]
    fn not_enough_disk_means_no_recommendation() {
        let rec = recommend(&profile(8, Some(1)), &allowlist());
        assert!(rec.primary.is_none());
        assert!(rec.reason.contains("disk"));
    }

    #[test]
    fn cloud_tags_are_recognised() {
        assert!(is_cloud_model("gpt-oss:20b-cloud"));
        assert!(is_cloud_model("glm-5.3:cloud"));
        assert!(!is_cloud_model("qwen3:4b"));
        assert!(!is_cloud_model("cloudy-model:4b"));
    }

    #[test]
    fn ram_rounds_to_marketing_size() {
        let mut p = profile(8, None);
        p.total_ram = 8_589_934_592 - 300 * 1024 * 1024; // what an 8 GB Mac may report
        assert_eq!(p.ram_gb(), 8);
    }
}
