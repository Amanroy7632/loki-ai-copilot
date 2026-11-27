//! `files.search` and `files.read` inside user-approved roots (P3-T08, SEC-02, SEC-03).
//!
//! Safety design:
//! - Roots come from config as `root_id -> path`. Each path is canonicalized once at startup
//!   (symlinks resolved); the model only ever sees `root_id`s.
//! - The model never supplies a path. `files.search` returns opaque `file_id`s
//!   (`<root_id>/<32 hex>`) that map to a root-relative path in an in-memory index, plus a
//!   root-relative display path. `files.read` accepts only `file_id`s from that index.
//! - Every read **re-resolves** the path: the relative path may only contain normal components,
//!   and the canonicalized target (symlinks resolved at read time) must be inside the canonical
//!   root. A symlink swapped in after the search is caught.
//! - The search walk never follows symlinks and skips hidden and system files.
//! - Reads are limited to text-like extensions, `max_bytes` (at most 256 KiB), and refuse binary data.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::FutureExt;
use futures::future::BoxFuture;
use loki_core::{Capability, Risk, ToolName, ToolSpec};
use loki_policy::Allowed;
use serde_json::{Value, json};

use crate::tool::{Tool, ToolContext, ToolError};

/// Largest `max_bytes` for `files.read` (CONTRACTS.md).
pub const MAX_READ_BYTES: u64 = 256 * 1024;
/// Largest `limit` for `files.search`.
pub const MAX_SEARCH_RESULTS: u64 = 50;
/// The walk stops after visiting this many entries, so a huge root can't stall a turn.
const MAX_WALK_ENTRIES: usize = 50_000;
/// Directory depth limit for the walk.
const MAX_WALK_DEPTH: usize = 12;
/// How many `file_id`s the index remembers (oldest are forgotten first).
const INDEX_CAPACITY: usize = 4_096;
/// Extensions `files.read` accepts: plain text, Markdown, CSV and common code/config files.
const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "csv", "tsv", "log", "json", "toml", "yaml", "yml", "ini", "cfg",
    "conf", "xml", "html", "css", "rs", "py", "js", "ts", "tsx", "jsx", "go", "java", "kt",
    "swift", "c", "h", "cpp", "hpp", "cs", "rb", "php", "sh", "zsh", "sql", "lua", "r", "tex",
];
/// System files skipped even though they don't start with a dot.
const SYSTEM_NAMES: &[&str] = &["Thumbs.db", "desktop.ini", "Icon\r"];

/// Why a path string is refused (SEC-02). Pure string checks, also used on Windows-style input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathRefusal {
    /// Empty path.
    #[error("empty path")]
    Empty,
    /// `..` anywhere.
    #[error("'..' is not allowed")]
    ParentDir,
    /// `/x`, `C:\x`, `C:x`, a UNC path or a device path.
    #[error("absolute, drive, UNC and device paths are not allowed")]
    Absolute,
    /// `~` or `~user`.
    #[error("'~' is not allowed")]
    Home,
    /// NUL or other control characters.
    #[error("control characters are not allowed")]
    Control,
}

/// Checks a **relative** path from untrusted input. Refuses `..`, absolute paths, drive letters,
/// UNC (`\\server\share`), device paths (`\\.\`, `\\?\`), `~` and control characters, on every OS.
pub fn check_relative_path(raw: &str) -> Result<PathBuf, PathRefusal> {
    if raw.is_empty() {
        return Err(PathRefusal::Empty);
    }
    if raw.chars().any(char::is_control) {
        return Err(PathRefusal::Control);
    }
    if raw.starts_with('/') || raw.starts_with('\\') {
        return Err(PathRefusal::Absolute);
    }
    let bytes = raw.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(PathRefusal::Absolute);
    }
    if raw.starts_with('~') {
        return Err(PathRefusal::Home);
    }
    let mut clean = PathBuf::new();
    for part in raw.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err(PathRefusal::ParentDir),
            normal => clean.push(normal),
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(PathRefusal::Empty);
    }
    Ok(clean)
}

/// Canonical allowed roots, by ID.
#[derive(Debug, Clone, Default)]
pub struct FileRoots {
    roots: BTreeMap<String, PathBuf>,
}

