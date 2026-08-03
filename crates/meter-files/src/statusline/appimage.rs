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
//! [`setup::invocation_for`] names. The copy is refreshed whenever it is no
//! longer the one inside the image — different bytes, or no longer executable
//! — so neither an upgrade nor a mode-stripping restore can leave the status
//! line running something that is not the current bridge; `setup::write`
//! already runs on every launch, which is the only reason a per-launch refresh
//! is affordable.
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

/// The other variable the same runtime exports: the directory the image is
/// mounted at, which is what makes [`APPIMAGE_VAR`] believable.
///
/// Both are inherited by every descendant process, so `$APPIMAGE` alone says
/// "something in this process tree came out of an image", not "*this* process
/// did" — an ordinary `.deb` install started from the terminal of an
/// `AppImage`-packaged editor sees the editor's image. Pairing the two turns
/// that into a checkable claim: this executable lives inside that mount.
const APPDIR_VAR: &str = "APPDIR";

/// Directory inside `~/.claudemeter/` holding the extracted bridge.
///
/// `AppImage` installs only — nothing else has anything to extract, and an
/// ordinary install must gain no side effect from this module at all.
pub const BIN_DIR: &str = "bin";

/// The `.AppImage` `exe` was started from, or `None` when it was not started
/// from one.
///
/// `exe` is the running executable, passed in rather than resolved here so the
/// caller's one `current_exe()` answers both this question and the invocation
/// it feeds.
pub fn running_from(exe: &Path) -> Option<PathBuf> {
    image_at(
        exe,
        std::env::var_os(APPDIR_VAR),
        std::env::var_os(APPIMAGE_VAR),
    )
}

