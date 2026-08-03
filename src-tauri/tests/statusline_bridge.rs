//! The two things about `rusted-claude-meter-statusline` that only the built
//! binary can prove (issue #72).
//!
//! **It must stay cheap.** The whole reason it exists is that the GUI binary
//! made dyld map `AppKit`, `WebKit`, Carbon, `CloudKit` and `QuartzCore` on every
//! status-line redraw — ~3ms of a ~5ms invocation, several times a second,
//! for frameworks the bridge never touches. Nothing in the source says
//! "do not link Tauri"; a single `use rusted_claude_meter_lib::…` in
//! `src/bin/statusline.rs` would put all of it back, compile cleanly, pass
//! every unit test and cost exactly what this change was made to remove. Only
//! the linked binary can answer that, so this asks it.
//!
//! **The subcommand must keep working.** It lives in the user's
//! `~/.claude/settings.json`, which this app never edits, so it is kept as an
//! alias indefinitely — and an alias nobody exercises is an alias that rots.

#![allow(clippy::unwrap_used)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use meter_files::export::claudemeter_path;
use meter_files::statusline::{STATUSLINE_FILE, SUBCOMMAND, setup::BRIDGE_BIN};

/// A payload shaped like Claude Code's, carrying both headline windows.
const PAYLOAD: &str = r#"{
  "model": { "display_name": "Opus 5" },
  "rate_limits": {
    "five_hour": { "used_percentage": 37.4, "resets_at": 1785682800 },
    "seven_day": { "used_percentage": 61.2, "resets_at": 1785920400 }
  }
}"#;

/// Shared libraries that only the GUI half of the app has any use for, and
/// the tool that reports them. Named per platform because the linkage
/// differs, but the question does not: has the bridge acquired a dependency
/// on the windowing stack?
///
/// The library names are matched against that tool's stdout, so they are
/// spelled exactly as the linker prints them — plain text, **not** the
/// back-ticked prose the doc comments around them use.
#[cfg(target_os = "macos")]
const GUI_LIBRARIES: &[&str] = &["AppKit", "WebKit", "Carbon", "CloudKit", "QuartzCore"];
#[cfg(target_os = "macos")]
const LINKAGE_TOOL: (&str, &[&str]) = ("otool", &["-L"]);

#[cfg(all(unix, not(target_os = "macos")))]
const GUI_LIBRARIES: &[&str] = &["webkit2gtk", "libgtk-3", "libsoup"];
#[cfg(all(unix, not(target_os = "macos")))]
const LINKAGE_TOOL: (&str, &[&str]) = ("ldd", &[]);

/// What `binary` links, or `None` where [`LINKAGE_TOOL`] is not installed.
/// Skipping beats failing on a toolchain gap — the assertions below are about
/// our binary, not about the developer's Xcode install.
fn linkage(binary: &str) -> Option<String> {
    let (tool, flags) = LINKAGE_TOOL;
    let output = Command::new(tool).args(flags).arg(binary).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Run `binary` with `args`, feeding it [`PAYLOAD`] on stdin and a private
/// `HOME` so the recorded reading lands in `home` rather than the developer's
/// real `~/.claudemeter/`.
fn render(binary: &str, args: &[&str], home: &Path) -> String {
    let mut child = Command::new(binary)
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(PAYLOAD.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{binary} exited {:?}",
        output.status
    );
    String::from_utf8(output.stdout).unwrap()
}

/// The load-bearing one. A bridge that links the windowing stack is a bridge
/// that costs what it cost before the split.
#[test]
fn the_bridge_binary_links_nothing_from_the_gui() {
    let bridge = env!("CARGO_BIN_EXE_rusted-claude-meter-statusline");
    let Some(linked) = linkage(bridge) else {
        eprintln!("skipped: no linkage tool on this platform");
        return;
    };
    for library in GUI_LIBRARIES {
        assert!(
            !linked.contains(library),
            "the status-line bridge links {library}:\n{linked}"
        );
    }
}

/// …and the sibling that stops the one above passing vacuously. If the GUI
/// binary stopped linking these too, the assertion would be checking nothing.
#[test]
fn the_gui_binary_does_link_them() {
    let gui = env!("CARGO_BIN_EXE_rusted-claude-meter");
    let Some(linked) = linkage(gui) else {
        eprintln!("skipped: no linkage tool on this platform");
        return;
    };
    assert!(
        GUI_LIBRARIES.iter().any(|library| linked.contains(library)),
        "the GUI binary links none of {GUI_LIBRARIES:?} — is this test still \
         asking the right question?\n{linked}"
    );
}

/// Where the bridge records its reading under a test `HOME`.
fn recording(home: &Path) -> PathBuf {
    claudemeter_path(home, STATUSLINE_FILE)
}

/// The name in `src-tauri/Cargo.toml`'s second `[[bin]]` really is the one
/// `setup` writes into the user's status line. A rename on the Cargo side
/// already breaks the `CARGO_BIN_EXE_…` below at compile time; this ties the
/// constant to it, so the pair cannot drift silently.
#[test]
fn the_bridge_constant_names_the_binary_cargo_actually_builds() {
    let built = Path::new(env!("CARGO_BIN_EXE_rusted-claude-meter-statusline"));
    assert_eq!(built.file_stem().unwrap(), BRIDGE_BIN);
}

#[test]
fn the_bridge_binary_renders_a_segment_and_records_the_reading() {
    let home = tempfile::tempdir().unwrap();
    let bridge = env!("CARGO_BIN_EXE_rusted-claude-meter-statusline");
    assert_eq!(render(bridge, &[], home.path()).trim(), "5h 37% · 7d 61%");

    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(recording(home.path())).unwrap()).unwrap();
    assert_eq!(json["session_usage"]["utilization"], 37.4);
    assert_eq!(json["weekly_usage"]["utilization"], 61.2);
}

#[test]
fn the_bridge_binary_takes_the_flags_without_the_subcommand() {
    let home = tempfile::tempdir().unwrap();
    let bridge = env!("CARGO_BIN_EXE_rusted-claude-meter-statusline");
    assert_eq!(render(bridge, &["--quiet"], home.path()), "");
    assert!(recording(home.path()).is_file());
}

/// Every status line set up before the standalone binary existed names this
/// form, and this app cannot edit `~/.claude/settings.json` to migrate them.
#[test]
fn the_gui_binarys_subcommand_still_renders_the_very_same_segment() {
    let home = tempfile::tempdir().unwrap();
    let gui = env!("CARGO_BIN_EXE_rusted-claude-meter");
    let bridge = env!("CARGO_BIN_EXE_rusted-claude-meter-statusline");
    assert_eq!(
        render(gui, &[SUBCOMMAND], home.path()),
        render(bridge, &[], home.path())
    );
}
