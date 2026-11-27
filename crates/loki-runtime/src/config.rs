//! Versioned user configuration (ADR-0008).
//!
//! - Stored as JSON in `<config_dir>/config.json`. **Never** holds secrets (they go in the OS
//!   secret store).
//! - Carries a `version`. Older files are migrated step by step (`vN -> vN+1`) on load, and the
//!   previous file is kept as `config.json.v<N>.bak`.
//! - Writes are atomic: write a temp file in the same directory, fsync, rename over the target.
//! - A file that can't be parsed is moved aside to `config.json.corrupt-<unix_ms>` and replaced
//!   by the default, with a warning. A file from a **newer** version is refused and left alone.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use loki_core::UnixMillis;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::RuntimeError;

/// The config version this build writes.
pub const CONFIG_VERSION: u64 = 3;

/// Default Ollama endpoint: IPv4 loopback literal, never `localhost` (see `net.rs`).
pub const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434";

/// Which model backend to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// The scripted `MockProvider`, for tests and demos. Replies are labelled as mock.
    Mock,
    /// A local Ollama server (P2). The default for new configs from version 2.
    Ollama,
}

/// Outbound network policy. Defined in `loki-core` (ADR-0011); same JSON form as before.
pub use loki_core::NetworkMode;

/// A directory the user allowed file tools to read. The model only ever sees `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllowedRoot {
    /// Stable opaque identifier given to the model instead of the path.
    pub id: String,
    /// Absolute path on this machine.
    pub path: PathBuf,
}

/// How long history is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retention {
    /// Delete conversations older than this many days. `None` keeps history until the user clears it.
    pub history_days: Option<u32>,
}

/// The user configuration. Unknown fields are rejected (and the file treated as corrupt), so a
/// typo can't silently change behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Schema version of this file. Always [`CONFIG_VERSION`] after loading.
    pub version: u64,
    /// The model backend.
    pub provider: ProviderKind,
    /// The model to use with the provider. `None` until onboarding picks one (P2).
    pub model_id: Option<String>,
    /// Directories file tools may read (P3).
    pub allowed_roots: Vec<AllowedRoot>,
    /// Outbound network policy.
    pub network_mode: NetworkMode,
    /// History retention.
    pub retention: Retention,
    /// Wake word enabled (P4). Off by default.
    pub wake_word: bool,
    /// Ollama connection (version 2).
    pub ollama: OllamaConfig,
    /// Context window sizing (version 2).
    pub context: ContextConfig,
    /// Tool limits (version 3).
    pub tools: ToolsConfig,
}

/// Tool loop limits (P3). Grants are **not** here: they live in the `permissions` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolsConfig {
    /// Offer tools to the model at all.
    pub enabled: bool,
    /// Tool calls per turn (hard maximum 20).
    pub max_calls_per_turn: u32,
    /// Deadline for one call, seconds.
    pub timeout_secs: u32,
    /// Tools running at the same time.
    pub concurrency: u32,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_calls_per_turn: 6,
            timeout_secs: 30,
            concurrency: 2,
        }
    }
}

/// How to reach the local Ollama server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OllamaConfig {
    /// Base URL. In `local_only` mode it must be a loopback IP literal (checked in `net.rs`).
    pub base_url: String,
}

/// Context window sizing, passed to the provider and used by the agent's context budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextConfig {
    /// Context window to request from the model (Ollama `num_ctx`). 4096 matches what Ollama
    /// picks on an 8 GB Apple M2 (P0-T03) and bounds memory use.
    pub tokens: u32,
    /// Tokens kept free for the reply. The prompt budget is `tokens - reserve_tokens`.
    pub reserve_tokens: u32,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            tokens: 4_096,
            reserve_tokens: 1_024,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            provider: ProviderKind::Ollama,
            model_id: None,
            allowed_roots: Vec::new(),
            network_mode: NetworkMode::LocalOnly,
            retention: Retention::default(),
            wake_word: false,
            ollama: OllamaConfig {
                base_url: String::from(DEFAULT_OLLAMA_URL),
            },
            context: ContextConfig::default(),
            tools: ToolsConfig::default(),
        }
    }
}

