//! Per-user directories for data, config and logs.
//!
//! Precedence: an explicit override (`--data-dir`), then the `LOKI_DATA_DIR` environment
//! variable, then the OS convention from the `directories` crate. Home directories are never
//! hard-coded. With an override, everything lives under that one directory, which is what
//! tests and CI rely on to stay away from real user data.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::RuntimeError;

/// Environment variable that overrides the data directory.
pub const DATA_DIR_ENV: &str = "LOKI_DATA_DIR";

/// Resolved directories. Create them with [`Paths::ensure`] before use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// Database and other durable data.
    pub data_dir: PathBuf,
    /// `config.json` lives here.
    pub config_dir: PathBuf,
    /// Rolling log files.
    pub log_dir: PathBuf,
}

impl Paths {
    /// Resolves paths using the real environment.
    pub fn resolve(override_dir: Option<PathBuf>) -> Result<Self, RuntimeError> {
        Self::resolve_with(override_dir, std::env::var_os(DATA_DIR_ENV))
    }

    /// Resolves paths from explicit inputs. Separate from [`Paths::resolve`] so tests don't
    /// mutate the process environment.
    pub fn resolve_with(
        override_dir: Option<PathBuf>,
        env_dir: Option<OsString>,
    ) -> Result<Self, RuntimeError> {
        let chosen =
            override_dir.or_else(|| env_dir.filter(|value| !value.is_empty()).map(PathBuf::from));
        if let Some(root) = chosen {
            let root = std::path::absolute(&root).map_err(|source| RuntimeError::Io {
                action: "resolve data dir",
                path: root.clone(),
                source,
            })?;
            return Ok(Self::under(&root));
        }
        let dirs = directories::ProjectDirs::from("", "", "Loki").ok_or(RuntimeError::NoHomeDir)?;
        Ok(Self {
            data_dir: dirs.data_local_dir().to_path_buf(),
            config_dir: dirs.config_dir().to_path_buf(),
            log_dir: dirs.data_local_dir().join("logs"),
        })
    }

    /// Everything under one root: `<root>`, `<root>/config`, `<root>/logs`.
    pub fn under(root: &Path) -> Self {
        Self {
            data_dir: root.to_path_buf(),
            config_dir: root.join("config"),
            log_dir: root.join("logs"),
        }
    }

    /// Creates all three directories if they don't exist.
    pub fn ensure(&self) -> Result<(), RuntimeError> {
        for dir in [&self.data_dir, &self.config_dir, &self.log_dir] {
            std::fs::create_dir_all(dir).map_err(|source| RuntimeError::Io {
                action: "create directory",
                path: dir.clone(),
                source,
            })?;
        }
        Ok(())
    }

    /// The SQLite database file.
    pub fn database_file(&self) -> PathBuf {
        self.data_dir.join("loki.db")
    }

    /// The configuration file.
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_override_wins_over_env() {
        let dir = tempfile::tempdir().unwrap();
        let flag = dir.path().join("flag");
        let env = dir.path().join("env");
        let paths = Paths::resolve_with(Some(flag.clone()), Some(env.into_os_string())).unwrap();
        assert_eq!(paths.data_dir, flag);
        assert_eq!(paths.config_dir, flag.join("config"));
        assert_eq!(paths.log_dir, flag.join("logs"));
    }

    #[test]
    fn env_is_used_when_no_flag() {
        let dir = tempfile::tempdir().unwrap();
        let env = dir.path().join("env");
        let paths = Paths::resolve_with(None, Some(env.clone().into_os_string())).unwrap();
        assert_eq!(paths.data_dir, env);
    }

    #[test]
    fn empty_env_falls_back_to_os_default() {
        let paths = Paths::resolve_with(None, Some(OsString::new())).unwrap();
        assert!(paths.data_dir.is_absolute());
        assert!(paths.log_dir.starts_with(&paths.data_dir));
    }

    #[test]
    fn relative_override_becomes_absolute() {
        let paths = Paths::resolve_with(Some(PathBuf::from("rel-data")), None).unwrap();
        assert!(paths.data_dir.is_absolute());
        assert!(paths.data_dir.ends_with("rel-data"));
    }

    #[test]
    fn ensure_creates_directories() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(&dir.path().join("nested").join("root"));
        paths.ensure().unwrap();
        assert!(paths.config_dir.is_dir() && paths.log_dir.is_dir());
        assert_eq!(
            paths.database_file().parent(),
            Some(paths.data_dir.as_path())
        );
    }
}
