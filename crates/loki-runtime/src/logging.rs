//! Structured logging to rolling files in the per-user log directory (P1-T07).
//!
//! - Files: `<log_dir>/loki.<date>.log`, rotated daily, the newest 14 kept.
//! - Filter: the `LOKI_LOG` env var (`tracing-subscriber` `EnvFilter` syntax), default `info`.
//! - Every formatted line passes through [`Redactor`] before it is written.
//! - Nothing is written to stdout/stderr, so the CLI's user output stays clean, and nothing is
//!   written to the current working directory.

use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

use crate::error::RuntimeError;
use crate::redact::Redactor;

/// Env var holding the log filter.
pub const LOG_FILTER_ENV: &str = "LOKI_LOG";
/// Log file name prefix.
pub const LOG_FILE_PREFIX: &str = "loki";
const MAX_LOG_FILES: usize = 14;

/// Keeps the background log writer alive. Logs are flushed when it is dropped, so hold it
/// until the process exits.
#[must_use = "dropping the guard stops logging"]
pub struct LoggingGuard {
    _worker: WorkerGuard,
}

/// A `MakeWriter` that redacts each formatted event before handing it to the file writer.
#[derive(Clone)]
struct RedactingMakeWriter {
    inner: NonBlocking,
    redactor: Arc<Redactor>,
}

struct RedactingWriter {
    inner: NonBlocking,
    redactor: Arc<Redactor>,
}

impl Write for RedactingWriter {
    // The fmt layer formats a whole event into one buffer and writes it in a single call,
    // so each call is one complete line and patterns can't be split across calls.
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let redacted = self.redactor.redact(&String::from_utf8_lossy(buf));
        self.inner.write_all(redacted.as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<'a> MakeWriter<'a> for RedactingMakeWriter {
    type Writer = RedactingWriter;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            inner: self.inner.clone(),
            redactor: Arc::clone(&self.redactor),
        }
    }
}

/// Builds the file subscriber without installing it, so tests can scope it with
/// `tracing::subscriber::with_default`.
pub fn file_subscriber(
    log_dir: &Path,
    filter: EnvFilter,
    redactor: Redactor,
) -> Result<(impl tracing::Subscriber + Send + Sync + use<>, LoggingGuard), RuntimeError> {
    std::fs::create_dir_all(log_dir).map_err(|source| RuntimeError::Io {
        action: "create log dir",
        path: log_dir.to_path_buf(),
        source,
    })?;
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix("log")
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)
        .map_err(|error| RuntimeError::Logging(error.to_string()))?;
    let (non_blocking, worker) = tracing_appender::non_blocking(appender);
    let writer = RedactingMakeWriter {
        inner: non_blocking,
        redactor: Arc::new(redactor),
    };
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(true)
        .with_writer(writer)
        .finish();
    Ok((subscriber, LoggingGuard { _worker: worker }))
}

/// The filter from `LOKI_LOG`, or `info` if it is unset or invalid.
pub fn filter_from_env() -> EnvFilter {
    EnvFilter::try_from_env(LOG_FILTER_ENV).unwrap_or_else(|_| EnvFilter::new("info"))
}

/// Installs file logging as the process-wide subscriber. Call once, early in `main`.
pub fn init(log_dir: &Path, redactor: Redactor) -> Result<LoggingGuard, RuntimeError> {
    let (subscriber, guard) = file_subscriber(log_dir, filter_from_env(), redactor)?;
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|error| RuntimeError::Logging(error.to_string()))?;
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log_files(dir: &Path) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().path())
                    .filter(|path| {
                        path.file_name()
                            .is_some_and(|name| name.to_string_lossy().starts_with(LOG_FILE_PREFIX))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn logs_are_written_to_the_log_dir_and_redacted() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("data").join("logs");
        let cwd_before = log_files(&std::env::current_dir().unwrap()).len();

        let (subscriber, guard) =
            file_subscriber(&log_dir, EnvFilter::new("info"), Redactor::default()).unwrap();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(
                user = "jane@example.com",
                "token=abc123secret opened /etc/hosts"
            );
            tracing::debug!("debug lines are filtered out at info");
        });
        drop(guard); // flushes the background writer

        let files = log_files(&log_dir);
        assert_eq!(files.len(), 1, "{files:?}");
        let contents = std::fs::read_to_string(&files[0]).unwrap();
        assert!(contents.contains("opened [PATH]"), "{contents}");
        assert!(contents.contains("token=[REDACTED]"), "{contents}");
        assert!(contents.contains("[EMAIL]"), "{contents}");
        assert!(!contents.contains("jane@example.com"));
        assert!(!contents.contains("abc123secret"));
        assert!(!contents.contains("debug lines"));
        assert_eq!(
            log_files(&std::env::current_dir().unwrap()).len(),
            cwd_before,
            "no log file may appear in the working directory"
        );
    }
}