impl FileRoots {
    /// Canonicalizes each root. Roots that don't exist or aren't directories are skipped, with a
    /// warning returned for the user.
    pub fn new(roots: impl IntoIterator<Item = (String, PathBuf)>) -> (Self, Vec<String>) {
        let mut out = BTreeMap::new();
        let mut warnings = Vec::new();
        for (id, path) in roots {
            if !is_valid_root_id(&id) {
                warnings.push(format!(
                    "allowed root '{id}' skipped: IDs are 1-32 of a-z, 0-9, _ or -"
                ));
                continue;
            }
            match path.canonicalize() {
                Ok(canonical) if canonical.is_dir() => {
                    out.insert(id, canonical);
                }
                _ => warnings.push(format!(
                    "allowed root '{id}' skipped: not an existing directory"
                )),
            }
        }
        (Self { roots: out }, warnings)
    }

    /// The canonical path of a root.
    pub fn get(&self, id: &str) -> Option<&Path> {
        self.roots.get(id).map(PathBuf::as_path)
    }

    /// Root IDs, for telling the model what exists.
    pub fn ids(&self) -> Vec<String> {
        self.roots.keys().cloned().collect()
    }

    /// Resolves `relative` inside root `id`, refusing anything that escapes it (after symlinks).
    pub fn resolve(&self, id: &str, relative: &Path) -> Result<PathBuf, ToolError> {
        let root = self
            .get(id)
            .ok_or_else(|| ToolError::InvalidArguments(format!("unknown root_id '{id}'")))?;
        if relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(ToolError::Refused(String::from(
                "path has non-normal components",
            )));
        }
        let canonical = root.join(relative).canonicalize().map_err(|error| match error.kind() {
            std::io::ErrorKind::PermissionDenied => ToolError::PermissionRequired(String::from(
                "macOS denied access; allow it in System Settings > Privacy & Security > Files and Folders",
            )),
            _ => ToolError::InvalidArguments(String::from("file no longer exists")),
        })?;
        if !canonical.starts_with(root) {
            return Err(ToolError::Refused(String::from(
                "path resolves outside the allowed root",
            )));
        }
        Ok(canonical)
    }
}

fn is_valid_root_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn is_hidden_or_system(name: &str) -> bool {
    name.starts_with('.') || SYSTEM_NAMES.contains(&name)
}

/// Index entries by file ID, plus issue order (oldest first) for eviction.
type IndexState = (HashMap<String, (String, PathBuf)>, VecDeque<String>);

/// `file_id -> (root_id, relative path)`, bounded. Shared by both tools.
#[derive(Debug, Default)]
pub struct FileIndex {
    inner: Mutex<IndexState>,
}

impl FileIndex {
    fn issue(&self, root_id: &str, relative: PathBuf) -> String {
        let id = format!("{root_id}/{}", uuid::Uuid::now_v7().simple());
        if let Ok(mut guard) = self.inner.lock() {
            let (map, order) = &mut *guard;
            map.insert(id.clone(), (root_id.to_owned(), relative));
            order.push_back(id.clone());
            while order.len() > INDEX_CAPACITY {
                if let Some(old) = order.pop_front() {
                    map.remove(&old);
                }
            }
        }
        id
    }

    fn lookup(&self, file_id: &str) -> Option<(String, PathBuf)> {
        self.inner.lock().ok()?.0.get(file_id).cloned()
    }
}

/// State shared by `files.search` and `files.read`.
#[derive(Debug, Clone, Default)]
pub struct FilesState {
    /// The allowed roots.
    pub roots: Arc<FileRoots>,
    /// Issued file IDs.
    pub index: Arc<FileIndex>,
}

fn root_scope(arguments: &Value, key: &str) -> String {
    let value = arguments.get(key).and_then(Value::as_str).unwrap_or("");
    // For file_id the scope is the root prefix; the read re-checks the index agrees.
    value.split('/').next().unwrap_or("").to_owned()
}

// ------------------------------------------------------------------------------ files.search

/// `files.search {root_id, query, limit?}`: file names containing `query` inside one root.
pub struct FilesSearch {
    spec: ToolSpec,
    state: FilesState,
}

impl FilesSearch {
    /// A search tool over `state`.
    pub fn new(state: FilesState) -> Self {
        Self {
            spec: ToolSpec {
                name: ToolName::new("files.search"),
                description: String::from(
                    "Find files by name inside an allowed folder (root_id). Returns file_ids for files.read.",
                ),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "root_id": { "type": "string", "pattern": "^[a-z0-9_-]{1,32}$", "maxLength": 32 },
                        "query": { "type": "string", "minLength": 1, "maxLength": 128 },
                        "limit": { "type": "integer", "minimum": 1, "maximum": MAX_SEARCH_RESULTS }
                    },
                    "required": ["root_id", "query"],
                    "additionalProperties": false
                }),
                risk: Risk::ReadOnly,
                capability: Capability::FileRead,
            },
            state,
        }
    }
}

