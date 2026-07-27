//! Pure tray view-model: no Tauri types, no I/O, fully unit-testable.
//!
//! Everything the tray shows is computed here from a [`MeterState`] plus a
//! `now` timestamp: the icon state to render, the menu's status line and the
//! live usage lines (one per window — 5-hour, 7-day, each scoped model).
//! [`TrayDiff`] is the debounce gate: it remembers what the tray last
//! successfully applied (the caller commits each part only after the tray
//! call succeeded) and turns a fresh view-model into the minimal
//! [`TrayPlan`], so identical consecutive states touch neither the icon nor
//! the menu (no flicker, no redundant `set_icon` calls).

use std::collections::HashSet;
use std::fmt::Write as _;

use jiff::{Timestamp, tz::TimeZone};
use meter_core::{LimitWindow, Money, Spend, UsageMode, UsageStatus, UsageWindow};
use meter_render::{IconState, IconStyle, Scale, round_percent};

use crate::scheduler::{MeterState, Phase, Staleness};

/// Everything the tray menu displays, as plain strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    /// One-line summary of the scheduler phase / data freshness.
    pub status_line: String,
    /// Two lines per reported window — the usage line and, when pace tracking
    /// is on and the window has a meaningful ratio, its indented pace/projection
    /// detail line. Headline windows first, then scoped, API order.
    ///
    /// There is deliberately no separate headline pace line. `PaceSignal`'s
    /// tooltip text said the same thing as the detail lines, in one much wider
    /// string, and a `StatusNotifierItem` menu is the whole Linux surface — the
    /// width cost was real and the content was redundant.
    pub usage_lines: Vec<String>,
}

/// The base gauge to render, independent of pace-first display: the user's
/// chosen glyph, whether it renders as monochrome/template artwork, and the
/// raster scale. Bundled into one value (mirrors `scheduler::PersistPaths`)
/// so [`icon_state`] stays within the workspace's `too_many_arguments` limit
/// once [`PaceOptions`] is threaded in alongside it.
#[derive(Debug, Clone, Copy)]
pub struct IconOptions {
    pub style: IconStyle,
    pub mono: bool,
    pub scale: Scale,
}

/// The pace settings the tray needs (issue #16), bundled together since they
/// always come from the same settings snapshot.
///
/// The two flags are kept apart on purpose, and the tray is the reason. The
/// master switch `pace_tracking_enabled` decides whether pace math means
/// anything at all; `pace_first_display` decides only what the *icon* shows.
/// The menu honours the master switch alone, so pace and projections are there
/// for every user who has not turned tracking off — on Linux the menu is the
/// whole surface, and gating it on an icon preference hid the feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaceOptions {
    pub weekly_pace_days: u8,
    pub pace_tracking_enabled: bool,
    pub pace_first_display: bool,
}

impl PaceOptions {
    /// Whether the *icon* switches to a pace ratio: both the master switch and
    /// the display preference. The menu deliberately asks only about
    /// `pace_tracking_enabled`.
    const fn icon_is_pace_first(self) -> bool {
        self.pace_tracking_enabled && self.pace_first_display
    }
}

/// The menu's render context: the four things [`menu_model`] needs beyond the
/// state and the clock, which always travel together because they all come
/// from the same settings snapshot at the one call site. Mirrors
/// [`IconOptions`] and `TraySeed`.
#[derive(Debug, Clone, Copy)]
pub struct MenuOptions<'a> {
    /// The user's opt-in set of scoped-model display names (issue #6).
    pub shown: &'a HashSet<String>,
    pub pace: PaceOptions,
    pub usage_mode: UsageMode,
    /// The zone projected limit-hit times are rendered in. Passed in rather
    /// than resolved here so this stays a pure function and the specs can pin a
    /// fixed zone; the caller resolves the system zone once and reuses it.
    pub tz: &'a TimeZone,
}

