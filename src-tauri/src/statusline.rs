//! `rusted-claude-meter statusline` — the Claude Code status-line bridge.
//!
//! Claude Code spawns the command configured under `statusLine.command` and
//! writes a JSON blob to its **stdin** on every status-line render. Since
//! Claude Code 2.1.216 that blob carries a `rate_limits` object holding the
//! very same 5-hour and 7-day windows this app otherwise polls claude.ai for:
//!
//! ```json
//! { "rate_limits": {
//!     "five_hour": { "used_percentage": 37.4, "resets_at": 1785682800 },
//!     "seven_day": { "used_percentage": 61.2, "resets_at": 1785920400 } } }
//! ```
//!
//! `resets_at` is epoch **seconds**; `used_percentage` is already on the
//! 0–100 scale [`meter_core::UsageWindow::utilization`] uses.
//!
//! **Why this exists.** Claude Code derives those numbers from the
//! `anthropic-ratelimit-unified-{5h,7d}-{utilization,reset}` response headers
//! on the user's *own* API traffic. Nothing in this module makes a claude.ai
//! request — Claude Code already made it, as itself — so this is the one data
//! path that sidesteps the Terms-of-Service problem written up in
//! `docs/terms-of-service.md` and guarded at runtime by [`crate::consent`].
//! It is deliberately **not** wired to that gate: there is nothing to consent
//! to when the app originates no traffic.
//!
//! **Why it cannot replace the poller.** Three hard limits, none of them
//! fixable here:
//! - Headline windows only. The payload carries no per-model breakdown, so
//!   `shown_scoped_models` has no source. (The Agent SDK's experimental
//!   `get_usage` control request does carry `model_scoped`; that is a
//!   different, heavier integration.)
//! - Subscription auth only. Claude Code hard-gates the store, so API-key,
//!   Bedrock and Vertex sessions omit `rate_limits` entirely.
//! - Only while Claude Code runs, and only from its first API response
//!   onward — the key is absent until then.
//!
//! So this records an *additional*, ToS-clean source alongside the gated
//! poller rather than replacing it.
//!
//! **Composing with an existing status line.** The `statusLine.command` slot
//! holds exactly one command, and most users already have one. `--quiet`
//! records without printing, so an existing line keeps its own rendering:
//!
//! ```sh
//! input=$(cat)
//! printf '%s' "$input" | rusted-claude-meter statusline --quiet
//! # ...the user's own rendering of "$input" follows
//! ```

use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use meter_core::{LimitWindow, UsageSnapshot, UsageWindow};
use serde::{Deserialize, Serialize};

use crate::export::{EXPORT_DIR, ExportLimit};
use crate::io_util::atomic_write;

/// `argv[1]` that selects this mode instead of launching the GUI.
pub const SUBCOMMAND: &str = "statusline";
/// Record the reading but print nothing, so the user's own status-line
/// command keeps the slot. See the module docs.
pub const QUIET_FLAG: &str = "--quiet";
/// File name inside [`EXPORT_DIR`], alongside `export.rs`'s `usage.json`.
///
/// A separate file on purpose: `usage.json` is this app's (and the Swift
/// app's) *output*, last-writer-wins with no merging. This one flows the
/// other way — written by a Claude Code subprocess, read by the app — and
/// mixing the two directions in one path would make "who wrote this?"
/// unanswerable.
pub const STATUSLINE_FILE: &str = "statusline.json";
/// Bumped whenever [`StatusLinePayload`] changes shape incompatibly, so a
/// future reader can tell a stale file from one it understands.
pub const SCHEMA_VERSION: u32 = 1;

/// The status-line separator, matching the tray menu's detail lines.
const SEPARATOR: &str = " · ";

/// One window as Claude Code writes it. Field names are Claude Code's, not
/// ours — this is a wire type, mapped to the domain by [`RawWindow::into_domain`].
#[derive(Debug, Clone, Copy, Deserialize)]
struct RawWindow {
    used_percentage: f64,
    /// Epoch **seconds**. Claude Code rounds it before emitting.
    resets_at: i64,
}

