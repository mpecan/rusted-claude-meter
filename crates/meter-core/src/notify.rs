//! Notification dedup state machine (issue #7).
//!
//! Decides *whether* a threshold crossing or window reset is worth telling
//! the user about, given nothing but successive [`UsageWindow`] observations
//! and the caller's configured thresholds. No I/O and no
//! `tauri-plugin-notification` here — that lives in `src-tauri`; this module
//! only tracks per-window state and returns [`NotificationEvent`]s for the
//! caller to render however it likes (native notification, log line, test
//! assertion).
//!
//! # Model
//! Each tracked window (headline five-hour/seven-day, or a model-scoped
//! limit keyed by its display name — see [`WindowId`]) has a *cycle*,
//! identified by its `resets_at`. Within a cycle, each threshold level
//! ([`ThresholdLevel::Warning`], [`ThresholdLevel::Critical`]) is notified
//! **at most once** — [`NotificationTracker::observe`] remembers which
//! levels already fired for that window's current cycle. Utilization
//! dropping back under a threshold and climbing over it again within the
//! same cycle does not re-fire ("hovering" produces no spam). When a
//! window's `resets_at` moves to a later moment, the cycle has rolled over:
//! the window "reset" (optionally reported as its own event, gated by the
//! caller) and every level re-arms for the new cycle.
//!
//! A window's first-ever observation is a baseline, not a crossing: if a
//! window is already above a threshold the very first time the tracker
//! sees it (e.g. the app just started and usage was already high), that
//! threshold is marked notified without emitting an event, so app startup
//! never fires a burst of notifications for state that predates tracking.
//! Only a level newly reached *after* that baseline counts as "crossing
//! up".
//!
//! Levels are computed from caller-supplied [`NotificationThresholds`], not
//! [`crate::UsageStatus`]'s hardcoded 50/80 split — those drive the tray
//! icon colour, a separate concern from what a user wants paged about.
//! `NotificationThresholds` is meant to be sourced from persisted settings.

use std::collections::{HashMap, HashSet};

use jiff::Timestamp;

use crate::window::{LimitWindow, UsageWindow};

/// Identity of a tracked window: which headline window, or which
/// model-scoped limit (keyed on the API's display name, the same identity
/// [`crate::ScopedLimit`] uses).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WindowId {
    Headline(LimitWindow),
    Scoped(String),
}

/// A crossing severity. Ordered: [`Self::Critical`] is worse than
/// [`Self::Warning`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ThresholdLevel {
    Warning,
    Critical,
}

/// Warning/critical utilization percentages (0-100 scale).
///
/// Sourced from persisted settings — never a hardcoded constant here
/// (issue #7's acceptance criteria: thresholds come from settings, not
/// constants).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NotificationThresholds {
    pub warning: f64,
    pub critical: f64,
}

impl NotificationThresholds {
    /// Every level `utilization` currently satisfies, in ascending
    /// severity.
    fn levels_reached(self, utilization: f64) -> impl Iterator<Item = ThresholdLevel> {
        [
            (self.warning, ThresholdLevel::Warning),
            (self.critical, ThresholdLevel::Critical),
        ]
        .into_iter()
        .filter(move |&(threshold, _)| utilization >= threshold)
        .map(|(_, level)| level)
    }
}

/// One thing worth telling the user about.
#[derive(Debug, Clone, PartialEq)]
pub enum NotificationEvent {
    /// `window` climbed into `level` for the first time this cycle.
    ThresholdCrossed {
        window: WindowId,
        level: ThresholdLevel,
        utilization: f64,
    },
    /// `window`'s limit reset (its `resets_at` rolled over to a later
    /// moment since it was last observed).
    WindowReset { window: WindowId },
}

/// Dedup bookkeeping for one window's current cycle.
#[derive(Debug, Clone)]
struct CycleState {
    resets_at: Timestamp,
    notified: HashSet<ThresholdLevel>,
}

/// Per-window dedup state, held across ticks by the caller.
///
/// One instance stays alive for the app's process lifetime. Pure: advancing
/// it only ever needs the windows observed this tick and the current
/// thresholds, never a clock or any I/O — every scenario is reproducible in
/// a unit test.
#[derive(Debug, Default)]
pub struct NotificationTracker {
    windows: HashMap<WindowId, CycleState>,
}