/// The icon to render for a state: the live gauge when a snapshot exists,
/// an empty safe gauge otherwise. `icon` (style/mono/scale) is the user's
/// current choice from Settings — passed in rather than hardcoded so
/// switching styles takes effect on the very next state (no restart
/// needed).
///
/// `pace` (issue #16) drives pace-first display. When `pace_first_display`
/// is set, the primary metric always becomes a pace *ratio* whenever pace
/// math is meaningful, mirroring upstream `MenuBarManager.updateIcon`'s
/// fallback chain `paceSignal?.ratio ?? session.paceRatio ?? weekly.paceRatio`:
/// the hybrid hot/cold [`PaceSignal`]'s ratio when it exists, else the plain
/// session ratio, else the weekly ratio (on the chosen 5/6/7-day basis). The
/// flame/snowflake `pace_kind` badge stays gated on the hybrid signal being
/// present (its ratio drives the band colour regardless), so a sustainable
/// window shows the ratio in its band colour but no badge — matching upstream,
/// where `button.image`'s `paceKind` comes only from `paceSignal?.kind`.
pub fn icon_state(
    state: &MeterState,
    now: Timestamp,
    icon: IconOptions,
    pace: PaceOptions,
    usage_mode: UsageMode,
) -> IconState {
    // The empty "safe" gauge: shown when there is no snapshot at all, and also
    // the Cost-mode fallback when there is no spend figure to gauge — never the
    // allowance percentage, even for a limits-bearing snapshot pinned to Cost.
    let empty = IconState {
        style: icon.style,
        percent: 0,
        secondary_percent: 0,
        status: UsageStatus::Safe,
        at_risk: false,
        pace_kind: None,
        pace_band: None,
        pace_ratio: None,
        mono: icon.mono,
        scale: icon.scale,
    };
    let Some(snapshot) = state.snapshot.as_ref() else {
        return empty;
    };
    let base = IconState::from_snapshot(snapshot, now, icon.style, icon.mono, icon.scale);
    // A cost/spend account drives the icon from spend, not the percentage
    // windows or pace: a spend cap becomes the gauge; without a usable spend
    // figure the icon stays the empty gauge (not the allowance percentage) and
    // the "$" figure, if any, surfaces in the menu.
    if usage_mode.effective(snapshot) == UsageMode::Cost {
        return snapshot
            .spend
            .as_deref()
            .map_or(empty, |spend| cost_icon(empty, spend));
    }
    if !pace.icon_is_pace_first() {
        return base;
    }
    let signal = snapshot.pace_signal(now, pace.weekly_pace_days);
    let ratio = signal.as_ref().map(|s| s.ratio).or_else(|| {
        // Upstream's fallback chain: the hybrid signal's ratio, else the
        // session window's, else the weekly one's — in that order, each on its
        // own pacing basis.
        [snapshot.five_hour.as_ref(), snapshot.seven_day.as_ref()]
            .into_iter()
            .flatten()
            .find_map(|w| w.pace_ratio(now, Some(w.window.pacing_duration(pace.weekly_pace_days))))
    });
    ratio.map_or(base, |ratio| {
        base.with_pace(Some(ratio), signal.map(|s| s.kind))
    })
}

/// The symbol for a currency code, when it's one we render with a glyph.
/// Anything else falls back to showing the ISO code after the amount.
const fn currency_symbol(code: &str) -> Option<&'static str> {
    match code.as_bytes() {
        b"USD" => Some("$"),
        b"EUR" => Some("€"),
        b"GBP" => Some("£"),
        b"JPY" => Some("¥"),
        _ => None,
    }
}

