//! Getting the bridge into the user's status line with as little manual
//! editing as possible.
//!
//! Claude Code ships a `/statusline` command that edits
//! `~/.claude/settings.json` for the user. The agent behind it can **read
//! files and edit them, and nothing else** — it cannot run a binary to ask
//! where that binary lives. So handing it an instruction is not enough; it
//! needs a *file* holding the finished command.
//!
//! That is what [`SETUP_FILE`] is. The app writes it on every launch (the
//! executable can move between launches), and the user runs the `/statusline`
//! invocation from `src/usage-source.ts` in Claude Code, which points the
//! agent straight at it.
//!
//! The document is also written for a human reading it directly — the same
//! facts serve both, and a file that only made sense to an agent would be a
//! trap for anyone who opened it.
//!
//! *Which* bridge it names is decided per install by [`invocation_for`],
//! which is why the file is rewritten on every launch rather than once: an
//! install can gain the standalone binary (or move) between two runs — and on
//! an `AppImage` it moves on *every* run, which is what
//! [`crate::statusline::appimage`] exists to handle.

use std::io;
use std::path::{Path, PathBuf};

use crate::io::atomic_write;
use crate::statusline::SUBCOMMAND;
use crate::statusline::appimage;

/// File name inside [`crate::export::EXPORT_DIR`], beside `usage.json` and
/// `statusline.json`.
///
/// **Mirrored in `src/usage-source.ts`**, whose `STATUSLINE_SLASH_COMMAND`
/// names this file so the read-only `/statusline` agent can find it — rename
/// them together, exactly like `pacing.rs`/`pacing.ts`. The name lives there
/// rather than being plumbed through IPC because it is part of a sentence the
/// user reads, and all such copy lives in the frontend.
pub const SETUP_FILE: &str = "statusline-command.txt";

/// Used only when the running executable's own path cannot be resolved.
/// Correct where it works — a packaged Linux install puts the binary on
/// `PATH` — and a legible placeholder where it does not.
const FALLBACK_EXE: &str = "rusted-claude-meter";

/// The standalone bridge binary (issue #72), installed beside the GUI one.
///
/// `tauri-bundler` copies every `[[bin]]` in the package into
/// `Contents/MacOS/` on macOS and `/usr/bin/` in the `.deb` that the
/// `AppImage` and the AUR package are both built from.
///
/// **Mirrored in `src-tauri/Cargo.toml`'s second `[[bin]]`** — rename them
/// together, or this resolves to a file that is not there and every setup
/// silently falls back to the slower subcommand.
pub const BRIDGE_BIN: &str = "rusted-claude-meter-statusline";

/// Single-quote `value` for POSIX `sh`, so a path containing spaces — every
/// macOS install — survives being pasted into a shell command. An embedded
/// single quote is closed, escaped and reopened, the standard idiom.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Everything the decision below needs to know about where this app is
/// installed. Named fields rather than a tuple of `Option<&Path>`s, which no
/// call site could transpose safely.
#[derive(Debug, Clone, Copy)]
struct Install<'a> {
    /// The running GUI binary. Answers [`SUBCOMMAND`] indefinitely.
    exe: &'a Path,
    /// [`BRIDGE_BIN`] beside `exe`, when it is really there.
    standalone: Option<&'a Path>,
    /// The `.AppImage` file, when the app was started from one.
    appimage: Option<&'a Path>,
    /// The bridge copied out of that image, when the copy is really there.
    extracted: Option<&'a Path>,
}

/// Render the pipe target: a standalone bridge where one is reachable,
/// otherwise an executable plus the subcommand it answers to.
///
/// The `AppImage` case swaps **both** candidates at once rather than adding a
/// branch to either. Inside an image, `exe` and `standalone` both point into a
/// `/tmp/.mount_…` directory that ceases to exist the moment the app quits, so
/// neither may be named: the durable pair — the extracted copy, and the image
/// file itself for the alias — replaces the ephemeral one wholesale, which
/// makes the mount path unnameable by construction. With `appimage: None` this
/// is character-for-character the expression that shipped before issue #74.
///
/// Pure, and separate from [`invocation_for`] purely so it can be tested
/// against realistic absolute paths. The probes below are the *only* part that
/// touches the filesystem, and a test that both names a plausible install
/// path and hits the real disk answers differently depending on what the
/// developer happens to have installed — which is exactly how this split came
/// about.
fn invocation(install: Install<'_>) -> String {
    let (bridge, alias) = install
        .appimage
        .map_or((install.standalone, install.exe), |image| {
            (install.extracted, image)
        });
    bridge.map_or_else(
        || format!("{} {SUBCOMMAND}", shell_quote(&alias.display().to_string())),
        |bridge| shell_quote(&bridge.display().to_string()),
    )
}

