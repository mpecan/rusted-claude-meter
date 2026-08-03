//! `rusted-claude-meter-statusline` — the Claude Code status-line bridge, on
//! its own.
//!
//! Reads one JSON blob from stdin, records the plan usage it carries to
//! `~/.claudemeter/statusline.json`, and prints a short segment. Exactly what
//! `rusted-claude-meter statusline` does, and that subcommand stays a working
//! alias — but this binary depends on `meter-files` and nothing else, so it
//! links none of `AppKit`, `WebKit`, Carbon, `CloudKit` or `QuartzCore`.
//!
//! That is the entire reason it exists. Claude Code spawns this on every
//! status-line redraw, several times a second, and mapping the GUI's
//! frameworks cost more than twice the bridge's own work (issue #72).
//!
//! Flags are the subcommand's, minus the subcommand: `--quiet` records
//! without printing, `--pace` appends the off-pace signal.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    meter_files::statusline::execute(meter_files::statusline::parse_flags(&args));
}
