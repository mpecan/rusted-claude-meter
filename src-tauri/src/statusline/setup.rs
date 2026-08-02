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

use std::io;
use std::path::{Path, PathBuf};

use crate::export::claudemeter_path;
use crate::io_util::atomic_write;
use crate::statusline::{STATUSLINE_FILE, SUBCOMMAND};

/// File name inside [`EXPORT_DIR`], beside `usage.json` and `statusline.json`.
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

/// Single-quote `value` for POSIX `sh`, so a path containing spaces — every
/// macOS install — survives being pasted into a shell command. An embedded
/// single quote is closed, escaped and reopened, the standard idiom.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// A complete `statusLine.command` driving the bridge at `exe`.
///
/// The shape is the *component* one — capture stdin once, pipe a copy through
/// the bridge, keep the result in `$meter` — so a user who already has a
/// status line replaces only the final `printf`. Claude Code hands its blob to
/// one command and one command only, so composing is the normal case.
#[must_use]
pub fn command_for(exe: &str) -> String {
    format!(
        "input=$(cat); meter=$(printf '%s' \"$input\" | {} {}); printf '%s' \"$meter\"",
        shell_quote(exe),
        SUBCOMMAND
    )
}

/// The running executable's path — the thing a user could not guess,
/// especially on macOS where it sits inside the app bundle.
fn current_exe() -> String {
    std::env::current_exe().map_or_else(
        |_| FALLBACK_EXE.to_owned(),
        |path| path.display().to_string(),
    )
}

/// [`command_for`] against the running executable.
#[must_use]
pub fn current_command() -> String {
    command_for(&current_exe())
}

/// The document written to [`SETUP_FILE`].
///
/// Written so `/statusline` cannot get the composition wrong: the two failure
/// modes are replacing an existing status line and reading stdin twice (which
/// hangs), so both are called out rather than left to be inferred.
#[must_use]
pub fn document(exe: &str) -> String {
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
        meter=$(printf '%s' \"$input\" | {exe} {subcommand})
        printf '%s %s' \"<whatever the existing line printed>\" \"$meter\"

    Do not read stdin twice — the second read has nothing to read and will
    hang.

Notes:

  * The meter prints nothing when Claude Code reports no usage: a cold
    session, or an API-key/Bedrock/Vertex login, which carry no plan limits.
    The rest of the status line is unaffected.
  * Add \" --quiet\" after \"{subcommand}\" to record the reading without
    printing a segment.
  * Requires Claude Code 2.1.216 or newer; older versions send no rate limits.

This file is rewritten by Rusted Claude Meter on every launch. Edit the
status line, not this file.
",
        command = command_for(exe),
        exe = shell_quote(exe),
        subcommand = SUBCOMMAND,
    )
}

/// Write the setup document for the running executable. Best-effort: a
/// failure here costs a convenience, never the app, so callers log and
/// continue — exactly how `export.rs`'s write is treated.
pub fn write(path: &Path) -> io::Result<()> {
    atomic_write(path, &document(&current_exe()))
}

/// Both files this module and its parent write, for one home directory.
/// Returned together so `lib.rs` resolves the home directory once.
#[must_use]
pub fn paths(home: &Path) -> (PathBuf, PathBuf) {
    (
        claudemeter_path(home, STATUSLINE_FILE),
        claudemeter_path(home, SETUP_FILE),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pretty_assertions::assert_eq;

    /// Pins the name `src/usage-source.ts` hardcodes into the `/statusline`
    /// invocation; renaming one without the other silently points the agent
    /// at a file that is not there.
    #[test]
    fn the_setup_file_name_matches_the_one_the_frontend_points_at() {
        assert_eq!(SETUP_FILE, "statusline-command.txt");
    }

    /// Every macOS install has spaces in its path ("Rusted Claude
    /// Meter.app"), so an unquoted path would be a broken paste for most
    /// users.
    #[test]
    fn a_path_with_spaces_is_quoted() {
        assert!(
            command_for("/Applications/Rusted Claude Meter.app/Contents/MacOS/rcm")
                .contains("'/Applications/Rusted Claude Meter.app/Contents/MacOS/rcm'")
        );
    }

    #[test]
    fn an_embedded_single_quote_is_escaped_rather_than_ending_the_string() {
        assert!(command_for("/home/o'brien/rcm").contains(r"'/home/o'\''brien/rcm'"));
    }

    #[test]
    fn the_command_reads_stdin_once_and_leaves_the_segment_in_meter() {
        let command = command_for("/usr/bin/rcm");
        assert!(command.starts_with("input=$(cat);"), "{command}");
        assert_eq!(command.matches("$(cat)").count(), 1, "{command}");
        assert!(command.contains("$meter"), "{command}");
        assert!(command.contains(SUBCOMMAND), "{command}");
    }

    #[test]
    fn the_current_command_names_a_real_executable_path() {
        // Under `cargo test` this is the test binary, which is the point:
        // whatever is running is what gets written.
        assert!(current_command().contains(SUBCOMMAND));
        assert!(current_command().contains(&shell_quote(&current_exe())));
    }

    /// The document exists to stop `/statusline` making the two mistakes that
    /// would break a working status line, so both must be stated outright.
    #[test]
    fn the_document_warns_against_replacing_and_against_double_reading_stdin() {
        let doc = document("/usr/bin/rcm");
        assert!(doc.contains("KEEP IT"), "{doc}");
        assert!(doc.contains("exactly one command"), "{doc}");
        assert!(doc.contains("Do not read stdin twice"), "{doc}");
    }

    #[test]
    fn the_document_embeds_the_command_verbatim() {
        let command = command_for("/usr/bin/rcm");
        assert!(document("/usr/bin/rcm").contains(&command));
    }

    /// The merge example must carry the real quoted path too, or an agent
    /// splicing it into an existing status line writes a placeholder.
    #[test]
    fn the_merge_example_carries_the_real_path_not_a_placeholder() {
        let doc = document("/Applications/Rusted Claude Meter.app/Contents/MacOS/rcm");
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

    #[test]
    fn the_document_pins_the_claude_code_version_floor() {
        // Below this the payload carries no `rate_limits` at all, so a user
        // would wire everything up correctly and see nothing.
        assert!(document("/usr/bin/rcm").contains("2.1.216"));
    }

    #[test]
    fn writing_creates_the_file_and_replaces_a_previous_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = claudemeter_path(dir.path(), SETUP_FILE);
        write(&path).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        assert!(first.contains(SUBCOMMAND));
        write(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn paths_resolves_both_files_from_one_home_directory() {
        let (recorded, setup) = paths(Path::new("/home/example"));
        assert_eq!(
            recorded,
            PathBuf::from("/home/example/.claudemeter/statusline.json")
        );
        assert_eq!(
            setup,
            PathBuf::from("/home/example/.claudemeter/statusline-command.txt")
        );
    }
}
