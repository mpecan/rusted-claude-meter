//! The Claude Code status-line bridge, and the on-disk contracts it shares
//! with the app.
//!
//! **This crate is the `rusted-claude-meter-statusline` binary's entire
//! dependency set.** That, and not "is it a file?", is what decides whether
//! something belongs here — adding a module or a dependency to this crate
//! adds it to a binary Claude Code spawns several times a second, on every
//! status-line redraw. When that command was the GUI binary, every render
//! paid dyld the cost of mapping `AppKit`, `WebKit`, Carbon, `CloudKit` and
//! `QuartzCore`, none of which the bridge touches: roughly half of the ~5.3ms
//! it took, before a single byte of JSON was read (issue #72).
//!
//! So the three things here are the three the bridge needs. [`statusline`] is
//! the bridge itself and the reading it records; [`export`] is
//! `~/.claudemeter/` — the directory, the paths, and the `usage.json` the app
//! publishes into it, whose `ExportLimit` the recorded reading reuses;
//! [`io`] is the atomic-write idiom underneath both.
//!
//! `cache.rs` and `settings.rs` stay in `src-tauri` despite also using
//! [`io`] — not because they could not move (neither touches Tauri; their
//! caller resolves the app data dir and hands them a `&Path`), but because
//! moving them would grow this crate for the app's benefit and the bridge's
//! cost. Weigh anything new the same way.

pub mod export;
pub mod io;
pub mod statusline;