/// A candidate bridge, but only if something is actually there. A directory
/// named like the bridge is not the bridge, hence `is_file` over `exists`.
fn bridge_at(path: &Path) -> Option<&Path> {
    path.is_file().then_some(path)
}

/// How to reach the bridge from an install whose GUI binary is `exe`: a
/// shell-quoted executable, plus the subcommand when that executable is the
/// GUI one standing in for the standalone bridge.
///
/// [`BRIDGE_BIN`] is preferred whenever it is actually there. Claude Code
/// spawns this on every status-line redraw, and the GUI binary makes dyld map
/// `AppKit`, `WebKit`, Carbon, `CloudKit` and `QuartzCore` first — more than twice
/// the bridge's own work, for frameworks it never touches (issue #72).
///
/// Probing the filesystem rather than assuming keeps one code path honest
/// everywhere: it picks the fast binary from an `.app`, a `.deb` and a
/// `cargo run` target directory alike, and degrades to the subcommand — which
/// is kept working indefinitely — anywhere it is missing, rather than writing
/// a command that names a file that is not there.
///
/// `extracted` gets the same probe as the sibling, and for the same reason:
/// naming an already-present copy is a *probe*, never a consequence of this
/// launch's copy having succeeded, so the Settings copy button and the setup
/// document cannot disagree about what this install can run.
#[must_use]
pub fn invocation_for(exe: &Path, appimage: Option<&Path>, extracted: Option<&Path>) -> String {
    let standalone = exe.with_file_name(BRIDGE_BIN);
    invocation(Install {
        exe,
        standalone: bridge_at(&standalone),
        appimage,
        extracted: extracted.and_then(bridge_at),
    })
}

/// A complete `statusLine.command` driving the bridge, given an
/// [`invocation_for`] an install.
///
/// The shape is the *component* one — capture stdin once, pipe a copy through
/// the bridge, keep the result in `$meter` — so a user who already has a
/// status line replaces only the final `printf`. Claude Code hands its blob to
/// one command and one command only, so composing is the normal case.
#[must_use]
pub fn command_for(invocation: &str) -> String {
    format!("input=$(cat); meter=$(printf '%s' \"$input\" | {invocation}); printf '%s' \"$meter\"")
}

/// The running executable's path — the thing a user could not guess,
/// especially on macOS where it sits inside the app bundle.
fn current_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from(FALLBACK_EXE))
}

/// [`invocation_for`] this install — the one place the running executable, the
/// environment and the bridge-or-alias decision are combined, so
/// [`current_command`] and [`write`] cannot answer it differently.
///
/// Probes for the extracted copy; it never *makes* one. This is reached from
/// an IPC handler (Settings' "Copy command"), and extracting a binary is
/// launch work, not something a button press should trigger.
fn current_invocation() -> String {
    let exe = current_exe();
    let image = appimage::running_from(&exe);
    let extracted = image
        .is_some()
        .then(|| super::home_path(appimage::BIN_DIR))
        .flatten()
        .map(|bin_dir| appimage::bridge_copy(&bin_dir));
    invocation_for(&exe, image.as_deref(), extracted.as_deref())
}

/// [`command_for`] against this install.
#[must_use]
pub fn current_command() -> String {
    command_for(&current_invocation())
}