impl RawWindow {
    /// `None` when `resets_at` is not a representable instant — a corrupt or
    /// future-changed payload should cost one window, never the whole read.
    fn into_domain(self, window: LimitWindow) -> Option<UsageWindow> {
        Some(UsageWindow {
            utilization: self.used_percentage,
            resets_at: Timestamp::from_second(self.resets_at).ok()?,
            window,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct RawRateLimits {
    five_hour: Option<RawWindow>,
    seven_day: Option<RawWindow>,
}

/// The slice of Claude Code's status-line payload we care about. Every other
/// field (`model`, `workspace`, `cost`, `context_window`, …) is ignored by
/// serde's default unknown-field handling, which is what keeps this tolerant
/// of Claude Code adding to the blob.
#[derive(Debug, Default, Deserialize)]
struct StatusLineInput {
    /// Absent entirely on a cold session, and on API-key/Bedrock/Vertex auth.
    #[serde(default)]
    rate_limits: RawRateLimits,
}

/// What one status-line render is asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Invocation {
    pub quiet: bool,
}

/// Classify `args` (argv **without** the program name).
///
/// `None` means "not a status-line invocation" — the caller falls through to
/// launching the GUI, so a normal double-click is unaffected.
///
/// Unrecognized extra arguments are ignored rather than rejected: this runs
/// on every status-line render, and failing loudly there would break the
/// user's prompt over a typo.
#[must_use]
pub fn parse_args(args: &[String]) -> Option<Invocation> {
    let (subcommand, rest) = args.split_first()?;
    if subcommand != SUBCOMMAND {
        return None;
    }
    Some(Invocation {
        quiet: rest.iter().any(|arg| arg == QUIET_FLAG),
    })
}

/// Map one raw status-line blob to a snapshot taken at `now`.
///
/// `None` when the blob is unparseable **or** carries no usable window. Both
/// are ordinary, expected states (a cold session has no `rate_limits` key at
/// all), and both must leave any previously recorded reading untouched — a
/// blank file would look to a reader like "0% used" rather than "nothing new
/// to say".
///
/// `scoped` is always empty and `spend` always `None`: the payload carries
/// neither, and inventing either would be worse than reporting less.
#[must_use]
pub fn snapshot(input: &str, now: Timestamp) -> Option<UsageSnapshot> {
    let parsed: StatusLineInput = serde_json::from_str(input).ok()?;
    let five_hour = parsed
        .rate_limits
        .five_hour
        .and_then(|window| window.into_domain(LimitWindow::FiveHour));
    let seven_day = parsed
        .rate_limits
        .seven_day
        .and_then(|window| window.into_domain(LimitWindow::SevenDay));
    if five_hour.is_none() && seven_day.is_none() {
        return None;
    }
    Some(UsageSnapshot {
        five_hour,
        seven_day,
        scoped: Vec::new(),
        spend: None,
        fetched_at: now,
    })
}

/// The recorded reading, as it lands on disk.
///
/// Reuses [`ExportLimit`] so the two files in `~/.claudemeter/` describe a
/// limit the same way — a consumer that already reads `usage.json` needs no
/// second parser for the `utilization`/`reset_at` pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusLinePayload {
    pub schema: u32,
    /// The 5-hour headline window; `null` when Claude Code reported only the
    /// weekly one.
    pub session_usage: Option<ExportLimit>,
    /// The 7-day headline window; see `session_usage`.
    pub weekly_usage: Option<ExportLimit>,
    /// When this bridge observed the reading — *not* when Claude Code fetched
    /// it. A reader should treat a stale `recorded_at` as "Claude Code has
    /// not run recently", which is the normal case.
    pub recorded_at: Timestamp,
}

impl From<&UsageSnapshot> for StatusLinePayload {
    fn from(snapshot: &UsageSnapshot) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            session_usage: snapshot.five_hour.as_ref().map(ExportLimit::from),
            weekly_usage: snapshot.seven_day.as_ref().map(ExportLimit::from),
            recorded_at: snapshot.fetched_at,
        }
    }
}

