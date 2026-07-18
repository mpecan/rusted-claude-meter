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

use jiff::Timestamp;
use meter_core::{LimitWindow, PaceSignal, UsageWindow};
use meter_render::{IconState, IconStyle, Scale, round_percent};

use crate::scheduler::{MeterState, Phase, Staleness};

/// Everything the tray menu displays, as plain strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    /// One-line summary of the scheduler phase / data freshness.
    pub status_line: String,
    /// One line per reported window: headline first, then scoped, API order.
    pub usage_lines: Vec<String>,
    /// The off-pace tooltip text ("Used 72% vs 40% expected by now - …"),
    /// gated behind pace-first display (issue #16). `StatusNotifierItem`
    /// gives Linux trays no tooltip, so this is the only place that text
    /// surfaces there; it renders as an extra menu line on every platform
    /// rather than special-casing one.
    pub pace_line: Option<String>,
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

/// The weekly pace basis and pace-first display toggle (issue #16), bundled
/// together since they always come from the same settings snapshot and both
/// gate `PaceSignal` computation — off (`pace_first_display: false`) means
/// no signal is computed at all, matching upstream's quota-first mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaceOptions {
    pub weekly_pace_days: u8,
    pub pace_first_display: bool,
}

/// Off-pace signal for the icon badge and the menu's pace line, computed
/// only in pace-first display (issue #16) — matching upstream, quota-first
/// mode never shows the flame/snowflake or the pace tooltip text. Headline
/// windows only (`UsageSnapshot::pace_signal`'s own contract); scoped limits
/// don't participate.
fn pace_signal(state: &MeterState, now: Timestamp, pace: PaceOptions) -> Option<PaceSignal> {
    if !pace.pace_first_display {
        return None;
    }
    state
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.pace_signal(now, pace.weekly_pace_days))
}

/// The icon to render for a state: the live gauge when a snapshot exists,
/// an empty safe gauge otherwise. `icon` (style/mono/scale) is the user's
/// current choice from Settings — passed in rather than hardcoded so
/// switching styles takes effect on the very next state (no restart
/// needed). `pace` (issue #16) drives the flame/snowflake override: the
/// pace signal is only computed, and only overlaid onto the base icon, when
/// `pace.pace_first_display` is set.
pub fn icon_state(
    state: &MeterState,
    now: Timestamp,
    icon: IconOptions,
    pace: PaceOptions,
) -> IconState {
    let base = state.snapshot.as_ref().map_or(
        IconState {
            style: icon.style,
            percent: 0,
            secondary_percent: 0,
            status: meter_core::UsageStatus::Safe,
            at_risk: false,
            pace_kind: None,
            pace_band: None,
            pace_ratio: None,
            mono: icon.mono,
            scale: icon.scale,
        },
        |snapshot| IconState::from_snapshot(snapshot, now, icon.style, icon.mono, icon.scale),
    );
    match pace_signal(state, now, pace) {
        Some(signal) => base.with_pace(Some(signal.ratio), Some(signal.kind)),
        None => base,
    }
}

/// Build the menu view-model for a state at `now`.
///
/// `shown` is the user's opt-in set of scoped-model display names from
/// Settings (issue #6): a scoped limit only becomes a usage line once its
/// name is in this set, even when the API reports it as `is_active`. Empty
/// by default, so a freshly reported model stays out of the tray menu until
/// switched on. `pace` (issue #16) gates the pace line the same way it gates
/// the icon badge in [`icon_state`].
pub fn menu_model(
    state: &MeterState,
    now: Timestamp,
    shown: &HashSet<String>,
    pace: PaceOptions,
) -> MenuModel {
    let mut usage_lines = Vec::new();
    if let Some(snapshot) = &state.snapshot {
        if let Some(window) = &snapshot.five_hour {
            usage_lines.push(usage_line(window_label(window.window), window, now));
        }
        if let Some(window) = &snapshot.seven_day {
            usage_lines.push(usage_line(window_label(window.window), window, now));
        }
        for limit in &snapshot.scoped {
            if !limit.is_visible(shown) {
                continue;
            }
            let label = format!(
                "{} ({})",
                limit.display_name,
                window_label(limit.usage.window)
            );
            usage_lines.push(usage_line(&label, &limit.usage, now));
        }
    }
    MenuModel {
        status_line: status_line(state, now),
        usage_lines,
        pace_line: pace_signal(state, now, pace).map(|signal| signal.tooltip()),
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

/// The one-line phase/freshness summary. Whenever a cached snapshot is
/// still shown while polling is paused or failing, its age is surfaced
/// here so the usage lines are never presented as current data.
fn status_line(state: &MeterState, now: Timestamp) -> String {
    let age = state
        .snapshot
        .as_ref()
        .map(|snapshot| short_duration(now.duration_since(snapshot.fetched_at).as_secs()));
    match (state.phase, age) {
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
