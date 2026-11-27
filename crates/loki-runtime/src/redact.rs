//! Masking secrets and personal data before text reaches a log (SECURITY_MODEL.md "Logging and audit").
//!
//! Masks: bearer tokens and `key=value` secrets, well-known token formats (JWTs, `sk-…`, `ghp_…`,
//! AWS access keys), long opaque strings that look like secrets, email addresses, and absolute
//! paths outside the user's allowed roots.
//!
//! This is defence in depth. Code must still avoid logging secrets in the first place.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::{Captures, Regex};

/// Placeholder for any token or secret.
pub const SECRET: &str = "[REDACTED]";
/// Placeholder for an email address.
pub const EMAIL: &str = "[EMAIL]";
/// Placeholder for a path outside the allowed roots.
pub const PATH: &str = "[PATH]";

/// Compiles a pattern that is a constant in this file.
///
/// `expect` is deliberate: the inputs are compile-time constants, every one is exercised by the
/// tests below, so an invalid pattern fails `cargo test` and can never reach a release. Falling
/// back to "match nothing" instead would silently stop redacting, which is worse than a crash.
#[allow(clippy::expect_used)]
fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("redaction patterns are valid constants")
}

static BEARER: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}"));
static KEY_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r#"(?i)\b((?:api[_-]?key|access[_-]?token|refresh[_-]?token|token|secret|password|passwd|pwd|authorization|auth)["']?\s*[:=]\s*["']?)[^\s"',;&]+"#,
    )
});
static KNOWN_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"\b(?:eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}|(?:sk|pk|rk)-[A-Za-z0-9_-]{16,}|gh[pousr]_[A-Za-z0-9]{20,}|xox[abprs]-[A-Za-z0-9-]{10,}|AKIA[0-9A-Z]{16})",
    )
});
static LONG_SECRET: LazyLock<Regex> = LazyLock::new(|| pattern(r"\b[A-Za-z0-9+/_-]{40,}={0,2}"));
static EMAIL_ADDRESS: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}"));
/// An absolute path: Unix `/a/b`, home `~/a`, Windows `C:\a` or `C:/a`, or UNC `\\server\share`.
/// Group 1 is the character before the path (start, whitespace, quote, `=`, `(`, `[`), kept as is.
static ABSOLUTE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r#"(^|[\s"'=(\[])((?:~[/\\]|[A-Za-z]:[\\/]|\\\\|/)[^\s"'<>|()\[\]]*)"#)
});

/// Redacts text for logging, keeping paths that are inside an allowed root.
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    allowed_roots: Vec<PathBuf>,
}

impl Redactor {
    /// A redactor that keeps paths under any of `allowed_roots` and masks every other absolute path.
    pub fn new(allowed_roots: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            allowed_roots: allowed_roots.into_iter().collect(),
        }
    }

    /// Returns `text` with secrets, emails and disallowed paths masked.
    pub fn redact(&self, text: &str) -> String {
        let text = BEARER.replace_all(text, |caps: &Captures<'_>| format!("{} {SECRET}", &caps[1]));
        let text =
            KEY_VALUE.replace_all(&text, |caps: &Captures<'_>| format!("{}{SECRET}", &caps[1]));
        let text = KNOWN_TOKEN.replace_all(&text, SECRET);
        let text = LONG_SECRET.replace_all(&text, SECRET);
        let text = EMAIL_ADDRESS.replace_all(&text, EMAIL);
        let text = ABSOLUTE_PATH.replace_all(&text, |caps: &Captures<'_>| {
            let path = &caps[2];
            let kept: Cow<'_, str> = if self.is_allowed(path) {
                Cow::Borrowed(path)
            } else {
                Cow::Borrowed(PATH)
            };
            format!("{}{kept}", &caps[1])
        });
        text.into_owned()
    }

    fn is_allowed(&self, path: &str) -> bool {
        let candidate = Path::new(path.trim_end_matches(['.', ',', ':', ';']));
        // `..` could climb out of a root, so a path containing it is never "inside".
        if candidate
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return false;
        }
        self.allowed_roots
            .iter()
            .any(|root| candidate.starts_with(root))
    }
}

/// Redacts with no allowed roots: every absolute path is masked.
pub fn redact(text: &str) -> String {
    Redactor::default().redact(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_bearer_and_key_value_secrets() {
        let out = redact("Authorization: Bearer abc.def-ghi_jkl123 and api_key=supersecret123");
        assert!(!out.contains("abc.def-ghi_jkl123"), "{out}");
        assert!(!out.contains("supersecret123"), "{out}");
        assert!(out.contains("api_key=[REDACTED]"), "{out}");

        let out = redact(r#"{"password": "hunter2", "token":"t0k3n"}"#);
        assert!(!out.contains("hunter2") && !out.contains("t0k3n"), "{out}");
    }

    #[test]
    fn masks_known_token_formats() {
        for token in [
            "sk-proj-abcdefghijklmnop1234",
            "ghp_abcdefghijklmnopqrstuvwxyz0123",
            "AKIAIOSFODNN7EXAMPLE",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.dozjgNryP4J3jVmNHl0w5N",
            "xoxb-1234567890-abcdefghij",
        ] {
            let out = redact(&format!("value {token} end"));
            assert_eq!(out, "value [REDACTED] end", "{token}");
        }
    }

    #[test]
    fn masks_long_opaque_secrets_but_not_short_words() {
        let secret = "A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8S9t0";
        assert_eq!(redact(secret), SECRET);
        assert_eq!(redact("hello world 12345"), "hello world 12345");
    }

    #[test]
    fn masks_emails() {
        assert_eq!(
            redact("mail jane.doe+loki@example.co.uk now"),
            "mail [EMAIL] now"
        );
    }

    #[test]
    fn masks_paths_outside_allowed_roots() {
        let redactor = Redactor::new([PathBuf::from("/home/me/Documents")]);
        let out = redactor.redact("read /home/me/Documents/notes.txt and /etc/passwd");
        assert_eq!(out, "read /home/me/Documents/notes.txt and [PATH]");
        assert_eq!(redactor.redact("open ~/secret.txt"), "open [PATH]");
        assert_eq!(redactor.redact(r"copy C:\Users\me\file.txt"), "copy [PATH]");
        assert_eq!(redactor.redact(r"share \\server\share\x"), "share [PATH]");
        assert_eq!(redactor.redact("path=\"/var/log/x\""), "path=\"[PATH]\"");
    }

    #[test]
    fn traversal_out_of_an_allowed_root_is_masked() {
        let redactor = Redactor::new([PathBuf::from("/home/me/Documents")]);
        assert_eq!(
            redactor.redact("/home/me/Documents/../.ssh/id_ed25519"),
            PATH
        );
    }

    #[test]
    fn allowed_root_match_is_by_component_not_prefix() {
        let redactor = Redactor::new([PathBuf::from("/home/me/Doc")]);
        assert_eq!(redactor.redact("/home/me/Documents/a"), PATH);
    }

    #[test]
    fn urls_and_relative_paths_are_left_alone() {
        assert_eq!(
            redact("see https://example.com/docs/page and src/main.rs"),
            "see https://example.com/docs/page and src/main.rs"
        );
    }
}