impl NotificationTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one tick's observations into the tracker, returning every event
    /// worth notifying about, in the order observed. For a window whose
    /// cycle just rolled over, [`NotificationEvent::WindowReset`] (when
    /// `notify_resets` is set) precedes that window's
    /// [`NotificationEvent::ThresholdCrossed`] events; those are always
    /// yielded in ascending severity (`Warning` before `Critical`).
    pub fn observe<'a>(
        &mut self,
        observations: impl IntoIterator<Item = (WindowId, &'a UsageWindow)>,
        thresholds: NotificationThresholds,
        notify_resets: bool,
    ) -> Vec<NotificationEvent> {
        let options = ObserveOptions {
            thresholds,
            notify_resets,
        };
        let mut events = Vec::new();
        for (id, usage) in observations {
            self.observe_one(&id, usage, options, &mut events);
        }
        events
    }

    fn observe_one(
        &mut self,
        id: &WindowId,
        usage: &UsageWindow,
        options: ObserveOptions,
        events: &mut Vec<NotificationEvent>,
    ) {
        let first_sighting = !self.windows.contains_key(id);
        let is_new_cycle = self
            .windows
            .get(id)
            .is_some_and(|state| state.resets_at != usage.resets_at);

        if is_new_cycle && options.notify_resets {
            events.push(NotificationEvent::WindowReset { window: id.clone() });
        }

        let state = self
            .windows
            .entry(id.clone())
            .or_insert_with(|| CycleState {
                resets_at: usage.resets_at,
                notified: HashSet::new(),
            });
        if is_new_cycle {
            state.resets_at = usage.resets_at;
            state.notified.clear();
        }

        for level in options.thresholds.levels_reached(usage.utilization) {
            if first_sighting {
                // Baseline: mark already-satisfied levels as spent so a
                // window that starts life above a threshold does not spam
                // on the very first observation.
                state.notified.insert(level);
                continue;
            }
            if state.notified.insert(level) {
                events.push(NotificationEvent::ThresholdCrossed {
                    window: id.clone(),
                    level,
                    utilization: usage.utilization,
                });
            }
        }
    }
}

/// Bundles [`observe`](NotificationTracker::observe)'s per-tick knobs so
/// the per-window worker stays under the argument-count lint.
#[derive(Debug, Clone, Copy)]
struct ObserveOptions {
    thresholds: NotificationThresholds,
    notify_resets: bool,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use jiff::SignedDuration;
    use pretty_assertions::assert_eq;

    fn now() -> Timestamp {
        "2026-07-17T12:00:00Z".parse().unwrap()
    }

    fn thresholds() -> NotificationThresholds {
        NotificationThresholds {
            warning: 75.0,
            critical: 90.0,
        }
    }

    fn window(utilization: f64, resets_in_hours: i64) -> UsageWindow {
        UsageWindow {
            utilization,
            resets_at: now() + SignedDuration::from_hours(resets_in_hours),
            window: LimitWindow::FiveHour,
        }
    }

    const FIVE_HOUR: WindowId = WindowId::Headline(LimitWindow::FiveHour);

    fn crossed(window: WindowId, level: ThresholdLevel, utilization: f64) -> NotificationEvent {
        NotificationEvent::ThresholdCrossed {
            window,
            level,
            utilization,
        }
    }

    #[test]
    fn crossing_up_notifies_once_per_level_in_order() {
        let mut tracker = NotificationTracker::new();
        let five_hour = window(40.0, 5);

        // Baseline below both thresholds: no event.
        assert_eq!(
            tracker.observe([(FIVE_HOUR, &five_hour)], thresholds(), false),
            vec![]
        );

        // Climbs to 80%: crosses Warning only.
        let at_warning = window(80.0, 5);
        assert_eq!(
            tracker.observe([(FIVE_HOUR, &at_warning)], thresholds(), false),
            vec![crossed(FIVE_HOUR, ThresholdLevel::Warning, 80.0)]
        );

        // Climbs further to 95%: crosses Critical only (Warning already
        // notified this cycle).
        let at_critical = window(95.0, 5);
        assert_eq!(
            tracker.observe([(FIVE_HOUR, &at_critical)], thresholds(), false),
            vec![crossed(FIVE_HOUR, ThresholdLevel::Critical, 95.0)]
        );
    }

    #[test]
    fn jumping_straight_past_both_thresholds_fires_both_in_severity_order() {
        let mut tracker = NotificationTracker::new();
        tracker.observe([(FIVE_HOUR, &window(10.0, 5))], thresholds(), false);

        let spike = window(99.0, 5);
        assert_eq!(
            tracker.observe([(FIVE_HOUR, &spike)], thresholds(), false),
            vec![
                crossed(FIVE_HOUR, ThresholdLevel::Warning, 99.0),
                crossed(FIVE_HOUR, ThresholdLevel::Critical, 99.0),
            ]
        );
    }

    #[test]
    fn hovering_around_a_threshold_does_not_spam() {
        let mut tracker = NotificationTracker::new();
        tracker.observe([(FIVE_HOUR, &window(40.0, 5))], thresholds(), false);

        let above = window(76.0, 5);
        assert_eq!(
            tracker
                .observe([(FIVE_HOUR, &above)], thresholds(), false)
                .len(),
            1
        );

        // Dips back under, climbs back over: no second notification within
        // the same cycle.
        let below = window(74.0, 5);
        assert!(
            tracker
                .observe([(FIVE_HOUR, &below)], thresholds(), false)
                .is_empty()
        );
        let above_again = window(77.0, 5);
        assert!(
            tracker
                .observe([(FIVE_HOUR, &above_again)], thresholds(), false)
                .is_empty()
        );
    }