/// The document written to [`SETUP_FILE`].
///
/// Written so `/statusline` cannot get the composition wrong: the two failure
/// modes are replacing an existing status line and reading stdin twice (which
/// hangs), so both are called out rather than left to be inferred.
///
/// `note` is `appimage::note` on an `AppImage` install, because that one has a
/// third failure mode the others do not: the command outlives only *this*
/// image file. Rendered by the caller rather than decided here — which of its
/// two forms applies depends on whether the extraction succeeded, which is
/// `statusline::appimage`'s business and not this document's. Empty everywhere
/// else, where the document is then byte-for-byte the one that shipped before
/// issue #74.
#[must_use]
pub fn document(invocation: &str, note: &str) -> String {
    format!(
        "Rusted Claude Meter — Claude Code status line
============================================

This machine's command for showing Claude plan usage in the Claude Code
status line. It reads the status-line JSON on stdin, records the reading for
Rusted Claude Meter, and prints one short segment such as \"5h 14% · 7d 3%\".

    {command}

Adding it to \"statusLine\" in ~/.claude/settings.json:

  * If there is no \"statusLine\" yet, use the command above exactly as it is.

  * If there is one already, KEEP IT. Claude Code passes its JSON to
    exactly one command, so this must be added to the existing command
    rather than replacing it. Capture stdin once into \"$input\", pipe a
    copy to the meter, and put \"$meter\" where the segment should appear:

        input=$(cat)
        meter=$(printf '%s' \"$input\" | {invocation})
        printf '%s %s' \"<whatever the existing line printed>\" \"$meter\"

    Do not read stdin twice — the second read has nothing to read and will
    hang.

Notes:

{note}  * The meter prints nothing when Claude Code reports no usage: a cold
    session, or an API-key/Bedrock/Vertex login, which carry no plan limits.
    The rest of the status line is unaffected.
  * Add \" --pace\" to the end of the piped command to append an off-pace
    signal when one is worth showing, e.g. \"5h 95% \u{b7} 7d 40% \u{b7} \u{1f525}1.6\u{d7}\" for
    burning quota faster than the window replenishes it, or \u{2744}\u{fe0f} for leaving
    it unspent. Silent when the burn rate is unremarkable.
  * Add \" --quiet\" the same way to record the reading without printing a
    segment.
  * Requires Claude Code 2.1.216 or newer; older versions send no rate limits.

This file is rewritten by Rusted Claude Meter on every launch. Edit the
status line, not this file.
",
        command = command_for(invocation),
    )
}

/// The document for an install, extracting the bridge from the image first
/// when there is one to extract.
///
/// `claudemeter` is the `~/.claudemeter/` directory — derived by [`write`]
/// from the setup file's own parent, which is what keeps its tests hermetic
/// and what lets `src-tauri` go on passing a single path.
///
/// The extraction is best-effort on purpose: a read-only home, or an image
/// built before the standalone bridge existed, costs the *fast* path only.
/// [`invocation_for`] then finds no copy and names the image plus the
/// subcommand, which is slower but durable — the point of issue #74 being that
/// a durable slow command beats a fast dead one.
fn document_for(exe: &Path, appimage: Option<&Path>, claudemeter: Option<&Path>) -> String {
    let copy = appimage
        .and(claudemeter)
        .map(|dir| appimage::bridge_copy(&dir.join(appimage::BIN_DIR)));
    if let Some(dest) = copy.as_deref()
        && let Err(error) = appimage::refresh(&exe.with_file_name(BRIDGE_BIN), dest)
    {
        eprintln!("could not extract the status-line bridge from the AppImage: {error}");
    }
    // Probed once and used twice: the command and the note that explains it
    // must describe the same install, or the document argues with itself.
    let extracted = copy.as_deref().and_then(bridge_at);
    document(
        &invocation_for(exe, appimage, extracted),
        &appimage.map_or_else(String::new, |image| appimage::note(image, extracted)),
    )
}

/// Write the setup document for this install. Best-effort: a failure here
/// costs a convenience, never the app, so callers log and continue — exactly
/// how `export.rs`'s write is treated.
pub fn write(path: &Path) -> io::Result<()> {
    let exe = current_exe();
    write_for(path, &exe, appimage::running_from(&exe).as_deref())
}

/// The half of [`write`] that is a decision rather than a reading of this
/// process, so the whole of it can be exercised against a staged install.
///
/// `~/.claudemeter/` is derived from `path`'s own parent rather than passed
/// alongside it: the setup document and the extracted bridge live in the same
/// directory by construction, and a second parameter naming it is a second
/// chance to name a different one. That leaves [`write`] itself two calls into
/// the environment and nothing else — the only lines here a test cannot reach.
fn write_for(path: &Path, exe: &Path, appimage: Option<&Path>) -> io::Result<()> {
    atomic_write(path, &document_for(exe, appimage, path.parent()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::export::claudemeter_path;
    use pretty_assertions::assert_eq;

    /// An install with no standalone binary beside it — the subcommand
    /// fallback, and the shape every assertion below that predates
    /// [`BRIDGE_BIN`] was written against.
    ///
    /// Deliberately the *pure* [`invocation`], not [`invocation_for`]: these
    /// cases name plausible install paths, and on a machine where Rusted
    /// Claude Meter is actually installed at one of them the probe would find
    /// a real sibling and answer the other way. Whether the probe finds the
    /// bridge is its own set of tests, against temp directories.
    fn aliased(exe: &str) -> String {
        invocation(Install {
            exe: Path::new(exe),
            standalone: None,
            appimage: None,
            extracted: None,
        })
    }

    /// An `AppImage` install, described rather than staged: the two durable
    /// candidates plus the two ephemeral ones inside the mount, so a test can
    /// say which of them exist without touching this developer's disk.
    fn from_image(image: &str, extracted: Option<&str>) -> String {
        invocation(Install {
            exe: Path::new("/tmp/.mount_RustedZzZ/usr/bin/rusted-claude-meter"),
            standalone: Some(Path::new(
                "/tmp/.mount_RustedZzZ/usr/bin/rusted-claude-meter-statusline",
            )),
            appimage: Path::new(image).into(),
            extracted: extracted.map(Path::new),
        })
    }

    /// A GUI binary with the standalone bridge installed next to it, as every
    /// bundle now ships.
    fn installed(dir: &tempfile::TempDir) -> (PathBuf, PathBuf) {
        let gui = dir.path().join(FALLBACK_EXE);
        let bridge = dir.path().join(BRIDGE_BIN);
        std::fs::write(&gui, "").unwrap();
        std::fs::write(&bridge, "").unwrap();
        (gui, bridge)
    }

    /// Pins the name `src/usage-source.ts` hardcodes into the `/statusline`
    /// invocation; renaming one without the other silently points the agent
    /// at a file that is not there.
    #[test]
    fn the_setup_file_name_matches_the_one_the_frontend_points_at() {
        assert_eq!(SETUP_FILE, "statusline-command.txt");
    }

    /// The whole point of issue #72: when the standalone bridge is installed,
    /// the generated command must drive *it* — not the GUI binary, which
    /// makes dyld map `AppKit` and `WebKit` before reading a byte of stdin.
    #[test]
    fn the_standalone_bridge_is_used_when_it_sits_beside_the_gui_binary() {
        let dir = tempfile::tempdir().unwrap();
        let (gui, bridge) = installed(&dir);
        let invocation = invocation_for(&gui, None, None);
        // The whole invocation is the quoted path and nothing else — no
        // trailing subcommand, and the GUI binary is not named at all.
        assert_eq!(invocation, shell_quote(&bridge.display().to_string()));
    }

    /// Where it is absent — a build predating it, or a hand-copied binary —
    /// the subcommand still does the job, so the command written is a working
    /// one rather than a path to nothing.
    #[test]
    fn without_it_the_gui_binarys_subcommand_is_named_instead() {
        let dir = tempfile::tempdir().unwrap();
        let gui = dir.path().join(FALLBACK_EXE);
        std::fs::write(&gui, "").unwrap();
        let invocation = invocation_for(&gui, None, None);
        assert!(
            invocation.ends_with(&format!(" {SUBCOMMAND}")),
            "{invocation}"
        );
        assert!(
            invocation.contains(&gui.display().to_string()),
            "{invocation}"
        );
    }

    /// A directory named like the bridge is not the bridge. `is_file` rather
    /// than `exists` is what makes that true.
    #[test]
    fn a_directory_by_that_name_does_not_count_as_the_bridge() {
        let dir = tempfile::tempdir().unwrap();
        let gui = dir.path().join(FALLBACK_EXE);
        std::fs::write(&gui, "").unwrap();
        std::fs::create_dir(dir.path().join(BRIDGE_BIN)).unwrap();
        assert!(invocation_for(&gui, None, None).contains(SUBCOMMAND));
    }

    /// Every macOS install has spaces in its path ("Rusted Claude
    /// Meter.app"), so an unquoted path would be a broken paste for most
    /// users.
    #[test]
    fn a_path_with_spaces_is_quoted() {
        assert!(
            command_for(&aliased(
                "/Applications/Rusted Claude Meter.app/Contents/MacOS/rcm"
            ))
            .contains("'/Applications/Rusted Claude Meter.app/Contents/MacOS/rcm'")
        );
    }

    #[test]
    fn an_embedded_single_quote_is_escaped_rather_than_ending_the_string() {
        assert!(command_for(&aliased("/home/o'brien/rcm")).contains(r"'/home/o'\''brien/rcm'"));
    }

    #[test]
    fn the_command_reads_stdin_once_and_leaves_the_segment_in_meter() {
        let command = command_for(&aliased("/usr/bin/rcm"));
        assert!(command.starts_with("input=$(cat);"), "{command}");
        assert_eq!(command.matches("$(cat)").count(), 1, "{command}");
        assert!(command.contains("$meter"), "{command}");
        assert!(command.contains(SUBCOMMAND), "{command}");
    }

    #[test]
    fn the_current_command_drives_whatever_this_install_actually_has() {
        // Under `cargo test` this is the test binary, which is the point:
        // whatever is running is what gets written.
        let command = current_command();
        assert!(command.contains(&current_invocation()), "{command}");
        assert!(command.contains("$meter"), "{command}");
    }

    /// The document exists to stop `/statusline` making the two mistakes that
    /// would break a working status line, so both must be stated outright.
    #[test]
    fn the_document_warns_against_replacing_and_against_double_reading_stdin() {
        let doc = document(&aliased("/usr/bin/rcm"), "");
        assert!(doc.contains("KEEP IT"), "{doc}");
        assert!(doc.contains("exactly one command"), "{doc}");
        assert!(doc.contains("Do not read stdin twice"), "{doc}");
    }

    #[test]
    fn the_document_embeds_the_command_verbatim() {
        let invocation = aliased("/usr/bin/rcm");
        assert!(document(&invocation, "").contains(&command_for(&invocation)));
    }

    /// The merge example must carry the real quoted path too, or an agent
    /// splicing it into an existing status line writes a placeholder.
    #[test]
    fn the_merge_example_carries_the_real_path_not_a_placeholder() {
        let doc = document(
            &aliased("/Applications/Rusted Claude Meter.app/Contents/MacOS/rcm"),
            "",
        );
        assert!(
            !doc.contains('<') || !doc.contains("path from the command"),
            "{doc}"
        );
        assert_eq!(
            doc.matches("'/Applications/Rusted Claude Meter.app/Contents/MacOS/rcm'")
                .count(),
            2,
            "{doc}"
        );
    }

    /// The flag notes must anchor to the command rather than to the
    /// subcommand word: on the standalone bridge there is no such word, and
    /// "add --pace after `statusline`" would point at nothing.
    #[test]
    fn the_flag_notes_anchor_to_the_command_not_to_the_subcommand() {
        let dir = tempfile::tempdir().unwrap();
        let (gui, _) = installed(&dir);
        for doc in [
            document(&invocation_for(&gui, None, None), ""),
            document(&aliased("/x/rcm"), ""),
        ] {
            assert!(doc.contains("--pace"), "{doc}");
            assert!(doc.contains("--quiet"), "{doc}");
            assert!(doc.contains("to the end of the piped command"), "{doc}");
        }
    }

    #[test]
    fn the_document_pins_the_claude_code_version_floor() {
        // Below this the payload carries no `rate_limits` at all, so a user
        // would wire everything up correctly and see nothing.
        assert!(document(&aliased("/usr/bin/rcm"), "").contains("2.1.216"));
    }

    #[test]
    fn writing_creates_the_file_and_replaces_a_previous_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = claudemeter_path(dir.path(), SETUP_FILE);
        write(&path).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        assert!(first.contains("$meter"));
        write(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
        assert!(!path.with_extension("json.tmp").exists());
    }

    /// The single most important regression issue #74 must not cause. Every
    /// macOS, `.deb`, AUR and `cargo run` install goes down this branch, and
    /// the command it produces has to be the one that shipped before — so it
    /// is pinned as exact bytes rather than as a `contains`.
    #[test]
    fn a_non_appimage_install_generates_exactly_the_command_it_always_has() {
        assert_eq!(
            invocation(Install {
                exe: Path::new("/opt/rcm/rusted-claude-meter"),
                standalone: Some(Path::new("/opt/rcm/rusted-claude-meter-statusline")),
                appimage: None,
                extracted: None,
            }),
            "'/opt/rcm/rusted-claude-meter-statusline'"
        );
        assert_eq!(
            aliased("/opt/rcm/rusted-claude-meter"),
            "'/opt/rcm/rusted-claude-meter' statusline"
        );
    }

    /// Inside an `AppImage` the sibling bridge *is* there — at a
    /// `/tmp/.mount_…` path that stops existing the moment the app quits. So
    /// its presence must not tempt the decision: naming it is precisely the
    /// bug (issue #74), and a status line that works until the next restart is
    /// worse than one that is obviously wrong straight away.
    #[test]
    fn an_appimage_install_names_the_extracted_copy_and_never_the_mount_path() {
        let invocation = from_image(
            "/home/you/Applications/RustedClaudeMeter.AppImage",
            Some("/home/you/.claudemeter/bin/rusted-claude-meter-statusline"),
        );
        assert_eq!(
            invocation,
            "'/home/you/.claudemeter/bin/rusted-claude-meter-statusline'"
        );
        assert!(!invocation.contains("/tmp/.mount_"), "{invocation}");
        assert!(!invocation.contains(".AppImage"), "{invocation}");
    }

    /// With no copy to name — an image predating the standalone bridge, or a
    /// home that could not be written — the durable thing left to point at is
    /// the image file itself, driving the compatibility subcommand. Slower,
    /// but it survives a restart, which the mount path does not.
    #[test]
    fn an_appimage_install_with_no_extracted_copy_runs_the_image_itself_with_the_subcommand() {
        let invocation = from_image("/home/you/Applications/RustedClaudeMeter.AppImage", None);
        assert_eq!(
            invocation,
            "'/home/you/Applications/RustedClaudeMeter.AppImage' statusline"
        );
        assert!(!invocation.contains("/tmp/.mount_"), "{invocation}");
    }

    /// The copy is *probed*, exactly like the sibling bridge, so one deleted
    /// between launches falls back to a working command rather than leaving a
    /// path to nothing in `~/.claude/settings.json`.
    #[test]
    fn an_extracted_copy_that_is_not_on_disk_is_not_named() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("RustedClaudeMeter.AppImage");
        let gone = dir.path().join(appimage::BIN_DIR).join(BRIDGE_BIN);
        let invocation = invocation_for(&dir.path().join(FALLBACK_EXE), Some(&image), Some(&gone));
        assert!(
            invocation.ends_with(&format!(" {SUBCOMMAND}")),
            "{invocation}"
        );
        assert!(
            invocation.contains(&image.display().to_string()),
            "{invocation}"
        );
    }

    /// End-to-end for option 3: the bridge inside the image is copied to
    /// `~/.claudemeter/bin/`, and the document names that copy — the one file
    /// on this machine that will still be there tomorrow.
    #[test]
    fn writing_on_an_appimage_install_extracts_the_bridge_and_names_the_copy() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount/usr/bin");
        std::fs::create_dir_all(&mount).unwrap();
        std::fs::write(mount.join(BRIDGE_BIN), "the bridge").unwrap();
        let image = dir.path().join("RustedClaudeMeter.AppImage");
        std::fs::write(&image, "").unwrap();
        let claudemeter = dir.path().join(".claudemeter");

        let doc = document_for(
            &mount.join(FALLBACK_EXE),
            Some(&image),
            Some(claudemeter.as_path()),
        );

        let copy = claudemeter.join(appimage::BIN_DIR).join(BRIDGE_BIN);
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "the bridge");
        assert!(doc.contains(&copy.display().to_string()), "{doc}");
        assert!(!doc.contains("/mount/usr/bin"), "{doc}");
    }

    /// Everything [`write`] adds to that is two readings of this process, and
    /// the rest — extracting into the directory the document itself lives in —
    /// had no test of its own: passing `None` for the image, or a directory
    /// from somewhere else, left every assertion above green while the shipped
    /// `AppImage` went back to recording its mount path. So the seam sits one
    /// call out, and this walks it end to end.
    #[test]
    fn writing_extracts_into_the_directory_the_document_itself_lives_in() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount/usr/bin");
        std::fs::create_dir_all(&mount).unwrap();
        std::fs::write(mount.join(BRIDGE_BIN), "the bridge").unwrap();
        let image = dir.path().join("RustedClaudeMeter.AppImage");
        std::fs::write(&image, "").unwrap();
        let path = claudemeter_path(dir.path(), SETUP_FILE);

        write_for(&path, &mount.join(FALLBACK_EXE), Some(&image)).unwrap();

        let copy = path
            .parent()
            .unwrap()
            .join(appimage::BIN_DIR)
            .join(BRIDGE_BIN);
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "the bridge");
        let doc = std::fs::read_to_string(&path).unwrap();
        assert!(doc.contains(&copy.display().to_string()), "{doc}");
        assert!(!doc.contains("/mount/usr/bin"), "{doc}");
    }

    /// When the extraction fails — an image predating the standalone bridge,
    /// a read-only home — the command falls back to the image itself, and the
    /// note beside it has to describe *that* install. A note promising a copy
    /// in `~/.claudemeter/bin/` sends the one user whose setup went wrong
    /// looking for a file no restart will ever produce.
    #[test]
    fn a_failed_extraction_is_described_as_the_image_running_itself() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("RustedClaudeMeter.AppImage");
        std::fs::write(&image, "").unwrap();
        let claudemeter = dir.path().join(".claudemeter");

        // Nothing to extract: the mount holds no bridge binary at all.
        let doc = document_for(
            &dir.path().join("mount/usr/bin").join(FALLBACK_EXE),
            Some(&image),
            Some(claudemeter.as_path()),
        );

        assert!(doc.contains("runs the AppImage itself"), "{doc}");
        assert!(!doc.contains("copy of the bridge"), "{doc}");
        assert!(
            doc.contains(&format!("{}' {SUBCOMMAND}", image.display())),
            "{doc}"
        );
    }

    /// The other half of that: an ordinary install must gain no side effect at
    /// all from issue #74 — no directory, no copy, no extra work per launch.
    #[test]
    fn nothing_is_extracted_on_an_ordinary_install() {
        let dir = tempfile::tempdir().unwrap();
        let claudemeter = dir.path().join(".claudemeter");
        document_for(
            &dir.path().join(FALLBACK_EXE),
            None,
            Some(claudemeter.as_path()),
        );
        assert!(!claudemeter.join(appimage::BIN_DIR).exists());
    }

    /// An `AppImage` install has a failure mode the others do not — the command
    /// belongs to *this* image file — and it is silent when it happens, so the
    /// document has to say so before it does.
    #[test]
    fn the_document_warns_that_an_appimage_command_is_tied_to_this_install() {
        let image = Path::new("/home/you/Applications/RustedClaudeMeter.AppImage");
        let doc = document(&aliased("/x/rcm"), &appimage::note(image, None));
        assert!(doc.contains(&image.display().to_string()), "{doc}");
        assert!(doc.contains("re-run /statusline"), "{doc}");
    }

    /// And says nothing about `AppImage`s anywhere else, which is what keeps the
    /// document every other install reads byte-identical to before.
    #[test]
    fn the_document_says_nothing_about_appimages_on_an_ordinary_install() {
        assert!(!document(&aliased("/usr/bin/rcm"), "").contains("AppImage"));
    }
}