/// The result of loading: the config plus anything the user should be told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedConfig {
    /// The effective configuration.
    pub config: Config,
    /// Human-readable warnings (e.g. a corrupt file was replaced).
    pub warnings: Vec<String>,
}

/// One migration step: turns a version-`N` JSON object into a version-`N+1` object.
type MigrationStep = fn(Map<String, Value>) -> Result<Map<String, Value>, String>;

/// `MIGRATIONS[n]` migrates version `n` to `n + 1`. Append only.
const MIGRATIONS: &[MigrationStep] = &[migrate_v0_to_v1, migrate_v1_to_v2, migrate_v2_to_v3];

/// Version 0 is a file without a `version` field (hand-written, or from a pre-release build).
/// Missing fields take the **version 1** defaults (frozen here, never `Config::default()`, which
/// moves on); present fields are kept as they are.
fn migrate_v0_to_v1(old: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let Value::Object(mut merged) = serde_json::json!({
        "version": 1,
        "provider": "mock",
        "model_id": null,
        "allowed_roots": [],
        "network_mode": "local_only",
        "retention": { "history_days": null },
        "wake_word": false
    }) else {
        return Err(String::from("v1 defaults are not an object"));
    };
    for (key, value) in old {
        merged.insert(key, value);
    }
    merged.insert(String::from("version"), Value::from(1_u64));
    Ok(merged)
}

/// Version 2 (P2) adds `ollama` and `context`. The chosen `provider` is kept: a P1 config that
/// says `mock` stays `mock` until the user switches.
fn migrate_v1_to_v2(mut old: Map<String, Value>) -> Result<Map<String, Value>, String> {
    old.entry("ollama")
        .or_insert_with(|| serde_json::json!({ "base_url": DEFAULT_OLLAMA_URL }));
    old.entry("context")
        .or_insert_with(|| serde_json::json!({ "tokens": 4096, "reserve_tokens": 1024 }));
    old.insert(String::from("version"), Value::from(2_u64));
    Ok(old)
}

/// Version 3 (P3) adds `tools` limits with their defaults.
fn migrate_v2_to_v3(mut old: Map<String, Value>) -> Result<Map<String, Value>, String> {
    old.entry("tools").or_insert_with(|| {
        serde_json::json!({ "enabled": true, "max_calls_per_turn": 6, "timeout_secs": 30, "concurrency": 2 })
    });
    old.insert(String::from("version"), Value::from(3_u64));
    Ok(old)
}

/// Loads the config at `path`, creating, migrating or repairing it as described in the module docs.
pub fn load_or_init(path: &Path) -> Result<LoadedConfig, RuntimeError> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let config = Config::default();
            save(path, &config)?;
            return Ok(LoadedConfig {
                config,
                warnings: Vec::new(),
            });
        }
        Err(source) => {
            return Err(RuntimeError::Io {
                action: "read config",
                path: path.to_path_buf(),
                source,
            });
        }
    };

    let object = match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(object)) => object,
        Ok(_) | Err(_) => return recover_corrupt(path, "it is not a JSON object"),
    };

    let found = match object.get("version") {
        None => 0,
        Some(value) => match value.as_u64() {
            Some(version) => version,
            None => return recover_corrupt(path, "its `version` is not a number"),
        },
    };
    if found > CONFIG_VERSION {
        return Err(RuntimeError::ConfigTooNew {
            found,
            supported: CONFIG_VERSION,
        });
    }

    let migrated = migrate(object, found)?;
    let config = match serde_json::from_value::<Config>(Value::Object(migrated)) {
        Ok(config) => config,
        Err(_) => return recover_corrupt(path, "it has missing, unknown or invalid fields"),
    };

    let mut warnings = Vec::new();
    if found < CONFIG_VERSION {
        let backup = sibling(path, &format!("v{found}.bak"));
        fs::copy(path, &backup).map_err(|source| RuntimeError::Io {
            action: "back up config",
            path: backup.clone(),
            source,
        })?;
        save(path, &config)?;
        warnings.push(format!(
            "Migrated config from version {found} to {CONFIG_VERSION}; the old file is at {}",
            backup.display()
        ));
    }
    Ok(LoadedConfig { config, warnings })
}

