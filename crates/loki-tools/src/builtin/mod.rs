//! Built-in tools. Each is a typed Rust function behind a strict schema; none runs a shell.

pub mod basic;
pub mod files;

use std::sync::Arc;

use crate::registry::{Registry, RegistryError};

pub use basic::{AppList, AppOpen, SystemInfoTool, TimeNow};
pub use files::{FileIndex, FileRoots, FilesRead, FilesSearch, FilesState};

/// A registry with every P3 built-in tool. `files` holds the allowed roots.
pub fn default_registry(files: FilesState) -> Result<Registry, RegistryError> {
    let mut registry = Registry::new();
    registry.register(Arc::new(TimeNow::default()))?;
    registry.register(Arc::new(SystemInfoTool::default()))?;
    registry.register(Arc::new(AppList::default()))?;
    registry.register(Arc::new(AppOpen::default()))?;
    registry.register(Arc::new(FilesSearch::new(files.clone())))?;
    registry.register(Arc::new(FilesRead::new(files)))?;
    Ok(registry)
}
