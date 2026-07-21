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
//! window's `resets_at` jumps by more than a jitter tolerance, the cycle has
//! rolled over: the window "reset" (optionally reported as its own event,
//! gated by the caller) and every level re-arms for the new cycle.
//!
//! Two kinds of `resets_at` movement are deliberately *not* rollovers. The
//! API stamps a real reset with sub-second jitter around a stable boundary,
//! so cycle detection tolerates changes under [`RESET_JITTER_TOLERANCE_SECS`]
//! rather than comparing for exact equality. And a *synthesized* reset — the
//! `fetched_at + window` fallback the mapping fills in when the API reports
//! `resets_at: null` for an idle window (see
//! [`UsageWindow::reset_is_estimated`]) — advances with every poll, so a
//! rollover is only ever recognized between two real, API-reported resets,
//! never when either side is estimated. Threshold crossings still fire for a
//! window with an estimated reset; only its (meaningless) reset movement is
//! ignored.
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

/// The API stamps a real `resets_at` with sub-second jitter that can even
/// straddle the whole second (observed live: the same 17:00 boundary arriving
/// as `16:59:59.601` on one poll and `17:00:00.306` on the next). Exact
/// equality would read every poll as a fresh cycle. A genuine reset moves the
/// boundary by at least a full window (≥5 h), so a minute of slack cleanly
/// separates a real rollover from that jitter.
const RESET_JITTER_TOLERANCE_SECS: f64 = 60.0;