/// Format a [`Money`] in its own currency, e.g. `"€0.35"`, `"$125.00"`, or
/// `"1,000 SEK"` for a currency without a known glyph. The value comes from the
/// API in minor units with the currency's decimal-place count, so the exact
/// figure is preserved without floating-point rounding. A negative amount
/// (which should not normally occur) keeps a leading `-` so a bad figure is
/// visible rather than silently mangled. Mirrors the frontend `formatMoney`.
fn format_money(money: &Money) -> String {
    let exponent = u32::from(money.exponent);
    let divisor = 10_u64.pow(exponent);
    let sign = if money.minor < 0 { "-" } else { "" };
    let abs = money.minor.unsigned_abs();
    let amount = if exponent == 0 {
        (abs / divisor).to_string()
    } else {
        format!(
            "{}.{:0width$}",
            abs / divisor,
            abs % divisor,
            width = exponent as usize
        )
    };
    currency_symbol(&money.currency).map_or_else(
        || format!("{sign}{amount} {}", money.currency),
        |symbol| format!("{sign}{symbol}{amount}"),
    )
}

/// The icon gauge for a cost/spend account. A spend limit/cap turns
/// spend-to-date into a percentage gauge (coloured by that fraction, like the
/// allowance windows); without a denominator the `empty` gauge stands and the
/// compact spend figure surfaces in the menu instead.
fn cost_icon(empty: IconState, spend: &Spend) -> IconState {
    spend.fraction_used().map_or(empty, |fraction| {
        let percent_value = fraction * 100.0;
        IconState {
            percent: round_percent(percent_value),
            secondary_percent: 0,
            status: UsageStatus::from_utilization(percent_value),
            at_risk: false,
            pace_kind: None,
            pace_band: None,
            pace_ratio: None,
            ..empty
        }
    })
}

/// The tray usage line(s) for a cost/spend account: the spend to date, annotated
/// with the percentage of the spend limit (or hard cap) when one is set. Empty
/// when the spend object holds no usable figure (e.g. the `{"unsurfaced": true}`
/// stub decoded to `None`), so the menu shows no line rather than a bogus `$0`.
fn cost_usage_lines(spend: &Spend) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(used) = &spend.used {
        let mut line = format!("Spend {} this period", format_money(used));
        let budget = spend.limit.as_ref().or(spend.cap.as_ref());
        if let (Some(budget), Some(fraction)) = (budget, spend.fraction_used()) {
            let percent = round_percent(fraction * 100.0);
            let _ = write!(line, " · {percent}% of {}", format_money(budget));
        }
        lines.push(line);
    }
    lines
}

/// Build the menu view-model for a state at `now`.
///
/// `shown` is the user's opt-in set of scoped-model display names from
/// Settings (issue #6): a scoped limit only becomes a usage line once its
/// name is in this set, even when the API reports it as `is_active`. Empty
/// by default, so a freshly reported model stays out of the tray menu until
/// switched on. `pace` (issue #16) gates the pace line the same way it gates
/// the icon badge in [`icon_state`].
pub fn menu_model(state: &MeterState, now: Timestamp, opts: MenuOptions) -> MenuModel {
    let mut usage_lines = Vec::new();
    if let Some(snapshot) = &state.snapshot {
        if opts.usage_mode.effective(snapshot) == UsageMode::Cost {
            // Cost/spend account: spend lines replace the percentage windows,
            // and there is no pace line (pacing is an allowance concept).
            if let Some(spend) = snapshot.spend.as_deref() {
                usage_lines = cost_usage_lines(spend);
            }
        } else {
            // Two lines per window at most, so the whole menu is one allocation.
            usage_lines.reserve(2 * (2 + snapshot.scoped.len()));
            let mut push = |label: &str, window: &UsageWindow| {
                usage_lines.push(usage_line(label, window, now));
                usage_lines.extend(detail_line(window, now, opts.pace, opts.tz));
            };
            if let Some(window) = &snapshot.five_hour {
                push(window_label(window.window), window);
            }
            if let Some(window) = &snapshot.seven_day {
                push(window_label(window.window), window);
            }
            for limit in &snapshot.scoped {
                if !limit.is_visible(opts.shown) {
                    continue;
                }
                let label = format!(
                    "{} ({})",
                    limit.display_name,
                    window_label(limit.usage.window)
                );
                push(&label, &limit.usage);
            }
        }
    }
    MenuModel {
        status_line: status_line(state, now),
        usage_lines,
    }
}

