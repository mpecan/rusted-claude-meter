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
//! install can gain the standalone binary (or move) between two runs.

use std::io;
use std::path::{Path, PathBuf};

use crate::io::atomic_write;
use crate::statusline::SUBCOMMAND;

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

/// Render the pipe target: the standalone bridge when `standalone` is where
/// it was found, otherwise `exe` plus the subcommand it answers to.
///
/// Pure, and separate from [`invocation_for`] purely so it can be tested
/// against realistic absolute paths. The probe below is the *only* part that
/// touches the filesystem, and a test that both names a plausible install
/// path and hits the real disk answers differently depending on what the
/// developer happens to have installed — which is exactly how this split came
/// about.
fn invocation(exe: &Path, standalone: Option<&Path>) -> String {
    standalone.map_or_else(
        || format!("{} {SUBCOMMAND}", shell_quote(&exe.display().to_string())),
        |bridge| shell_quote(&bridge.display().to_string()),
    )
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
#[must_use]
pub fn invocation_for(exe: &Path) -> String {
    let standalone = exe.with_file_name(BRIDGE_BIN);
    invocation(exe, standalone.is_file().then_some(standalone.as_path()))
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

/// [`invocation_for`] this install — the one place the running executable and
/// the bridge-or-alias decision are combined, so [`current_command`] and
/// [`write`] cannot answer it differently.
fn current_invocation() -> String {
    invocation_for(&current_exe())
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
#[must_use]
pub fn document(invocation: &str) -> String {
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

  * The meter prints nothing when Claude Code reports no usage: a cold
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

/// Write the setup document for this install. Best-effort: a failure here
/// costs a convenience, never the app, so callers log and continue — exactly
/// how `export.rs`'s write is treated.
pub fn write(path: &Path) -> io::Result<()> {
    atomic_write(path, &document(&current_invocation()))
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
        invocation(Path::new(exe), None)
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
        let invocation = invocation_for(&gui);
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
        let invocation = invocation_for(&gui);
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
        assert!(invocation_for(&gui).contains(SUBCOMMAND));
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
        let doc = document(&aliased("/usr/bin/rcm"));
        assert!(doc.contains("KEEP IT"), "{doc}");
        assert!(doc.contains("exactly one command"), "{doc}");
        assert!(doc.contains("Do not read stdin twice"), "{doc}");
    }

    #[test]
    fn the_document_embeds_the_command_verbatim() {
        let invocation = aliased("/usr/bin/rcm");
        assert!(document(&invocation).contains(&command_for(&invocation)));
    }

    /// The merge example must carry the real quoted path too, or an agent
    /// splicing it into an existing status line writes a placeholder.
    #[test]
    fn the_merge_example_carries_the_real_path_not_a_placeholder() {
        let doc = document(&aliased(
            "/Applications/Rusted Claude Meter.app/Contents/MacOS/rcm",
        ));
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
            document(&invocation_for(&gui)),
            document(&aliased("/x/rcm")),
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
        assert!(document(&aliased("/usr/bin/rcm")).contains("2.1.216"));
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
}