/// Dedup bookkeeping for one window's current cycle.
#[derive(Debug, Clone)]
struct CycleState {
    resets_at: Timestamp,
    /// Whether `resets_at` was a synthesized fallback (`fetched_at + window`)
    /// the last time this window was observed — see
    /// [`UsageWindow::reset_is_estimated`]. A synthesized reset advances every
    /// poll, so a rollover is only ever recognized between two *real*
    /// (API-reported) resets, never when either side is estimated.
    estimated: bool,
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
        fetched_at: Timestamp,
    ) -> Vec<NotificationEvent> {
        let options = ObserveOptions {
            thresholds,
            notify_resets,
            fetched_at,
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
        let estimated = usage.reset_is_estimated(options.fetched_at);
        let previous = self.windows.get(id);
        let first_sighting = previous.is_none();
        // A real rollover: both the last-seen and current resets are real
        // (never a synthesized fallback, which drifts every poll), and the
        // reset instant jumped by more than the jitter tolerance — so the
        // API's sub-second wobble on a stable boundary is not mistaken for a
        // new cycle.
        let is_new_cycle = previous.is_some_and(|state| {
            !estimated
                && !state.estimated
                && state
                    .resets_at
                    .duration_since(usage.resets_at)
                    .as_secs_f64()
                    .abs()
                    > RESET_JITTER_TOLERANCE_SECS
        });

        if is_new_cycle && options.notify_resets {
            events.push(NotificationEvent::WindowReset { window: id.clone() });
        }

        let state = self
            .windows
            .entry(id.clone())
            .or_insert_with(|| CycleState {
                resets_at: usage.resets_at,
                estimated,
                notified: HashSet::new(),
            });
        // Always track the latest reset instant and its estimated-ness (so a
        // window that transitions from idle/fallback to a real reported reset
        // is compared correctly next tick); only a real rollover re-arms the
        // thresholds.
        state.resets_at = usage.resets_at;
        state.estimated = estimated;
        if is_new_cycle {
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
    /// The snapshot's fetch time, used to recognize a synthesized (fallback)
    /// `resets_at` via [`UsageWindow::reset_is_estimated`].
    fetched_at: Timestamp,
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

    /// A fetch time far from every test window's `resets_at`, so the helper
    /// windows below are never accidentally classified as having a
    /// synthesized (`fetched_at + window`) reset. Tests that exercise the
    /// synthesized path build their `resets_at` from `fallback_reset` and pass
    /// the matching fetch time explicitly.
    fn fetched_at() -> Timestamp {
        "2000-01-01T00:00:00Z".parse().unwrap()
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

    fn window_resetting_at(utilization: f64, resets_at: Timestamp) -> UsageWindow {
        UsageWindow {
            utilization,
            resets_at,
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
            tracker.observe([(FIVE_HOUR, &five_hour)], thresholds(), false, fetched_at()),
            vec![]
        );

        // Climbs to 80%: crosses Warning only.
        let at_warning = window(80.0, 5);
        assert_eq!(
            tracker.observe(
                [(FIVE_HOUR, &at_warning)],
                thresholds(),
                false,
                fetched_at()
            ),
            vec![crossed(FIVE_HOUR, ThresholdLevel::Warning, 80.0)]
        );

        // Climbs further to 95%: crosses Critical only (Warning already
        // notified this cycle).
        let at_critical = window(95.0, 5);
        assert_eq!(
            tracker.observe(
                [(FIVE_HOUR, &at_critical)],
                thresholds(),
                false,
                fetched_at()
            ),
            vec![crossed(FIVE_HOUR, ThresholdLevel::Critical, 95.0)]
        );
    }

    #[test]
    fn jumping_straight_past_both_thresholds_fires_both_in_severity_order() {
        let mut tracker = NotificationTracker::new();
        tracker.observe(
            [(FIVE_HOUR, &window(10.0, 5))],
            thresholds(),
            false,
            fetched_at(),
        );

        let spike = window(99.0, 5);
        assert_eq!(
            tracker.observe([(FIVE_HOUR, &spike)], thresholds(), false, fetched_at()),
            vec![
                crossed(FIVE_HOUR, ThresholdLevel::Warning, 99.0),
                crossed(FIVE_HOUR, ThresholdLevel::Critical, 99.0),
            ]
        );
    }

    #[test]
    fn hovering_around_a_threshold_does_not_spam() {
        let mut tracker = NotificationTracker::new();
        tracker.observe(
            [(FIVE_HOUR, &window(40.0, 5))],
            thresholds(),
            false,
            fetched_at(),
        );

        let above = window(76.0, 5);
        assert_eq!(
            tracker
                .observe([(FIVE_HOUR, &above)], thresholds(), false, fetched_at())
                .len(),
            1
        );

        // Dips back under, climbs back over: no second notification within
        // the same cycle.
        let below = window(74.0, 5);
        assert!(
            tracker
                .observe([(FIVE_HOUR, &below)], thresholds(), false, fetched_at())
                .is_empty()
        );
        let above_again = window(77.0, 5);
        assert!(
            tracker
                .observe(
                    [(FIVE_HOUR, &above_again)],
                    thresholds(),
                    false,
                    fetched_at()
                )
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
                .observe([(FIVE_HOUR, &hot)], thresholds(), false, fetched_at())
                .is_empty()
        );

        // But a later crossing after the baseline (there is none left to
        // cross here, so re-affirm the level stays armed only by dropping
        // and re-crossing after a reset, covered separately) — same value
        // again still produces nothing.
        assert!(
            tracker
                .observe([(FIVE_HOUR, &hot)], thresholds(), false, fetched_at())
                .is_empty()
        );
    }

    #[test]
    fn reset_re_arms_thresholds_for_the_new_cycle() {
        let mut tracker = NotificationTracker::new();
        tracker.observe(
            [(FIVE_HOUR, &window(80.0, 5))],
            thresholds(),
            true,
            fetched_at(),
        );

        // Same cycle, still above Warning: no repeat.
        assert!(
            tracker
                .observe(
                    [(FIVE_HOUR, &window(82.0, 5))],
                    thresholds(),
                    true,
                    fetched_at()
                )
                .is_empty()
        );

        // The window resets: resets_at moves to a new, later moment.
        let reset = window(0.0, 24);
        assert_eq!(
            tracker.observe([(FIVE_HOUR, &reset)], thresholds(), true, fetched_at()),
            vec![NotificationEvent::WindowReset { window: FIVE_HOUR }]
        );

        // Climbing back over Warning in the new cycle fires again — it was
        // re-armed by the reset.
        let above_again = window(80.0, 24);
        assert_eq!(
            tracker.observe(
                [(FIVE_HOUR, &above_again)],
                thresholds(),
                true,
                fetched_at()
            ),
            vec![crossed(FIVE_HOUR, ThresholdLevel::Warning, 80.0)]
        );
    }

    #[test]
    fn reset_event_is_suppressed_when_the_caller_opts_out() {
        let mut tracker = NotificationTracker::new();
        tracker.observe(
            [(FIVE_HOUR, &window(80.0, 5))],
            thresholds(),
            false,
            fetched_at(),
        );

        let reset = window(0.0, 24);
        // No WindowReset event, but the cycle still re-arms underneath.
        assert!(
            tracker
                .observe([(FIVE_HOUR, &reset)], thresholds(), false, fetched_at())
                .is_empty()
        );
        let above_again = window(80.0, 24);
        assert_eq!(
            tracker.observe(
                [(FIVE_HOUR, &above_again)],
                thresholds(),
                false,
                fetched_at()
            ),
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
            fetched_at(),
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
            fetched_at(),
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
            fetched_at(),
        );

        let events = tracker.observe(
            [
                (fable_id.clone(), &window(90.0, 5)),
                (sonnet_id, &window(10.0, 5)),
            ],
            thresholds(),
            false,
            fetched_at(),
        );
        assert_eq!(
            events,
            vec![
                crossed(fable_id.clone(), ThresholdLevel::Warning, 90.0),
                crossed(fable_id, ThresholdLevel::Critical, 90.0),
            ]
        );
    }

    #[test]
    fn sub_second_jitter_in_a_real_reset_never_fires_a_reset() {
        // The exact values from a live api-responses.log: the same 17:00
        // boundary wobbling ±~1s across polls, even straddling the whole
        // second. None of these are the `fetched_at + window` fallback, so
        // they are real resets — their jitter must not read as a rollover.
        let mut tracker = NotificationTracker::new();
        let polls = [
            "2026-07-20T17:00:00.306338Z",
            "2026-07-20T16:59:59.601000Z",
            "2026-07-20T16:59:59.357227Z",
            "2026-07-20T17:00:00.261356Z",
            "2026-07-20T17:00:00.881395Z",
        ];
        let mut events = Vec::new();
        for ts in polls {
            let window = window_resetting_at(10.0, ts.parse().unwrap());
            events.extend(tracker.observe(
                [(FIVE_HOUR, &window)],
                thresholds(),
                true,
                fetched_at(),
            ));
        }
        assert_eq!(events, vec![], "sub-second jitter must not fire a reset");
    }

    #[test]
    fn a_synthesized_fallback_reset_never_fires_a_reset_even_as_it_drifts() {
        // An idle window: the API sends `resets_at: null`, so the mapping fills
        // `fetched_at + window`, which advances with every poll. That drift
        // must never be mistaken for a reset, even with reset notices on.
        let mut tracker = NotificationTracker::new();
        let mut events = Vec::new();
        for minutes in [0_i64, 5, 10, 20, 45] {
            let fetched = now() + SignedDuration::from_secs(minutes * 60);
            let window = window_resetting_at(10.0, LimitWindow::FiveHour.fallback_reset(fetched));
            events.extend(tracker.observe([(FIVE_HOUR, &window)], thresholds(), true, fetched));
        }
        assert_eq!(
            events,
            vec![],
            "a drifting synthesized reset must not fire a reset"
        );
    }

    #[test]
    fn a_window_with_a_synthesized_reset_still_reports_threshold_crossings() {
        // Estimated-ness suppresses only the (meaningless) reset movement —
        // the utilization is real, so crossings must still fire.
        let mut tracker = NotificationTracker::new();
        let baseline_fetch = now();
        let baseline =
            window_resetting_at(10.0, LimitWindow::FiveHour.fallback_reset(baseline_fetch));
        tracker.observe([(FIVE_HOUR, &baseline)], thresholds(), true, baseline_fetch);

        let later_fetch = now() + SignedDuration::from_secs(5 * 60);
        let hot = window_resetting_at(95.0, LimitWindow::FiveHour.fallback_reset(later_fetch));
        assert_eq!(
            tracker.observe([(FIVE_HOUR, &hot)], thresholds(), true, later_fetch),
            vec![
                crossed(FIVE_HOUR, ThresholdLevel::Warning, 95.0),
                crossed(FIVE_HOUR, ThresholdLevel::Critical, 95.0),
            ]
        );
    }
}
