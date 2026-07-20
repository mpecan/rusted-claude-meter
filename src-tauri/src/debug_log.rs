//! Opt-in debug logging of raw claude.ai API responses.
//!
//! When the user turns on "Log API responses" in Settings, every usage payload
//! the scheduler fetches is appended verbatim to a local file so the exact
//! wire shape can be inspected — how the token/cost `spend` shape was pinned
//! down (see `crates/meter-api/src/response.rs`), and the way to verify it
//! against account types not yet observed (e.g. a no-limits Enterprise account).
//!
//! The response body carries only usage data: the session key travels in a
//! request header, never in the body, so nothing secret is written. The file is
//! created `0600` (owner-only) all the same, since a user's usage/spend figures
//! are their own business on a shared machine. Logging is best-effort — a write
//! failure is swallowed so it can never break a poll — and off by default.

use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use jiff::Timestamp;

/// File name of the response log inside the resolved log directory.
pub const LOG_FILE: &str = "api-responses.log";

/// Size ceiling for the log. A deliberately-enabled debug log still shouldn't
/// grow without bound across a long session; once it passes this, the next
/// write starts a fresh file (the most recent responses — the ones worth
/// inspecting — are kept, older ones discarded).
const MAX_LOG_BYTES: u64 = 4 * 1024 * 1024;

/// A shared sink for raw API responses. Cheap to clone-share behind an `Arc`:
/// the scheduler's transport holds one clone to write through, the
/// `set_debug_logging` command another to flip [`Self::set_enabled`], and they
/// see the same atomic flag and path.
#[derive(Debug)]
pub struct ResponseLog {
    enabled: AtomicBool,
    /// Destination file, or `None` when no log directory could be resolved — in
    /// which case logging is a no-op regardless of the toggle.
    path: Option<PathBuf>,
}

impl ResponseLog {
    /// A log writing to `path` (when some), starting in the given enabled state.
    #[must_use]
    pub const fn new(path: Option<PathBuf>, enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            path,
        }
    }

    /// A permanently-off log with no destination, for tests and for the
    /// transport constructors that predate response logging.
    #[must_use]
    pub const fn disabled() -> Self {
        Self::new(None, false)
    }

    /// Turn logging on or off (the Settings toggle). Takes effect on the next
    /// recorded response.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// The log file path, for display in Settings. `None` when no log directory
    /// was resolvable at startup.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Append one response to the log, framed with a timestamp and the endpoint
    /// it came from. No-op unless logging is enabled and a path is set. Any I/O
    /// error is intentionally swallowed: capturing a debug trace must never
    /// perturb polling.
    pub fn record(&self, endpoint: &str, body: &str) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        if let Some(path) = self.path.as_deref() {
            let _ = append_entry(path, endpoint, body);
        }
    }
}

/// Append a framed `<header>\n<body>\n\n` entry to `path`, resetting the file
/// first if it has grown past [`MAX_LOG_BYTES`]. Creates the parent directory
/// and the file (`0600`) as needed.
fn append_entry(path: &Path, endpoint: &str, body: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let over_cap = fs::metadata(path).is_ok_and(|meta| meta.len() > MAX_LOG_BYTES);
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    if over_cap {
        options.truncate(true);
    } else {
        options.append(true);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        // Best-effort tighten to owner-only; ignore if the platform/filesystem
        // refuses (the log is non-critical either way).
        let _ = file.set_permissions(fs::Permissions::from_mode(0o600));
    }
    let entry = format!(
        "===== {timestamp} {endpoint} =====\n{body}\n\n",
        timestamp = Timestamp::now(),
        body = body.trim_end(),
    );
    file.write_all(entry.as_bytes())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn disabled_log_never_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG_FILE);
        let log = ResponseLog::new(Some(path.clone()), false);
        log.record("usage", "{\"spend\":123}");
        assert!(!path.exists(), "a disabled log must not create the file");
    }

    #[test]
    fn enabled_log_appends_framed_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(LOG_FILE);
        let log = ResponseLog::new(Some(path.clone()), true);
        log.record("usage", "{\"a\":1}");
        log.record("usage", "{\"b\":2}");
        let contents = fs::read_to_string(&path).unwrap();
        assert_eq!(contents.matches("===== ").count(), 2);
        assert!(contents.contains("{\"a\":1}"));
        assert!(contents.contains("{\"b\":2}"));
        assert!(contents.contains("usage ====="));
    }

    #[test]
    fn toggling_enabled_gates_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG_FILE);
        let log = ResponseLog::new(Some(path.clone()), false);
        log.record("usage", "ignored");
        assert!(!path.exists());

        log.set_enabled(true);
        log.record("usage", "kept");
        assert!(path.exists());
        assert!(fs::read_to_string(&path).unwrap().contains("kept"));

        log.set_enabled(false);
        log.record("usage", "dropped-again");
        assert!(!fs::read_to_string(&path).unwrap().contains("dropped-again"));
    }

    #[test]
    fn oversized_log_is_reset_rather_than_grown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG_FILE);
        // Seed a file already past the cap.
        let cap = usize::try_from(MAX_LOG_BYTES).unwrap();
        fs::write(&path, "x".repeat(cap + 1)).unwrap();
        let log = ResponseLog::new(Some(path.clone()), true);
        log.record("usage", "fresh-start");
        let contents = fs::read_to_string(&path).unwrap();
        assert!(
            contents.len() < cap,
            "the log should have been reset, not appended to"
        );
        assert!(contents.contains("fresh-start"));
    }

    #[test]
    fn disabled_constructor_has_no_path_and_stays_off() {
        let log = ResponseLog::disabled();
        assert!(log.path().is_none());
        // Recording is a harmless no-op with no destination (and, being
        // disabled, would not write even if it had one).
        log.record("usage", "nowhere");
    }

    #[cfg(unix)]
    #[test]
    fn log_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG_FILE);
        let log = ResponseLog::new(Some(path.clone()), true);
        log.record("usage", "secret-ish usage figures");
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