/// `~/.claudemeter/statusline.json` given the user's home directory.
#[must_use]
pub fn statusline_path(home: &Path) -> PathBuf {
    home.join(EXPORT_DIR).join(STATUSLINE_FILE)
}

/// Persist `snapshot`, replacing any previous reading. Atomic, so the app can
/// read the file at any moment without seeing a half-written document.
pub fn record(path: &Path, snapshot: &UsageSnapshot) -> io::Result<()> {
    let body = serde_json::to_string_pretty(&StatusLinePayload::from(snapshot))?;
    atomic_write(path, &body)
}

/// The one-line status-line segment, e.g. `5h 37% · 7d 61%`.
///
/// Only windows Claude Code actually reported appear, so a payload carrying
/// just the weekly window renders `7d 61%` rather than a gap.
#[must_use]
pub fn render(snapshot: &UsageSnapshot) -> String {
    let mut segments = Vec::new();
    if let Some(window) = snapshot.five_hour.as_ref() {
        segments.push(format!("5h {:.0}%", window.utilization));
    }
    if let Some(window) = snapshot.seven_day.as_ref() {
        segments.push(format!("7d {:.0}%", window.utilization));
    }
    segments.join(SEPARATOR)
}

/// `$HOME`, which is what both target platforms (macOS, Linux) define. The
/// GUI resolves this through Tauri's path API, but that needs a built `App`
/// and this runs long before — and instead of — one.
fn home_dir() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    (!home.as_os_str().is_empty()).then_some(home)
}

