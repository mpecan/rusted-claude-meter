//! `rusted-claude-meter statusline` — the Claude Code side of the bridge.
//!
//! Claude Code spawns the command named by `statusLine.command` and writes a
//! JSON blob to its **stdin** on every status-line render. Since Claude Code
//! 2.1.216 that blob carries a `rate_limits` object holding the same two
//! headline windows this app otherwise polls claude.ai for:
//!
//! ```json
//! { "rate_limits": {
//!     "five_hour": { "used_percentage": 37.4, "resets_at": 1785682800 },
//!     "seven_day": { "used_percentage": 61.2, "resets_at": 1785920400 } } }
//! ```
//!
//! `resets_at` is epoch **seconds**. `used_percentage` is already on the
//! 0–100 scale [`meter_core::UsageWindow::utilization`] uses, and arrives as
//! a JSON **integer** whenever a window sits on a round number — see the
//! captured fixture in the tests.
//!
//! **What this source cannot report**, all three fixed upstream rather than
//! here:
//! - Headline windows only — no per-model breakdown, so `shown_scoped_models`
//!   has no source. (The Agent SDK's experimental `get_usage` control request
//!   does carry `model_scoped`; a different, heavier integration.)
//! - Subscription auth only. Claude Code hard-gates its rate-limit store, so
//!   API-key, Bedrock and Vertex sessions omit `rate_limits` entirely.
//! - Only while Claude Code runs, and only from its first API response onward.
//!
//! **Composing with an existing status line.** The `statusLine.command` slot
//! holds exactly one command, so this is designed to be added to whatever the
//! user already has rather than to replace it. It prints its segment on
//! stdout, so command substitution drops it into an existing line:
//!
//! ```sh
//! input=$(cat)
//! meter=$(printf '%s' "$input" | rusted-claude-meter statusline)
//! printf '%s' "…the user's own line… $meter"
//! ```
//!
//! `--quiet` records without printing, for a line that wants the recording
//! but not the text. `--pace` appends the off-pace signal — the thing Claude
//! Code cannot tell you itself, since it reports a percentage but has no
//! notion of whether you are spending it faster than the window replenishes.

use std::io::{self, Read as _};

use jiff::Timestamp;
use meter_core::{HEAVY_OVERUSE_THRESHOLD, LimitWindow, PaceKind, UsageSnapshot, UsageWindow};
use serde::Deserialize;

use super::config::{self, StatuslineConfig};
use super::{STATUSLINE_FILE, headline_snapshot, home_path, record};

/// `argv[1]` that selects this mode instead of launching the GUI.
pub const SUBCOMMAND: &str = "statusline";
/// Record the reading but print nothing, so the user's own status-line
/// command keeps the slot. See the module docs.
pub const QUIET_FLAG: &str = "--quiet";
/// Append the off-pace signal to the segment. Opt-in, and a flag rather than
/// a setting because the status line is its own surface: the tray already
/// sets the precedent that one surface's display preference must not silently
/// gate another's. The pace *maths* still comes from the app — see
/// [`super::config`].
pub const PACE_FLAG: &str = "--pace";

/// Burning quota faster than the window replenishes it.
const HOT_GLYPH: &str = "🔥";
/// Underusing — on track to leave quota unspent before the window resets.
const COLD_GLYPH: &str = "❄️";

/// The status-line separator, matching the tray menu's detail lines.
const SEPARATOR: &str = " · ";

/// One window as Claude Code writes it. Field names are Claude Code's, not
/// ours — a wire type, mapped to the domain by [`RawWindow::into_domain`].
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
    pub pace: bool,
}

/// Classify `args` (argv **without** the program name).
///
/// `None` means "not a status-line invocation" — the caller falls through to
/// launching the GUI, so an ordinary launch is unaffected.
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
        pace: rest.iter().any(|arg| arg == PACE_FLAG),
    })
}

/// Map one raw status-line blob to a snapshot taken at `now`.
///
/// `None` when the blob is unparseable **or** carries no usable window — see
/// [`headline_snapshot`] for the second rule. Both are ordinary, expected
/// states (a cold session has no `rate_limits` key at all), and both must
/// leave any previously recorded reading untouched.
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
    headline_snapshot(five_hour, seven_day, now)
}

/// The one-line status-line segment, e.g. `5h 37% · 7d 61%`, with the
/// off-pace signal appended when `pace` is supplied: `5h 37% · 7d 61% · 🔥1.4×`.
///
/// Only windows Claude Code actually reported appear, so a payload carrying
/// just the weekly window renders `7d 61%` rather than a gap.
#[must_use]
pub fn render(snapshot: &UsageSnapshot, now: Timestamp, pace: Option<StatuslineConfig>) -> String {
    let mut segments = Vec::new();
    if let Some(window) = snapshot.five_hour.as_ref() {
        segments.push(format!("5h {:.0}%", window.utilization));
    }
    if let Some(window) = snapshot.seven_day.as_ref() {
        segments.push(format!("7d {:.0}%", window.utilization));
    }
    if let Some(signal) = pace.and_then(|config| pace_segment(snapshot, now, config)) {
        segments.push(signal);
    }
    segments.join(SEPARATOR)
}