    #[test]
    fn first_sighting_already_above_threshold_does_not_notify() {
        // App startup with a window already hot: no burst of notifications
        // for state that predates tracking.
        let mut tracker = NotificationTracker::new();
        let hot = window(95.0, 5);
        assert!(
            tracker
                .observe([(FIVE_HOUR, &hot)], thresholds(), false)
                .is_empty()
        );

        // But a later crossing after the baseline (there is none left to
        // cross here, so re-affirm the level stays armed only by dropping
        // and re-crossing after a reset, covered separately) — same value
        // again still produces nothing.
        assert!(
            tracker
                .observe([(FIVE_HOUR, &hot)], thresholds(), false)
                .is_empty()
        );
    }

    #[test]
    fn reset_re_arms_thresholds_for_the_new_cycle() {
        let mut tracker = NotificationTracker::new();
        tracker.observe([(FIVE_HOUR, &window(80.0, 5))], thresholds(), true);

        // Same cycle, still above Warning: no repeat.
        assert!(
            tracker
                .observe([(FIVE_HOUR, &window(82.0, 5))], thresholds(), true)
                .is_empty()
        );

        // The window resets: resets_at moves to a new, later moment.
        let reset = window(0.0, 24);
        assert_eq!(
            tracker.observe([(FIVE_HOUR, &reset)], thresholds(), true),
            vec![NotificationEvent::WindowReset { window: FIVE_HOUR }]
        );

        // Climbing back over Warning in the new cycle fires again — it was
        // re-armed by the reset.
        let above_again = window(80.0, 24);
        assert_eq!(
            tracker.observe([(FIVE_HOUR, &above_again)], thresholds(), true),
            vec![crossed(FIVE_HOUR, ThresholdLevel::Warning, 80.0)]
        );
    }

    #[test]
    fn reset_event_is_suppressed_when_the_caller_opts_out() {
        let mut tracker = NotificationTracker::new();
        tracker.observe([(FIVE_HOUR, &window(80.0, 5))], thresholds(), false);

        let reset = window(0.0, 24);
        // No WindowReset event, but the cycle still re-arms underneath.
        assert!(
            tracker
                .observe([(FIVE_HOUR, &reset)], thresholds(), false)
                .is_empty()
        );
        let above_again = window(80.0, 24);
        assert_eq!(
            tracker.observe([(FIVE_HOUR, &above_again)], thresholds(), false),
            vec![crossed(FIVE_HOUR, ThresholdLevel::Warning, 80.0)]
        );
    }

    #[test]
    fn multiple_windows_crossing_in_one_tick_each_get_their_own_event() {
        let mut tracker = NotificationTracker::new();
        let seven_day_id = WindowId::Headline(LimitWindow::SevenDay);
        let fable_id = WindowId::Scoped("Fable".to_owned());

        tracker.observe(
            [
                (FIVE_HOUR, &window(10.0, 5)),
                (seven_day_id.clone(), &window(10.0, 100)),
                (fable_id.clone(), &window(10.0, 100)),
            ],
            thresholds(),
            false,
        );

        let five_hour_hot = window(95.0, 5);
        let seven_day_warm = window(80.0, 100);
        let fable_safe = window(20.0, 100);
        let events = tracker.observe(
            [
                (FIVE_HOUR, &five_hour_hot),
                (seven_day_id.clone(), &seven_day_warm),
                (fable_id.clone(), &fable_safe),
            ],
            thresholds(),
            false,
        );
        assert_eq!(
            events,
            vec![
                crossed(FIVE_HOUR, ThresholdLevel::Warning, 95.0),
                crossed(FIVE_HOUR, ThresholdLevel::Critical, 95.0),
                crossed(seven_day_id, ThresholdLevel::Warning, 80.0),
            ]
        );
        // The unaffected scoped window produced nothing.
        assert!(!events.iter().any(|event| matches!(
            event,
            NotificationEvent::ThresholdCrossed { window, .. } if *window == fable_id
        )));
    }

    #[test]
    fn distinct_windows_are_tracked_independently() {
        let mut tracker = NotificationTracker::new();
        let fable_id = WindowId::Scoped("Fable".to_owned());
        let sonnet_id = WindowId::Scoped("Sonnet".to_owned());

        tracker.observe(
            [
                (fable_id.clone(), &window(10.0, 5)),
                (sonnet_id.clone(), &window(10.0, 5)),
            ],
            thresholds(),
            false,
        );

        let events = tracker.observe(
            [
                (fable_id.clone(), &window(90.0, 5)),
                (sonnet_id, &window(10.0, 5)),
            ],
            thresholds(),
            false,
        );
        assert_eq!(
            events,
            vec![
                crossed(fable_id.clone(), ThresholdLevel::Warning, 90.0),
                crossed(fable_id, ThresholdLevel::Critical, 90.0),
            ]
        );
    }
}
