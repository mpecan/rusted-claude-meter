//! The atomic-write idiom every on-disk contract in this app shares.
//!
//! Used by `cache.rs`, `settings.rs` and [`crate::export`] alike: create the
//! parent directory, write to a sibling `.tmp` file, then rename over the
//! destination. The rename is atomic on the platforms this app targets, so a
//! crash mid-write (or a concurrent read by an external script) can never
//! observe a truncated file.
//!
//! [`atomic_copy`] is the same dance for a file whose body comes from another
//! file rather than from a string, and which has to end up executable.

use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;
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

/// Serialize `value` as pretty JSON and [`atomic_write`] it to `path`.
///
/// The three files this app publishes for something else to read
/// (`usage.json`, the recorded status-line reading, and the bridge's config
/// mirror) are all pretty-printed, because a human opening one is a supported
/// way to answer "what does it think my usage is?". The compact writers
/// (`cache`, `settings`) wrap their payload in a version envelope and stay as
/// they are.
pub fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    atomic_write(path, &serde_json::to_string_pretty(value)?)
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
    atomic_replace(path, 0o600, |tmp| fs::write(tmp, body))
}

/// Copy `source` over `dest` atomically, leaving it owner-executable (`0755`).
///
/// The third form of the idiom, for the one thing this app puts on disk that
/// is not a document: the status-line bridge extracted out of an `AppImage`'s
/// FUSE mount (`statusline::appimage`). Claude Code has to be able to *spawn*
/// it, so `atomic_write`'s `0600` would not do; and because the rename is what
/// publishes it, a launch that dies mid-copy leaves the previous bridge in
/// place rather than a truncated binary the status line would try to execute.
///
/// Renaming over a file that is currently running is safe on both target
/// platforms — the running process keeps the old inode — so an upgrade can
/// refresh the copy while a status-line render is in flight.
pub fn atomic_copy(source: &Path, dest: &Path) -> io::Result<()> {
    atomic_replace(dest, 0o755, |tmp| fs::copy(source, tmp).map(drop))
}

/// The shared dance: `create_dir_all` the parent, let `fill` produce the temp
/// file, chmod it to `mode`, then rename over `path`.
///
/// The `json` in the temp name is the idiom's, not a claim about the content —
/// it is what three existing tests assert the *absence* of after a successful
/// write, so it stays as it is even for the binary copy.
fn atomic_replace(
    path: &Path,
    mode: u32,
    fill: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
    #[cfg(not(unix))]
    let _ = mode;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fill(&tmp)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
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

    /// The extracted bridge lands in `~/.claudemeter/bin/`, a directory that
    /// does not exist on any install until the first `AppImage` launch — so the
    /// copy has to create it, exactly as the writer does.
    #[test]
    fn copies_a_file_into_place_creating_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("bridge");
        fs::write(&source, "binary").unwrap();
        let dest = dir.path().join("bin").join("bridge");
        atomic_copy(&source, &dest).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "binary");
        assert!(!dest.with_extension("json.tmp").exists());
    }

    /// The one file this app writes that something else has to *run*: Claude
    /// Code spawns it on every status-line redraw, so `atomic_write`'s `0600`
    /// would leave a command that cannot be executed at all.
    #[cfg(unix)]
    #[test]
    fn copied_executables_are_runnable_by_their_owner() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("bridge");
        fs::write(&source, "binary").unwrap();
        // Deliberately not executable at the source: an AppImage's payload is
        // read through a FUSE mount whose modes are not ours to rely on.
        fs::set_permissions(&source, fs::Permissions::from_mode(0o644)).unwrap();
        let dest = dir.path().join("bridge-copy");
        atomic_copy(&source, &dest).unwrap();
        let mode = fs::metadata(&dest).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755, "expected 0755, got {mode:o}");
    }

    /// Publishing by rename is what makes a half-written binary unobservable:
    /// a copy that cannot be made must leave the previous, working bridge
    /// exactly where it was rather than truncating it.
    #[test]
    fn a_copy_that_cannot_be_made_leaves_the_previous_file_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("bridge");
        fs::write(&dest, "the bridge that works").unwrap();
        assert!(atomic_copy(&dir.path().join("absent"), &dest).is_err());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "the bridge that works");
    }
}