/// Read one blob from stdin, record it, and print the segment unless
/// `--quiet`.
///
/// Never returns an error and never panics: this process *is* part of the
/// user's prompt, so every failure degrades to "print nothing this render"
/// rather than surfacing as a broken status line. Write failures go to
/// stderr, which Claude Code does not render.
pub fn execute(invocation: Invocation) {
    let mut raw = String::new();
    if let Err(error) = io::stdin().read_to_string(&mut raw) {
        eprintln!("rusted-claude-meter: could not read the status-line payload: {error}");
        return;
    }
    let Some(snapshot) = snapshot(&raw, Timestamp::now()) else {
        return;
    };
    if let Some(path) = home_dir().map(|home| statusline_path(&home))
        && let Err(error) = record(&path, &snapshot)
    {
        eprintln!("rusted-claude-meter: could not record the reading: {error}");
    }
    if !invocation.quiet {
        println!("{}", render(&snapshot));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    // Utilization is carried from the payload to the domain byte-for-byte —
    // no arithmetic touches it — so exact equality is the assertion that
    // actually pins the mapping.
    #![allow(clippy::float_cmp)]

    use super::*;
    use pretty_assertions::assert_eq;
    use std::fs;

    fn now() -> Timestamp {
        "2026-08-02T12:00:00Z".parse().unwrap()
    }

    fn args(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|arg| (*arg).to_owned()).collect()
    }

    /// Shaped after the real payload Claude Code 2.1.220 emits, unrelated
    /// fields included: the point is that they are ignored, so the bridge
    /// survives Claude Code adding to the blob.
    fn payload() -> String {
        serde_json::json!({
            "cwd": "/home/example/project",
            "model": { "id": "claude-opus-5", "display_name": "Opus 5" },
            "workspace": { "current_dir": "/home/example/project" },
            "cost": { "total_cost_usd": 1.25 },
            "context_window": { "used_percentage": 12.0 },
            "exceeds_200k_tokens": false,
            "rate_limits": {
                "five_hour": { "used_percentage": 37.4, "resets_at": 1_785_682_800_i64 },
                "seven_day": { "used_percentage": 61.2, "resets_at": 1_785_920_400_i64 },
            },
        })
        .to_string()
    }

    #[test]
    fn a_gui_launch_is_not_a_statusline_invocation() {
        assert_eq!(parse_args(&args(&[])), None);
        assert_eq!(parse_args(&args(&["--some-tauri-flag"])), None);
    }

    #[test]
    fn the_subcommand_prints_by_default_and_stays_silent_with_quiet() {
        assert_eq!(
            parse_args(&args(&[SUBCOMMAND])),
            Some(Invocation { quiet: false })
        );
        assert_eq!(
            parse_args(&args(&[SUBCOMMAND, QUIET_FLAG])),
            Some(Invocation { quiet: true })
        );
    }

    #[test]
    fn unknown_arguments_are_tolerated_rather_than_breaking_the_prompt() {
        assert_eq!(
            parse_args(&args(&[SUBCOMMAND, "--typo"])),
            Some(Invocation { quiet: false })
        );
    }

    #[test]
    fn both_headline_windows_map_from_a_real_shaped_payload() {
        let snapshot = snapshot(&payload(), now()).unwrap();
        let five_hour = snapshot.five_hour.unwrap();
        assert_eq!(five_hour.utilization, 37.4);
        assert_eq!(five_hour.window, LimitWindow::FiveHour);
        // epoch seconds, not milliseconds
        assert_eq!(
            five_hour.resets_at,
            "2026-08-02T15:00:00Z".parse::<Timestamp>().unwrap()
        );
        assert_eq!(snapshot.seven_day.unwrap().window, LimitWindow::SevenDay);
        assert_eq!(snapshot.fetched_at, now());
    }

    #[test]
    fn the_payload_carries_no_scoped_models_or_spend() {
        let snapshot = snapshot(&payload(), now()).unwrap();
        assert!(snapshot.scoped.is_empty());
        assert_eq!(snapshot.spend, None);
    }

    /// The cold-session and API-key/Bedrock/Vertex case: Claude Code omits
    /// `rate_limits` entirely. Recording an empty reading would read as
    /// "0% used" to a consumer, so there must be nothing to record.
    #[test]
    fn a_payload_without_rate_limits_yields_nothing_to_record() {
        let raw = serde_json::json!({ "model": { "display_name": "Opus 5" } }).to_string();
        assert_eq!(snapshot(&raw, now()), None);
    }

    #[test]
    fn an_empty_rate_limits_object_also_yields_nothing_to_record() {
        let raw = serde_json::json!({ "rate_limits": {} }).to_string();
        assert_eq!(snapshot(&raw, now()), None);
    }

    #[test]
    fn a_single_reported_window_is_kept_rather_than_discarded() {
        let raw = serde_json::json!({
            "rate_limits": {
                "seven_day": { "used_percentage": 61.2, "resets_at": 1_785_920_400_i64 },
            },
        })
        .to_string();
        let snapshot = snapshot(&raw, now()).unwrap();
        assert_eq!(snapshot.five_hour, None);
        assert!(snapshot.seven_day.is_some());
    }

    #[test]
    fn an_unrepresentable_reset_costs_one_window_not_the_whole_read() {
        let raw = serde_json::json!({
            "rate_limits": {
                "five_hour": { "used_percentage": 37.4, "resets_at": i64::MAX },
                "seven_day": { "used_percentage": 61.2, "resets_at": 1_785_920_400_i64 },
            },
        })
        .to_string();
        let snapshot = snapshot(&raw, now()).unwrap();
        assert_eq!(snapshot.five_hour, None);
        assert!(snapshot.seven_day.is_some());
    }

    /// Captured from Claude Code 2.1.220 (paths and ids genericised, the
    /// `rate_limits` block verbatim) — the fixture to check shape questions
    /// against, mirroring `meter-api`'s `usage_response_live.json`.
    ///
    /// It pins two things the hand-written fixture above cannot: that the
    /// unrelated two thirds of the blob really are ignorable, and that
    /// `used_percentage` arrives as a JSON **integer** for a window sitting
    /// on a round number (`seven_day` was `3`, not `3.0`) while its sibling
    /// carries `float`-noise from Claude Code's `utilization * 100`.
    #[test]
    fn the_live_captured_payload_maps_cleanly() {
        let raw = include_str!("../tests/fixtures/statusline_payload_live.json");
        let snapshot = snapshot(raw, now()).unwrap();
        assert_eq!(
            snapshot.five_hour.unwrap().utilization,
            14.000_000_000_000_002
        );
        let seven_day = snapshot.seven_day.unwrap();
        assert_eq!(seven_day.utilization, 3.0);
        assert_eq!(
            seven_day.resets_at,
            "2026-08-09T11:00:00Z".parse::<Timestamp>().unwrap()
        );
    }

    #[test]
    fn the_live_payload_renders_without_float_noise() {
        let raw = include_str!("../tests/fixtures/statusline_payload_live.json");
        assert_eq!(render(&snapshot(raw, now()).unwrap()), "5h 14% · 7d 3%");
    }

    #[test]
    fn malformed_input_is_ignored_rather_than_fatal() {
        assert_eq!(snapshot("", now()), None);
        assert_eq!(snapshot("not json at all", now()), None);
        assert_eq!(snapshot("{\"rate_limits\": 7}", now()), None);
    }

    #[test]
    fn utilization_above_a_hundred_is_carried_through_unclamped() {
        let raw = serde_json::json!({
            "rate_limits": {
                "five_hour": { "used_percentage": 104.5, "resets_at": 1_785_682_800_i64 },
            },
        })
        .to_string();
        assert_eq!(
            snapshot(&raw, now())
                .unwrap()
                .five_hour
                .unwrap()
                .utilization,
            104.5
        );
    }

    #[test]
    fn render_shows_both_windows_rounded() {
        let snapshot = snapshot(&payload(), now()).unwrap();
        assert_eq!(render(&snapshot), "5h 37% · 7d 61%");
    }

    #[test]
    fn render_omits_a_window_the_payload_did_not_report() {
        let raw = serde_json::json!({
            "rate_limits": {
                "seven_day": { "used_percentage": 61.2, "resets_at": 1_785_920_400_i64 },
            },
        })
        .to_string();
        assert_eq!(render(&snapshot(&raw, now()).unwrap()), "7d 61%");
    }

    /// Golden-file test: pins the on-disk contract the app-side reader will
    /// be written against. Any change here is a breaking change.
    #[test]
    fn golden_recorded_schema() {
        let payload = StatusLinePayload::from(&snapshot(&payload(), now()).unwrap());
        assert_eq!(
            serde_json::to_value(&payload).unwrap(),
            serde_json::json!({
                "schema": 1,
                "session_usage": {
                    "utilization": 37.4,
                    "reset_at": "2026-08-02T15:00:00Z",
                },
                "weekly_usage": {
                    "utilization": 61.2,
                    "reset_at": "2026-08-05T09:00:00Z",
                },
                "recorded_at": "2026-08-02T12:00:00Z",
            })
        );
    }

    #[test]
    fn record_round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = statusline_path(dir.path());
        let snapshot = snapshot(&payload(), now()).unwrap();
        record(&path, &snapshot).unwrap();
        let decoded: StatusLinePayload =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(decoded, StatusLinePayload::from(&snapshot));
    }

    #[test]
    fn record_replaces_a_previous_reading_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = statusline_path(dir.path());
        let mut snapshot = snapshot(&payload(), now()).unwrap();
        record(&path, &snapshot).unwrap();
        snapshot.fetched_at = "2026-08-02T13:00:00Z".parse().unwrap();
        record(&path, &snapshot).unwrap();
        let decoded: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(decoded["recorded_at"], "2026-08-02T13:00:00Z");
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn the_recorded_file_sits_beside_the_usage_export() {
        assert_eq!(
            statusline_path(Path::new("/home/example")),
            PathBuf::from("/home/example/.claudemeter/statusline.json")
        );
    }
}
