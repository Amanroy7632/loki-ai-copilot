//! Runtime errors, surfaced to the CLI and (from P5) the Tauri commands.

use std::path::PathBuf;

use loki_llm::LlmError;
use loki_memory::MemoryError;

/// Why a runtime operation failed.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// A filesystem operation failed.
    #[error("could not {action} {}: {source}", path.display())]
    Io {
        /// What we were doing, e.g. "write config".
        action: &'static str,
        /// The path involved.
        path: PathBuf,
        /// The OS error.
        #[source]
        source: std::io::Error,
    },
    /// The OS reported no home directory, so default paths can't be resolved.
    #[error("no home directory found; pass --data-dir or set LOKI_DATA_DIR")]
    NoHomeDir,
    /// The config file was written by a newer Loki. It is left untouched.
    #[error(
        "config version {found} is newer than this build supports ({supported}); refusing to overwrite it"
    )]
    ConfigTooNew {
        /// Version found on disk.
        found: u64,
        /// Highest version this build knows.
        supported: u64,
    },
    /// A config migration step failed.
    #[error("config migration from version {from} failed: {reason}")]
    ConfigMigration {
        /// The version being migrated from.
        from: u64,
        /// What went wrong.
        reason: String,
    },
    /// Logging could not be initialised.
    #[error("could not initialise logging: {0}")]
    Logging(String),
    /// Storage failed.
    #[error(transparent)]
    Memory(#[from] MemoryError),
    /// The model provider failed.
    #[error(transparent)]
    Llm(#[from] LlmError),
    /// No conversation has that ID.
    #[error("conversation {0} not found")]
    ConversationNotFound(String),
    /// The configured provider isn't available in this build.
    #[error("provider '{0}' is not available yet in this build")]
    ProviderUnavailable(String),
}