const fn window_label(window: LimitWindow) -> &'static str {
    match window {
        LimitWindow::FiveHour => "5-hour",
        LimitWindow::SevenDay => "7-day",
    }
}

/// A reset moment this recently in the past still reads "resets soon";
/// beyond it the line says how long ago the window reset — the cue that the
/// numbers come from a stale snapshot, not live data.
const RESET_SOON_GRACE_SECS: i64 = 5 * 60;

/// "5-hour: 42% — resets in 2h 15m"
fn usage_line(label: &str, window: &UsageWindow, now: Timestamp) -> String {
    let percent = round_percent(window.utilization);
    let remaining = window.resets_at.duration_since(now).as_secs();
    if remaining > 0 {
        format!(
            "{label}: {percent}% — resets in {}",
            short_duration(remaining)
        )
    } else if remaining > -RESET_SOON_GRACE_SECS {
        format!("{label}: {percent}% — resets soon")
    } else {
        format!(
            "{label}: {percent}% — reset {} ago",
            short_duration(-remaining)
        )
    }
}

/// Indent for a window's detail line, so it reads as belonging to the line
/// above rather than as another window.
///
/// Four spaces rather than a glyph because `DBusMenu` passes the label through
/// verbatim on both GNOME and Plasma (verified in the container harness) and a
/// leading punctuation mark would be read aloud by screen readers.
const DETAIL_INDENT: &str = "    ";

/// The pace and projection line that sits under a window's usage line:
/// `"    2.1× pace · 40% expected · hits limit ~1:59 PM"`.
///
/// The first two parts are exactly the popover's own pace line
/// (`render.ts::paceLine`), so the two surfaces read the same; the projection
/// is the third.
///
/// `None` when the window has no meaningful pace ratio yet — too little
/// elapsed *and* too little used (a front-loaded burst past
/// `MIN_USAGE_FOR_PROJECTION` surfaces immediately, which is the point of the
/// early-overuse rule) — which is what keeps the menu short early in a window
/// instead of padding it with "0.0× pace". Also `None` when pace tracking is
/// switched off entirely.
///
/// The projection prefers the limit-hit date (the actionable one: you are going
/// to run out, and when) and falls back to the projected end percentage when
/// the window is not on course to hit its limit at all.
fn detail_line(
    window: &UsageWindow,
    now: Timestamp,
    pace: PaceOptions,
    tz: &TimeZone,
) -> Option<String> {
    if !pace.pace_tracking_enabled {
        return None;
    }
    let pacing = Some(window.window.pacing_duration(pace.weekly_pace_days));
    let ratio = window.pace_ratio(now, pacing)?;
    let mut line = format!("{DETAIL_INDENT}{ratio:.1}× pace");
    if let Some(expected) = window.expected_usage_percent(now, pacing) {
        let _ = write!(line, " · {}% expected", round_percent(expected));
    }
    // Same three-case cascade the popover renders (`view-model.ts`'s
    // `projectionFor`): a limit already reached, else a projected hit before
    // reset, else where the window is on course to end. `projected_limit_date`
    // deliberately returns `None` at 100%, so without the first arm a maxed
    // window reads "on pace to end at ~100%" here while the popover says
    // "Limit reached" — the two surfaces describing the same data differently.
    if window.utilization >= 100.0 {
        line.push_str(" · limit reached");
    } else if let Some(hit_at) = window.projected_limit_date(now, pacing) {
        let _ = write!(line, " · hits limit ~{}", hit_time(hit_at, now, tz));
    } else if let Some(end) = window.projected_end_percent(now, pacing) {
        let _ = write!(line, " · on pace to end at ~{}%", round_percent(end));
    }
    Some(line)
}

