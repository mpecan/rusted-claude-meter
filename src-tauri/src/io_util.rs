//! Shared atomic-write idiom used by every on-disk contract (`cache.rs`,
//! `settings.rs`, `export.rs`): create the parent directory, write to a
//! sibling `.tmp` file, then rename over the destination. The rename is
//! atomic on the platforms this app targets, so a crash mid-write (or a
//! concurrent read by an external script) can never observe a truncated
//! file.

use std::fs;
use std::io;
use std::path::Path;

use serde::de::DeserializeOwned;

/// Decode a JSON document from `path`, or `None` for any failure — absent,
/// unreadable, or not the shape `T` expects.
///
/// The read half of the idiom [`atomic_write`] is the write half of. Every
/// on-disk file this app owns is optional by design: a missing or damaged one
/// falls back to a default rather than failing the operation that wanted it,
/// so the distinction between "no file" and "bad file" is never acted on and
/// collapsing both to `None` here keeps four call sites from spelling that
/// out individually.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// Write `body` to `path` atomically: `create_dir_all` the parent, write to
/// `path` with a `.tmp`-suffixed extension, then rename over `path`.
///
/// On Unix the file is chmod'ed to `0600` before the rename: usage
/// percentages and notification settings are the owner's business, not
/// something every other account on a shared machine should be able to read
/// (`fs::write` would otherwise leave the default umask-derived mode,
/// typically world-readable `0644`). Same-user consumers — the statusline
/// scripts `export.rs`'s `usage.json` exists for — are unaffected.
pub fn atomic_write(path: &Path, body: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn writes_the_body_and_replaces_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("file.json");
        atomic_write(&path, "first").unwrap();
        atomic_write(&path, "second").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    }

    #[cfg(unix)]
    #[test]
    fn written_files_are_owner_readable_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.json");
        atomic_write(&path, "{}").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "expected 0600, got {mode:o}");
    }
}