fn migrate(mut object: Map<String, Value>, from: u64) -> Result<Map<String, Value>, RuntimeError> {
    for version in from..CONFIG_VERSION {
        let step = usize::try_from(version)
            .ok()
            .and_then(|index| MIGRATIONS.get(index))
            .ok_or_else(|| RuntimeError::ConfigMigration {
                from: version,
                reason: String::from("no migration step registered"),
            })?;
        object = step(object).map_err(|reason| RuntimeError::ConfigMigration {
            from: version,
            reason,
        })?;
    }
    Ok(object)
}

/// Moves a bad file aside and writes the default config in its place.
fn recover_corrupt(path: &Path, why: &str) -> Result<LoadedConfig, RuntimeError> {
    let backup = sibling(path, &format!("corrupt-{}", UnixMillis::now().0));
    fs::rename(path, &backup).map_err(|source| RuntimeError::Io {
        action: "move corrupt config aside",
        path: backup.clone(),
        source,
    })?;
    let config = Config::default();
    save(path, &config)?;
    let warning = format!(
        "The config file was unreadable because {why}. It was moved to {} and the defaults were restored.",
        backup.display()
    );
    tracing::warn!("{warning}");
    Ok(LoadedConfig {
        config,
        warnings: vec![warning],
    })
}

/// `config.json` + `suffix` -> `config.json.<suffix>`, in the same directory.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".");
    name.push(suffix);
    path.with_file_name(name)
}