fn walk(root: &Path, query: &str, limit: usize) -> (Vec<(PathBuf, u64)>, bool) {
    let mut found = Vec::new();
    let mut visited = 0_usize;
    let mut stack: Vec<(PathBuf, usize)> = vec![(PathBuf::new(), 0)];
    while let Some((relative, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(root.join(&relative)) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > MAX_WALK_ENTRIES {
                return (found, true);
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_hidden_or_system(&name) {
                continue;
            }
            // symlink_metadata: never follow links while walking (SEC-03).
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            let child = relative.join(&name);
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                if depth < MAX_WALK_DEPTH {
                    stack.push((child, depth + 1));
                }
            } else if meta.is_file() && name.to_lowercase().contains(query) {
                found.push((child, meta.len()));
                if found.len() >= limit {
                    return (found, true);
                }
            }
        }
    }
    (found, false)
}

fn display_path(relative: &Path) -> String {
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

impl Tool for FilesSearch {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn scope(&self, arguments: &Value) -> String {
        root_scope(arguments, "root_id")
    }

    fn execute<'a>(
        &'a self,
        arguments: Value,
        _context: ToolContext,
        _permit: &'a Allowed,
    ) -> BoxFuture<'a, Result<Value, ToolError>> {
        async move {
            let root_id = arguments["root_id"].as_str().unwrap_or("").to_owned();
            let query = arguments["query"].as_str().unwrap_or("").to_lowercase();
            let limit =
                usize::try_from(arguments.get("limit").and_then(Value::as_u64).unwrap_or(20))
                    .unwrap_or(20);
            let root = self
                .state
                .roots
                .get(&root_id)
                .ok_or_else(|| {
                    ToolError::InvalidArguments(format!(
                        "unknown root_id '{root_id}'; known: {:?}",
                        self.state.roots.ids()
                    ))
                })?
                .to_path_buf();
            let (hits, truncated) = tokio::task::spawn_blocking(move || walk(&root, &query, limit))
                .await
                .map_err(|error| ToolError::Failed(error.to_string()))?;
            let files: Vec<Value> = hits
                .into_iter()
                .map(|(relative, size)| {
                    let path = display_path(&relative);
                    let file_id = self.state.index.issue(&root_id, relative);
                    json!({ "file_id": file_id, "path": path, "size_bytes": size })
                })
                .collect();
            Ok(json!({ "root_id": root_id, "files": files, "truncated": truncated }))
        }
        .boxed()
    }

    fn audit_summary(&self, result: &Value) -> String {
        format!(
            "{} files in root '{}'",
            result["files"].as_array().map_or(0, Vec::len),
            result["root_id"].as_str().unwrap_or("?")
        )
    }
}

// -------------------------------------------------------------------------------- files.read

/// `files.read {file_id, max_bytes}`: text content of a file found by `files.search`.
pub struct FilesRead {
    spec: ToolSpec,
    state: FilesState,
}

impl FilesRead {
    /// A read tool over `state`.
    pub fn new(state: FilesState) -> Self {
        Self {
            spec: ToolSpec {
                name: ToolName::new("files.read"),
                description: String::from(
                    "Read a text file by file_id (from files.search), up to max_bytes.",
                ),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "file_id": { "type": "string", "pattern": "^[a-z0-9_-]{1,32}/[0-9a-f]{32}$", "maxLength": 65 },
                        "max_bytes": { "type": "integer", "minimum": 1, "maximum": MAX_READ_BYTES }
                    },
                    "required": ["file_id", "max_bytes"],
                    "additionalProperties": false
                }),
                risk: Risk::ReadOnly,
                capability: Capability::FileRead,
            },
            state,
        }
    }
}