/// The decision half of [`running_from`], split out so the rule can be tested
/// against real values. `unsafe` is forbidden workspace-wide and edition 2024
/// makes `set_var` unsafe, so a test that set `$APPIMAGE` could not be written
/// even if process-wide mutation from parallel tests were acceptable — and a
/// test that read the *real* environment would answer differently depending on
/// what the developer launched `cargo test` from, which is the very confusion
/// the [`APPDIR_VAR`] corroboration exists to remove.
///
/// Both facts have to hold. `exe` must sit inside the mount `appdir` names —
/// that is what makes this process, and not merely some ancestor of it, the
/// one that came out of the image — and `image` must be absolute and name an
/// existing file, so a variable left over after the image was deleted degrades
/// to ordinary behaviour rather than producing a command naming a path that is
/// not there. (An empty value fails an absolute test, so neither needs a
/// separate check.)
fn image_at(exe: &Path, appdir: Option<OsString>, image: Option<OsString>) -> Option<PathBuf> {
    let mount = PathBuf::from(appdir?);
    let image = PathBuf::from(image?);
    let ours = mount.is_absolute() && exe.starts_with(&mount);
    (ours && image.is_absolute() && image.is_file()).then_some(image)
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

/// Whether `dest` is already the copy this module makes of `source`: the same
/// bytes, at the same [`EXECUTABLE_MODE`](crate::io::EXECUTABLE_MODE).
///
/// Length first because it settles the answer for almost every mismatch
/// without a read, then the full contents. Not mtime: `fs::copy` does not
/// preserve it and reproducible builds normalise it, so a timestamp answers
/// "when was this copied", not "is this the same bridge" — and getting that
/// wrong in the permissive direction leaves an upgraded app driving the
/// previous release's bridge.
///
/// The mode counts because it is the copy's whole purpose: Claude Code has to
/// *spawn* this file, and a restore that drops mode bits (`rsync` without
/// `-p`, a file-sync client, a tar unpacked under a strict umask) leaves the
/// bytes identical and the status line dead. Content alone would then skip the
/// refresh on every launch thereafter, which is exactly the silent, unfixable
/// failure issue #74 exists to remove.
fn is_current(source: &Path, dest: &Path) -> bool {
    let (Ok(from), Ok(to)) = (fs::metadata(source), fs::metadata(dest)) else {
        return false;
    };
    as_copied(&to)
        && from.len() == to.len()
        && matches!((fs::read(source), fs::read(dest)), (Ok(a), Ok(b)) if a == b)
}

/// Whether `meta` carries the permissions [`atomic_copy`] leaves.
///
/// Equality rather than "has an execute bit somewhere", because the copy's
/// mode is written, not inherited: anything else means something changed it,
/// and re-copying restores an over-permissive one (a world-writable file the
/// status line spawns several times a second) as readily as a stripped one.
#[cfg(unix)]
fn as_copied(meta: &fs::Metadata) -> bool {
    use crate::io::EXECUTABLE_MODE;
    use std::os::unix::fs::PermissionsExt;

    meta.permissions().mode() & 0o777 == EXECUTABLE_MODE
}

/// Off Unix there are no mode bits to lose, so the question is only about the
/// bytes. Neither target platform takes this branch.
#[cfg(not(unix))]
fn as_copied(_meta: &fs::Metadata) -> bool {
    true
}

/// The setup document's `AppImage` bullet: what the recorded command is tied to,
/// and what to do when that changes.
///
/// Worth saying outright because the failure it prevents is silent — the
/// status line simply stops showing a segment — and because the remedy
/// (start the app once, re-run `/statusline`) is not guessable, which is why
/// the last sentence is shared by both forms.
///
/// The middle sentence is not, and takes `extracted` — the copy [`refresh`]
/// was asked to make, as [`bridge_copy`] and then *probed*, so it is `None`
/// exactly when the command above names the image itself. Describing a copy
/// that is not there would send the one user whose extraction failed (a
/// read-only home, an image predating the standalone bridge) looking for a
/// file no amount of restarting will produce — and that user is the one most
/// in need of a document that explains itself. The copy's path is not
/// repeated: it *is* the command printed a few lines above.
pub fn note(appimage: &Path, extracted: Option<&Path>) -> String {
    let form = if extracted.is_some() {
        "It names a copy of the bridge kept outside the image, refreshed
    each time the app starts."
    } else {
        "It runs the AppImage itself, because the bridge could not be
    copied out of it — slower, but it survives a restart."
    };
    format!(
        "  * This command belongs to the AppImage install at
    {image}
    {form}
    If you move, rename or replace the AppImage, start the app once and
    then re-run /statusline against this file: the command above may have
    changed.
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

    /// A copy whose mode has been stripped — a backup restored without mode
    /// bits, a `chmod -R go-x`, a file-sync client — is bytes-identical and
    /// completely useless: Claude Code gets `EACCES` on every redraw. The
    /// per-launch refresh is the only thing that can heal it, so "already a
    /// copy" has to mean the mode too, or restarting the app (what the docs
    /// tell the user to do) fixes nothing, forever.
    #[cfg(unix)]
    #[test]
    fn a_copy_that_has_lost_its_executable_bit_is_made_executable_again() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let from = source(&dir, "bridge v1");
        let dest = bridge_copy(&dir.path().join(BIN_DIR));
        refresh(&from, &dest).unwrap();
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o644)).unwrap();
        refresh(&from, &dest).unwrap();
        let mode = fs::metadata(&dest).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755, "expected 0755, got {mode:o}");
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

    /// An `AppImage` install, staged: the mount root, an executable inside it,
    /// and the image file the runtime names.
    fn mounted(dir: &tempfile::TempDir) -> (PathBuf, PathBuf, PathBuf) {
        let mount = dir.path().join(".mount_RustedZzZ");
        let exe = mount.join("usr/bin/rusted-claude-meter");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, "").unwrap();
        let image = dir.path().join("RustedClaudeMeter.AppImage");
        fs::write(&image, "").unwrap();
        (mount, exe, image)
    }

    /// Detection is opt-in: with neither variable set — every macOS, `.deb`,
    /// AUR and `cargo run` install — nothing about the existing behaviour
    /// changes.
    ///
    /// Asserted over the decision function rather than over the real process
    /// environment, which a contributor running `cargo test` from the terminal
    /// of an `AppImage`-packaged editor does not control.
    #[test]
    fn an_unset_appimage_variable_means_this_is_not_an_appimage_install() {
        let dir = tempfile::tempdir().unwrap();
        let (_, exe, image) = mounted(&dir);
        assert_eq!(image_at(&exe, None, None), None);
        assert_eq!(image_at(&exe, None, Some(image.into())), None);
    }

    /// A variable left over from some earlier `AppImage` must not be believed:
    /// the command would name a file that is not there, which is worse than
    /// the ordinary path it replaced.
    #[test]
    fn a_relative_or_absent_image_path_is_not_believed() {
        let dir = tempfile::tempdir().unwrap();
        let (mount, exe, _) = mounted(&dir);
        let appdir = || Some(OsString::from(&mount));
        assert_eq!(image_at(&exe, appdir(), Some(OsString::from(""))), None);
        assert_eq!(
            image_at(
                &exe,
                appdir(),
                Some(OsString::from("RustedClaudeMeter.AppImage"))
            ),
            None
        );
        assert_eq!(
            image_at(
                &exe,
                appdir(),
                Some(dir.path().join("gone.AppImage").into())
            ),
            None
        );
    }

    #[test]
    fn an_existing_absolute_image_path_is_taken_as_the_install() {
        let dir = tempfile::tempdir().unwrap();
        let (mount, exe, image) = mounted(&dir);
        assert_eq!(
            image_at(&exe, Some(mount.into()), Some(image.clone().into())),
            Some(image)
        );
    }

    /// Both variables are inherited by every descendant, so an ordinary `.deb`
    /// install launched from the terminal of an `AppImage`-packaged editor sees
    /// that editor's image. Believing it would copy `/usr/bin`'s bridge into
    /// `~/.claudemeter/bin/` on an install that promises no such side effect,
    /// freeze the recorded command against a copy no `apt upgrade` touches,
    /// and — where the copy could not be made — pipe Claude Code's JSON into
    /// the editor several times a second.
    #[test]
    fn an_image_this_executable_does_not_live_inside_is_somebody_elses() {
        let dir = tempfile::tempdir().unwrap();
        let (mount, _, image) = mounted(&dir);
        let installed = dir.path().join("usr/bin/rusted-claude-meter");
        assert_eq!(
            image_at(&installed, Some(mount.into()), Some(image.into())),
            None
        );
    }

    /// An empty `$APPDIR` would prefix-match every path there is, which would
    /// make the corroboration above no corroboration at all.
    #[test]
    fn a_relative_or_empty_mount_root_corroborates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (_, exe, image) = mounted(&dir);
        for appdir in ["", "usr"] {
            assert_eq!(
                image_at(
                    &exe,
                    Some(OsString::from(appdir)),
                    Some(image.clone().into())
                ),
                None,
                "$APPDIR={appdir:?}"
            );
        }
    }

    /// The note has to carry the path, or a user with two installs cannot tell
    /// which one the recorded command belongs to.
    #[test]
    fn the_note_names_the_image_and_says_what_to_re_run() {
        let copy = Path::new("/home/you/.claudemeter/bin/rusted-claude-meter-statusline");
        let note = note(
            Path::new("/home/you/Applications/RustedClaudeMeter.AppImage"),
            Some(copy),
        );
        assert!(
            note.contains("/home/you/Applications/RustedClaudeMeter.AppImage"),
            "{note}"
        );
        assert!(note.contains("/statusline"), "{note}");
        assert!(note.starts_with("  * "), "{note}");
        assert!(note.ends_with('\n'), "{note}");
    }

    /// Where the extraction failed there is no copy to go looking for, and a
    /// note that describes one sends the only user who needs this document
    /// after a file that will never appear.
    #[test]
    fn the_note_says_the_image_itself_is_being_run_when_no_copy_was_made() {
        let image = Path::new("/home/you/Applications/RustedClaudeMeter.AppImage");
        let fallback = note(image, None);
        assert!(fallback.contains("runs the AppImage itself"), "{fallback}");
        assert!(!fallback.contains("copy of the bridge"), "{fallback}");
        assert!(fallback.contains("re-run /statusline"), "{fallback}");
        assert!(
            note(image, Some(Path::new("/home/you/.claudemeter/bin/b")))
                .contains("copy of the bridge"),
        );
    }
}