/// Wall-clock time for a projected limit-hit: time-only when it lands today
/// ("1:59 PM"), month/day plus time otherwise ("Jul 27, 5:37 AM") so a
/// multi-day weekly projection isn't shown as a bare clock time that reads like
/// today. Mirrors the frontend `formatHitTime` (`src/format.ts`), except that
/// the frontend follows the browser locale while this is fixed English — the
/// menu is built in Rust and has no locale to consult.
fn hit_time(hit_at: Timestamp, now: Timestamp, tz: &TimeZone) -> String {
    let hit = hit_at.to_zoned(tz.clone());
    let today = now.to_zoned(tz.clone());
    let same_day = hit.date() == today.date();
    let format = if same_day {
        "%-I:%M %p"
    } else {
        "%b %-d, %-I:%M %p"
    };
    hit.strftime(format).to_string()
}

/// The one-line phase/freshness summary. Whenever a cached snapshot is
/// still shown while polling is paused or failing, its age is surfaced
/// here so the usage lines are never presented as current data.
fn status_line(state: &MeterState, now: Timestamp) -> String {
    let age = state
        .snapshot
        .as_ref()
        .map(|snapshot| short_duration(now.duration_since(snapshot.fetched_at).as_secs()));
    match (state.phase, age) {
        // Named as a decision the user still owes, not a fault: polling is off
        // because the ToS risk has not been accepted (see `crate::consent`).
        (Phase::AwaitingConsent, None) => "Paused — review the risk in Settings".to_owned(),
        (Phase::AwaitingConsent, Some(age)) => {
            format!("Paused — showing data from {age} ago")
        }
        (Phase::AwaitingSession, None) => "No session key — choose Open to set one".to_owned(),
        (Phase::AwaitingSession, Some(age)) => {
            format!("No session key — showing data from {age} ago")
        }
        (Phase::SessionExpired, None) => "Session expired — choose Open to update it".to_owned(),
        (Phase::SessionExpired, Some(age)) => {
            format!("Session expired — showing data from {age} ago")
        }
        (Phase::Degraded, None) => "Connection trouble — retrying".to_owned(),
        (Phase::Degraded, Some(age)) => format!("Connection trouble — data from {age} ago"),
        (Phase::Polling, None) => "Waiting for first update…".to_owned(),
        (Phase::Polling, Some(age)) => {
            if state.staleness == Staleness::Stale {
                format!("Stale — updated {age} ago")
            } else {
                format!("Updated {age} ago")
            }
        }
    }
}

/// Coarse human duration: "3d 4h", "2h 15m", "12m", "under 1m".
fn short_duration(total_secs: i64) -> String {
    let secs = total_secs.max(0);
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let minutes = (secs % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        "under 1m".to_owned()
    }
}

/// What the tray must actually touch for one state change. `None` fields
/// mean "already showing this — do nothing".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayPlan {
    pub icon: Option<IconState>,
    pub menu: Option<MenuModel>,
}

/// Debounce gate: remembers the last applied icon and menu so repeated
/// identical states produce no tray calls at all.
#[derive(Debug, Default)]
pub struct TrayDiff {
    last_icon: Option<IconState>,
    last_menu: Option<MenuModel>,
}

impl TrayDiff {
    /// Diff a fresh view-model against what the tray last successfully
    /// applied. Nothing is recorded here: the caller confirms each part via
    /// [`Self::commit_icon`] / [`Self::commit_menu`] only after the tray
    /// call actually succeeded, so a failed render or menu rebuild is
    /// retried on the next state instead of silently desyncing the gate.
    pub fn plan(&self, icon: IconState, menu: &MenuModel) -> TrayPlan {
        TrayPlan {
            icon: (self.last_icon != Some(icon)).then_some(icon),
            menu: (self.last_menu.as_ref() != Some(menu)).then(|| menu.clone()),
        }
    }

    /// Record that `icon` is now what the tray shows.
    pub const fn commit_icon(&mut self, icon: IconState) {
        self.last_icon = Some(icon);
    }

    /// Record that `menu` is now what the tray shows.
    pub fn commit_menu(&mut self, menu: MenuModel) {
        self.last_menu = Some(menu);
    }
}

#[cfg(test)]
mod spec;
