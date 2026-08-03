//! Surviving being run from an `AppImage` (issue #74).
//!
//! An `AppImage` is a squashfs image mounted, by its own runtime, at a fresh
//! `/tmp/.mount_XXXXXX` for the lifetime of the process. Everything inside it
//! — including the standalone bridge binary [`setup::BRIDGE_BIN`] — therefore
//! lives at a path that is **valid only while the app is running**, and a
//! different one on the next launch. Writing that path into
//! `~/.claude/settings.json` produces a status line that works until the app
//! quits and never again.
//!
//! So on an `AppImage` install the bridge is copied *out* of the mount, once per
//! launch, to a stable path under the user's own home
//! (`~/.claudemeter/bin/rusted-claude-meter-statusline`), and **that** is what
//! [`setup::invocation_for`] names. The copy is refreshed whenever it differs
//! from the one inside the image, so upgrading the `AppImage` cannot leave a
//! stale bridge behind; `setup::write` already runs on every launch, which is
//! the only reason a per-launch refresh is affordable.
//!
//! [`setup`]: crate::statusline::setup

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::io::atomic_copy;
use crate::statusline::setup::BRIDGE_BIN;

/// The environment variable an `AppImage`'s runtime exports into the
/// application's environment: the absolute path of the `.AppImage` *file*
/// itself, not the mount.
///
/// This is the detection signal, deliberately rather than pattern-matching
/// `/tmp/.mount_`: the mount prefix is an implementation detail of one
/// runtime version, while this variable is the documented contract, and it is
/// also the only way to name the image for the fallback invocation.
const APPIMAGE_VAR: &str = "APPIMAGE";

/// Directory inside `~/.claudemeter/` holding the extracted bridge.
///
/// `AppImage` installs only — nothing else has anything to extract, and an
/// ordinary install must gain no side effect from this module at all.
pub const BIN_DIR: &str = "bin";

/// The `.AppImage` this process was started from, or `None` when it was not
/// started from one.
///
/// The value is accepted only when it is absolute and names an existing file.
/// A stale exported variable — inherited from a shell that once ran an
/// `AppImage`, or left over after the image was deleted — must degrade to
/// ordinary behaviour rather than produce a command naming a relative or
/// absent path. (An empty value fails the absolute test, so it needs no
/// separate check.)
pub fn running_from() -> Option<PathBuf> {
    image_at(std::env::var_os(APPIMAGE_VAR))
}

/// The decision half of [`running_from`], split out so the rule can be tested
/// against real values. `unsafe` is forbidden workspace-wide and edition 2024
/// makes `set_var` unsafe, so a test that set `$APPIMAGE` could not be written
/// even if process-wide mutation from parallel tests were acceptable.
fn image_at(value: Option<OsString>) -> Option<PathBuf> {
    let path = PathBuf::from(value?);
    (path.is_absolute() && path.is_file()).then_some(path)
}

/// Where the extracted bridge lives, given `~/.claudemeter/bin`.
///
/// The single spelling of that file name: [`setup::write`] reaches this
/// directory from the setup document beside it and `setup::current_invocation`
/// reaches it from `$HOME`, and two spellings of one path is precisely how the
/// bridge once came to read its own recording as its config.
///
/// [`setup::write`]: crate::statusline::setup::write
pub fn bridge_copy(bin_dir: &Path) -> PathBuf {
    bin_dir.join(BRIDGE_BIN)
}

/// Make `dest` a copy of `source`, unless it already is one.
///
/// The skip is what makes this affordable on every launch: the common case is
/// an unchanged install, which then costs a read and no write at all.
pub fn refresh(source: &Path, dest: &Path) -> io::Result<()> {
    if is_current(source, dest) {
        return Ok(());
    }
    atomic_copy(source, dest)
}

/// Whether `dest` already holds exactly the bytes of `source`.
///
/// Length first because it settles the answer for almost every mismatch
/// without a read, then the full contents. Not mtime: `fs::copy` does not
/// preserve it and reproducible builds normalise it, so a timestamp answers
/// "when was this copied", not "is this the same bridge" — and getting that
/// wrong in the permissive direction leaves an upgraded app driving the
/// previous release's bridge.
fn is_current(source: &Path, dest: &Path) -> bool {
    let (Ok(from), Ok(to)) = (fs::metadata(source), fs::metadata(dest)) else {
        return false;
    };
    from.len() == to.len() && matches!((fs::read(source), fs::read(dest)), (Ok(a), Ok(b)) if a == b)
}