fn read_text(path: &Path, max_bytes: u64) -> Result<(String, bool, usize), ToolError> {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !TEXT_EXTENSIONS.contains(&extension.as_str()) {
        return Err(ToolError::Refused(format!(
            "unsupported file type '.{extension}'; only text files can be read"
        )));
    }
    let file = std::fs::File::open(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::PermissionDenied => {
            ToolError::PermissionRequired(String::from("the OS denied read access"))
        }
        _ => ToolError::Failed(String::from("could not open the file")),
    })?;
    let mut buffer = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut buffer)
        .map_err(|_| ToolError::Failed(String::from("could not read the file")))?;
    let truncated = buffer.len() as u64 > max_bytes;
    buffer.truncate(usize::try_from(max_bytes).unwrap_or(usize::MAX));
    if buffer.contains(&0) {
        return Err(ToolError::Refused(String::from("file looks binary")));
    }
    // A cut may split a multi-byte character; keep only the valid prefix.
    let valid = match std::str::from_utf8(&buffer) {
        Ok(text) => text.len(),
        Err(error) if truncated && error.error_len().is_none() => error.valid_up_to(),
        Err(_) => return Err(ToolError::Refused(String::from("file is not UTF-8 text"))),
    };
    buffer.truncate(valid);
    let bytes = buffer.len();
    let text = String::from_utf8(buffer)
        .map_err(|_| ToolError::Refused(String::from("file is not UTF-8 text")))?;
    Ok((text, truncated, bytes))
}

impl Tool for FilesRead {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn scope(&self, arguments: &Value) -> String {
        root_scope(arguments, "file_id")
    }

    fn execute<'a>(
        &'a self,
        arguments: Value,
        _context: ToolContext,
        _permit: &'a Allowed,
    ) -> BoxFuture<'a, Result<Value, ToolError>> {
        async move {
            let file_id = arguments["file_id"].as_str().unwrap_or("");
            let max_bytes = arguments["max_bytes"].as_u64().unwrap_or(0).min(MAX_READ_BYTES);
            let (root_id, relative) = self
                .state
                .index
                .lookup(file_id)
                .ok_or_else(|| ToolError::InvalidArguments(String::from("unknown file_id; get one from files.search")))?;
            // The scope policy checked is the file_id prefix; it must be the indexed root.
            if file_id.split('/').next() != Some(root_id.as_str()) {
                return Err(ToolError::Refused(String::from("file_id root mismatch")));
            }
            let roots = Arc::clone(&self.state.roots);
            let (text, truncated, bytes) = tokio::task::spawn_blocking(move || {
                let path = roots.resolve(&root_id, &relative)?;
                read_text(&path, max_bytes)
            })
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))??;
            Ok(json!({ "file_id": file_id, "content": text, "bytes": bytes, "truncated": truncated }))
        }
        .boxed()
    }

    /// Never the content: only how much was read.
    fn audit_summary(&self, result: &Value) -> String {
        format!(
            "read {} bytes from root '{}'{}",
            result["bytes"],
            result["file_id"]
                .as_str()
                .and_then(|id| id.split('/').next())
                .unwrap_or("?"),
            if result["truncated"] == true {
                " (truncated)"
            } else {
                ""
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec_02_path_strings_are_refused() {
        for (raw, expected) in [
            ("../etc/passwd", PathRefusal::ParentDir),
            ("notes/../../secret", PathRefusal::ParentDir),
            ("..\\..\\windows", PathRefusal::ParentDir),
            ("/etc/passwd", PathRefusal::Absolute),
            ("C:\\Windows\\system32", PathRefusal::Absolute),
            ("c:relative", PathRefusal::Absolute),
            ("\\\\server\\share\\file", PathRefusal::Absolute),
            ("\\\\.\\PhysicalDrive0", PathRefusal::Absolute),
            ("\\\\?\\C:\\x", PathRefusal::Absolute),
            ("~/.ssh/id_ed25519", PathRefusal::Home),
            ("~root", PathRefusal::Home),
            ("a\0b", PathRefusal::Control),
            ("", PathRefusal::Empty),
            ("./.", PathRefusal::Empty),
        ] {
            assert_eq!(check_relative_path(raw), Err(expected), "{raw:?}");
        }
        assert_eq!(
            check_relative_path("docs/./a.txt").unwrap(),
            PathBuf::from("docs").join("a.txt")
        );
    }

    #[test]
    fn root_ids_are_restricted() {
        assert!(is_valid_root_id("docs"));
        assert!(is_valid_root_id("work_2-x"));
        for bad in ["", "Docs", "../x", "a/b", &"x".repeat(33)] {
            assert!(!is_valid_root_id(bad), "{bad}");
        }
    }
}