/// The off-pace signal, or `None` when there is nothing worth saying.
///
/// Built on the app's hybrid headline-only rule (`pace_signal`: overuse on
/// either window, underuse only on the weekly one), with **one deliberate
/// difference** — an overuse signal must reach
/// [`HEAVY_OVERUSE_THRESHOLD`], not merely [`meter_core::RISK_THRESHOLD`].
///
/// The tray can afford the lower bar: its detail line prints the expected
/// percentage beside the ratio, so `1.0× pace · 40% expected` is
/// interpretable, and a badge is something you look at deliberately. A status
/// line is neither. It sits in the prompt permanently and carries no context,
/// so a ratio hovering either side of 1.0 would flicker a flame in and out
/// all afternoon and render as `🔥1.0×` — a glyph saying "trouble" next to a
/// number saying "exactly on pace". Raising the bar makes the signal both
/// stable and unambiguous: at 1.2 and above the rounded ratio can never read
/// as 1.0. Underuse already has its own margin ([`meter_core::UNDERUSE_THRESHOLD`],
/// 0.8) and needs no adjustment.
fn pace_segment(
    snapshot: &UsageSnapshot,
    now: Timestamp,
    config: StatuslineConfig,
) -> Option<String> {
    if !config.pace_tracking_enabled {
        return None;
    }
    let signal = snapshot
        .pace_signal(now, config.weekly_pace_days)
        .filter(|signal| signal.kind != PaceKind::Hot || signal.ratio >= HEAVY_OVERUSE_THRESHOLD)?;
    let glyph = match signal.kind {
        PaceKind::Hot => HOT_GLYPH,
        PaceKind::Cold => COLD_GLYPH,
    };
    Some(format!("{glyph}{:.1}×", signal.ratio))
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
    let now = Timestamp::now();
    let Some(snapshot) = snapshot(&raw, now) else {
        return;
    };
    if let Some(path) = home_path(STATUSLINE_FILE)
        && let Err(error) = record(&path, &snapshot)
    {
        eprintln!("rusted-claude-meter: could not record the reading: {error}");
    }
    if !invocation.quiet {
        // The pace mirror is only read when it is going to be used, so a
        // status line that never asked for pace pays nothing for it.
        let pace = invocation.pace.then(|| {
            home_path(config::CONFIG_FILE)
                .map(|path| config::read(&path))
                .unwrap_or_default()
        });
        println!("{}", render(&snapshot, now, pace));
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

    /// Captured from Claude Code 2.1.220 (paths and ids genericised, the
    /// `rate_limits` block verbatim) — the fixture to check shape questions
    /// against, mirroring `meter-api`'s `usage_response_live.json`.
    const LIVE: &str = include_str!("../../tests/fixtures/statusline_payload_live.json");

    fn now() -> Timestamp {
        "2026-08-02T12:00:00Z".parse().unwrap()
    }

    fn args(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|arg| (*arg).to_owned()).collect()
    }

    /// Hand-written, covering every branch; see [`LIVE`] for shape questions.
    fn payload() -> String {
        serde_json::json!({
            "cwd": "/home/example/project",
            "model": { "id": "claude-opus-5", "display_name": "Opus 5" },
            "cost": { "total_cost_usd": 1.25 },
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
            Some(Invocation {
                quiet: false,
                pace: false
            })
        );
        assert_eq!(
            parse_args(&args(&[SUBCOMMAND, QUIET_FLAG])),
            Some(Invocation {
                quiet: true,
                pace: false
            })
        );
    }

    #[test]
    fn unknown_arguments_are_tolerated_rather_than_breaking_the_prompt() {
        assert_eq!(
            parse_args(&args(&[SUBCOMMAND, "--typo"])),
            Some(Invocation {
                quiet: false,
                pace: false
            })
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

    /// Pins two things the hand-written fixture cannot: that the unrelated
    /// two thirds of the blob really are ignorable, and that
    /// `used_percentage` arrives as a JSON **integer** for a window on a
    /// round number (`seven_day` was `3`, not `3.0`) while its sibling
    /// carries float noise from Claude Code's `utilization * 100`.
    #[test]
    fn the_live_captured_payload_maps_cleanly() {
        let snapshot = snapshot(LIVE, now()).unwrap();
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
        assert_eq!(
            render(&snapshot(LIVE, now()).unwrap(), now(), None),
            "5h 14% · 7d 3%"
        );
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
        assert_eq!(
            render(&snapshot(&payload(), now()).unwrap(), now(), None),
            "5h 37% · 7d 61%"
        );
    }

    #[test]
    fn the_pace_flag_is_recognised_and_independent_of_quiet() {
        assert_eq!(
            parse_args(&args(&[SUBCOMMAND, PACE_FLAG])),
            Some(Invocation {
                quiet: false,
                pace: true
            })
        );
        assert_eq!(
            parse_args(&args(&[SUBCOMMAND, QUIET_FLAG, PACE_FLAG])),
            Some(Invocation {
                quiet: true,
                pace: true
            })
        );
    }

    /// 95% spent with two hours left of a five-hour window: 60% of the window
    /// has elapsed, so this is ~1.6× sustainable pace. The case the flame is
    /// for, and the one Claude Code's own percentage cannot express.
    fn burning_hot() -> UsageSnapshot {
        let raw = serde_json::json!({
            "rate_limits": {
                "five_hour": { "used_percentage": 95.0, "resets_at": 1_785_690_000_i64 },
            },
        })
        .to_string();
        snapshot(&raw, "2026-08-02T15:00:00Z".parse().unwrap()).unwrap()
    }

    #[test]
    fn the_pace_flag_appends_a_banded_ratio() {
        let hot = burning_hot();
        let rendered = render(&hot, hot.fetched_at, Some(StatuslineConfig::default()));
        assert!(rendered.starts_with("5h 95%"), "{rendered}");
        assert!(rendered.contains(HOT_GLYPH), "{rendered}");
        assert!(rendered.contains('×'), "{rendered}");
    }

    /// Without the flag the segment is byte-for-byte what it was before pace
    /// existed — an existing status line must not change under its owner.
    #[test]
    fn without_the_flag_the_segment_is_unchanged() {
        let hot = burning_hot();
        assert_eq!(render(&hot, hot.fetched_at, None), "5h 95%");
    }

    /// The feature's master switch wins over the flag: "off" means pacing
    /// does not exist anywhere, which is what it means in the tray too.
    #[test]
    fn the_master_switch_overrides_the_flag() {
        let hot = burning_hot();
        let off = StatuslineConfig::new(7, false);
        assert_eq!(render(&hot, hot.fetched_at, Some(off)), "5h 95%");
    }

    /// The flicker guard, and the reason the status line raises the overuse
    /// bar above the tray's. This payload is genuinely over pace (~1.04×), so
    /// the app's own signal fires — but rendering it would put a flame beside
    /// a rounded `1.0×` and take it away again minutes later.
    #[test]
    fn a_barely_over_pace_burn_stays_silent_rather_than_flickering() {
        let steady = snapshot(&payload(), now()).unwrap();
        assert!(
            steady.pace_signal(now(), 7).is_some(),
            "fixture must actually trip the app's own lower threshold"
        );
        assert_eq!(
            render(&steady, now(), Some(StatuslineConfig::default())),
            "5h 37% · 7d 61%"
        );
    }

    /// Quiet by design: an unremarkable burn rate says nothing rather than
    /// parking a `1.0×` in the user's prompt all day.
    #[test]
    fn an_unremarkable_pace_adds_nothing() {
        let steady = snapshot(&payload(), now()).unwrap();
        assert_eq!(
            render(&steady, now(), Some(StatuslineConfig::default())),
            render(&steady, now(), None)
        );
    }

    /// The whole reason the pace basis is mirrored from the app rather than
    /// defaulted: a five-day week expects the weekly quota spent faster, so
    /// the same usage is a different ratio — and on a 7-day basis this one is
    /// not a signal at all.
    #[test]
    fn the_weekly_pace_basis_changes_what_the_signal_says() {
        let raw = serde_json::json!({
            "rate_limits": {
                "seven_day": { "used_percentage": 40.0, "resets_at": 1_786_017_600_i64 },
            },
        })
        .to_string();
        let now: Timestamp = "2026-08-05T12:00:00Z".parse().unwrap();
        let snap = snapshot(&raw, now).unwrap();
        let over_seven = render(&snap, now, Some(StatuslineConfig::new(7, true)));
        let over_five = render(&snap, now, Some(StatuslineConfig::new(5, true)));
        assert_ne!(
            over_seven, over_five,
            "a 5-day basis must not render the same as a 7-day one"
        );
    }

    #[test]
    fn render_omits_a_window_the_payload_did_not_report() {
        let raw = serde_json::json!({
            "rate_limits": {
                "seven_day": { "used_percentage": 61.2, "resets_at": 1_785_920_400_i64 },
            },
        })
        .to_string();
        assert_eq!(
            render(&snapshot(&raw, now()).unwrap(), now(), None),
            "7d 61%"
        );
    }
}