/// Writes `config` atomically: temp file in the same directory, fsync, then rename.
///
/// Rename within one directory is atomic on every supported OS (on Windows, `std::fs::rename`
/// replaces the target), so readers see either the old file or the new one, never a torn write.
pub fn save(path: &Path, config: &Config) -> Result<(), RuntimeError> {
    let io = |action: &'static str, at: &Path| {
        let at = at.to_path_buf();
        move |source| RuntimeError::Io {
            action,
            path: at,
            source,
        }
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(io("create config dir", dir))?;
    let temp = sibling(path, &format!("tmp-{}", std::process::id()));
    let mut json = serde_json::to_vec_pretty(config).map_err(|error| RuntimeError::Io {
        action: "serialize config",
        path: path.to_path_buf(),
        source: std::io::Error::other(error),
    })?;
    json.push(b'\n');
    {
        let mut file = File::create(&temp).map_err(io("create temp config", &temp))?;
        file.write_all(&json)
            .map_err(io("write temp config", &temp))?;
        file.sync_all().map_err(io("sync temp config", &temp))?;
    }
    if let Err(source) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(RuntimeError::Io {
            action: "replace config",
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_path(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("config.json")
    }

    fn backups(dir: &tempfile::TempDir, marker: &str) -> Vec<PathBuf> {
        fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.to_string_lossy().contains(marker))
            .collect()
    }

    #[test]
    fn default_config_is_private_and_offline() {
        let config = Config::default();
        assert_eq!(config.version, CONFIG_VERSION);
        assert_eq!(config.network_mode, NetworkMode::LocalOnly);
        assert!(!config.wake_word);
        assert!(config.allowed_roots.is_empty());
        assert_eq!(config.retention.history_days, None);
    }

    #[test]
    fn missing_file_creates_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(&dir);
        let loaded = load_or_init(&path).unwrap();
        assert_eq!(loaded.config, Config::default());
        assert!(loaded.warnings.is_empty());
        assert!(path.is_file());
    }

    #[test]
    fn config_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(&dir);
        let config = Config {
            model_id: Some("qwen2.5:3b-instruct".into()),
            allowed_roots: vec![AllowedRoot {
                id: "docs".into(),
                path: dir.path().join("Documents"),
            }],
            network_mode: NetworkMode::ToolsAllowed,
            retention: Retention {
                history_days: Some(30),
            },
            wake_word: true,
            ..Config::default()
        };
        save(&path, &config).unwrap();
        let loaded = load_or_init(&path).unwrap();
        assert_eq!(loaded.config, config);
        assert!(loaded.warnings.is_empty());
        assert!(backups(&dir, "tmp-").is_empty(), "temp file left behind");
    }

    #[test]
    fn corrupted_file_is_backed_up_and_replaced_with_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(&dir);
        fs::write(&path, "{ this is not json").unwrap();
        let loaded = load_or_init(&path).unwrap();
        assert_eq!(loaded.config, Config::default());
        assert_eq!(loaded.warnings.len(), 1);
        let saved = backups(&dir, "corrupt-");
        assert_eq!(saved.len(), 1);
        assert_eq!(fs::read_to_string(&saved[0]).unwrap(), "{ this is not json");
        // The replacement is valid and loads cleanly.
        assert!(load_or_init(&path).unwrap().warnings.is_empty());
    }

    #[test]
    fn unknown_field_is_treated_as_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(&dir);
        let mut value = serde_json::to_value(Config::default()).unwrap();
        value["auto_approve_everything"] = Value::Bool(true);
        fs::write(&path, value.to_string()).unwrap();
        let loaded = load_or_init(&path).unwrap();
        assert_eq!(loaded.config, Config::default());
        assert_eq!(backups(&dir, "corrupt-").len(), 1);
    }

    #[test]
    fn version_0_file_is_migrated_and_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(&dir);
        let v0 = r#"{ "model_id": "llama3.2:3b", "network_mode": "tools_allowed" }"#;
        fs::write(&path, v0).unwrap();

        let loaded = load_or_init(&path).unwrap();
        assert_eq!(loaded.config.version, CONFIG_VERSION);
        assert_eq!(loaded.config.model_id.as_deref(), Some("llama3.2:3b"));
        assert_eq!(loaded.config.network_mode, NetworkMode::ToolsAllowed);
        assert_eq!(loaded.config.provider, ProviderKind::Mock);
        assert_eq!(loaded.warnings.len(), 1);

        let backup = path.with_file_name("config.json.v0.bak");
        assert_eq!(fs::read_to_string(backup).unwrap(), v0);
        let on_disk: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk["version"], CONFIG_VERSION);
        // Second load: already current, nothing to migrate.
        assert!(load_or_init(&path).unwrap().warnings.is_empty());
    }

    #[test]
    fn newer_version_is_refused_and_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(&dir);
        let future = r#"{ "version": 99, "provider": "something_new" }"#;
        fs::write(&path, future).unwrap();
        let error = load_or_init(&path).unwrap_err();
        assert!(
            matches!(error, RuntimeError::ConfigTooNew { found: 99, .. }),
            "{error:?}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), future);
    }

    #[test]
    fn version_1_file_from_p1_is_migrated_to_v2_keeping_the_provider() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(&dir);
        // Byte-for-byte what a P1 build wrote by default.
        let v1 = r#"{
  "version": 1,
  "provider": "mock",
  "model_id": null,
  "allowed_roots": [],
  "network_mode": "local_only",
  "retention": { "history_days": null },
  "wake_word": false
}
"#;
        fs::write(&path, v1).unwrap();
        let loaded = load_or_init(&path).unwrap();
        assert_eq!(loaded.config.version, CONFIG_VERSION);
        assert_eq!(loaded.config.tools, ToolsConfig::default());
        assert_eq!(
            loaded.config.provider,
            ProviderKind::Mock,
            "explicit choice kept"
        );
        assert_eq!(loaded.config.ollama.base_url, DEFAULT_OLLAMA_URL);
        assert_eq!(loaded.config.context, ContextConfig::default());
        assert_eq!(
            fs::read_to_string(path.with_file_name("config.json.v1.bak")).unwrap(),
            v1
        );
    }

    #[test]
    fn new_configs_default_to_ollama_on_loopback() {
        let config = Config::default();
        assert_eq!(config.provider, ProviderKind::Ollama);
        assert_eq!(config.ollama.base_url, "http://127.0.0.1:11434");
        assert!(config.context.reserve_tokens < config.context.tokens);
    }

    #[test]
    fn every_version_has_a_migration_step() {
        assert_eq!(MIGRATIONS.len() as u64, CONFIG_VERSION);
    }
}
