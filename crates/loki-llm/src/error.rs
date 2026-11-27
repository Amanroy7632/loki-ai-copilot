//! Provider errors.

use serde::{Deserialize, Serialize};

/// Why a generation failed. Carried as the terminal `Err` item of a [`crate::ChatStream`].
///
/// Messages are for logs and the user. They must never contain secrets or full prompts.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum LlmError {
    /// The caller cancelled through the `CancellationToken`. Not a failure of the provider.
    #[error("generation cancelled")]
    Cancelled,
    /// The backend can't be reached or isn't ready (e.g. the model server is not running).
    #[error("provider unavailable: {0}")]
    Unavailable(String),
    /// The backend is running but the requested model isn't installed.
    #[error("model not installed: {0}")]
    ModelMissing(String),
    /// The backend answered with something that breaks the provider protocol.
    #[error("provider protocol error: {0}")]
    Protocol(String),
    /// The request does not fit the model's context window.
    #[error("context limit exceeded: {0}")]
    ContextLimit(String),
}

impl LlmError {
    /// `true` for user cancellation, which callers normally report quietly rather than as an error.
    pub const fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}