/// The setup document's `AppImage` bullet: what the recorded command is tied to,
/// and what to do when that changes.
///
/// Worth saying outright because the failure it prevents is silent — the
/// status line simply stops showing a segment — and because the remedy
/// (start the app once, re-run `/statusline`) is not guessable. One wording
/// covers both the extracted-copy and the `$APPIMAGE` fallback forms: which
/// one is in play is this module's business, and the user's action is the
/// same either way.
pub fn note(appimage: &Path) -> String {
    format!(
        "  * This command belongs to the AppImage install at
    {image}
    It names a copy of the bridge kept outside the image, refreshed each
    time the app starts. If you move, rename or replace the AppImage, start
    the app once and then re-run /statusline against this file: the command
    above may have changed.
",
        image = appimage.display(),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pretty_assertions::assert_eq;

    /// A stand-in for the bridge inside the mounted image.
    fn source(dir: &tempfile::TempDir, body: &str) -> PathBuf {
        let path = dir.path().join("mount").join(BRIDGE_BIN);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }

    /// The inode a path resolves to, which is what a rename-over changes and
    /// a skipped refresh leaves alone.
    #[cfg(unix)]
    fn inode(path: &Path) -> u64 {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(path).unwrap().ino()
    }

    /// The whole point: a bridge inside a FUSE mount is unreachable the moment
    /// the app quits, so it has to end up somewhere the user's home keeps.
    #[test]
    fn the_bridge_is_copied_out_of_the_image_to_the_stable_path() {
        let dir = tempfile::tempdir().unwrap();
        let dest = bridge_copy(&dir.path().join(BIN_DIR));
        refresh(&source(&dir, "bridge v1"), &dest).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "bridge v1");
    }

    /// Claude Code *spawns* this file several times a second; a copy it cannot
    /// execute is no better than the mount path it replaced.
    #[cfg(unix)]
    #[test]
    fn the_extracted_bridge_can_be_executed_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let dest = bridge_copy(&dir.path().join(BIN_DIR));
        refresh(&source(&dir, "bridge v1"), &dest).unwrap();
        let mode = fs::metadata(&dest).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755, "expected 0755, got {mode:o}");
    }

    /// Refreshing runs on every launch, so the unchanged case — which is every
    /// launch but the first after an upgrade — must not rewrite the file.
    #[cfg(unix)]
    #[test]
    fn a_copy_that_already_matches_the_image_is_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let from = source(&dir, "bridge v1");
        let dest = bridge_copy(&dir.path().join(BIN_DIR));
        refresh(&from, &dest).unwrap();
        let first = inode(&dest);
        refresh(&from, &dest).unwrap();
        assert_eq!(inode(&dest), first, "the copy was replaced needlessly");
    }

    /// The reason the comparison reads contents rather than trusting size: two
    /// builds of the same binary are very often the same length, and a stale
    /// bridge is exactly what an upgrade must not leave behind.
    #[cfg(unix)]
    #[test]
    fn a_copy_left_by_an_older_build_is_replaced_even_at_the_same_length() {
        let dir = tempfile::tempdir().unwrap();
        let dest = bridge_copy(&dir.path().join(BIN_DIR));
        refresh(&source(&dir, "bridge v1"), &dest).unwrap();
        let first = inode(&dest);
        refresh(&source(&dir, "bridge v2"), &dest).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "bridge v2");
        assert_ne!(inode(&dest), first, "the stale copy was kept");
    }

    /// An `AppImage` built before issue #72 carries no standalone bridge. The
    /// copy must then simply not happen — which is what routes the generated
    /// command to the `$APPIMAGE` subcommand fallback instead of to a path
    /// with nothing at it.
    #[test]
    fn an_image_with_no_bridge_inside_it_leaves_no_copy_behind() {
        let dir = tempfile::tempdir().unwrap();
        let dest = bridge_copy(&dir.path().join(BIN_DIR));
        assert!(refresh(&dir.path().join("absent"), &dest).is_err());
        assert!(!dest.exists());
    }

    /// Detection is opt-in: with no `$APPIMAGE` in the environment — every
    /// macOS, `.deb`, AUR and `cargo run` install, and this test process —
    /// nothing about the existing behaviour changes.
    #[test]
    fn an_unset_appimage_variable_means_this_is_not_an_appimage_install() {
        assert_eq!(image_at(None), None);
        assert_eq!(running_from(), None);
    }

    /// A variable left over from some earlier `AppImage` must not be believed:
    /// the command would name a file that is not there, which is worse than
    /// the ordinary path it replaced.
    #[test]
    fn a_relative_or_absent_image_path_is_not_believed() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(image_at(Some(OsString::from(""))), None);
        assert_eq!(
            image_at(Some(OsString::from("RustedClaudeMeter.AppImage"))),
            None
        );
        assert_eq!(
            image_at(Some(dir.path().join("gone.AppImage").into())),
            None
        );
    }

    #[test]
    fn an_existing_absolute_image_path_is_taken_as_the_install() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("RustedClaudeMeter.AppImage");
        fs::write(&image, "").unwrap();
        assert_eq!(image_at(Some(image.clone().into())), Some(image));
    }

    /// The note has to carry the path, or a user with two installs cannot tell
    /// which one the recorded command belongs to.
    #[test]
    fn the_note_names_the_image_and_says_what_to_re_run() {
        let note = note(Path::new(
            "/home/you/Applications/RustedClaudeMeter.AppImage",
        ));
        assert!(
            note.contains("/home/you/Applications/RustedClaudeMeter.AppImage"),
            "{note}"
        );
        assert!(note.contains("/statusline"), "{note}");
        assert!(note.starts_with("  * "), "{note}");
        assert!(note.ends_with('\n'), "{note}");
    }
}
