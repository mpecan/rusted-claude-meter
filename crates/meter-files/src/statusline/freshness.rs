//! Which of two readings is newer, when several Claude Code sessions share
//! one recording.
//!
//! Every open Claude Code session runs the bridge on every status-line
//! redraw, and a session redraws whether or not it is doing anything — on a
//! `refreshInterval`, on a resize, on a mode change. But its `rate_limits`
//! only move when *that* session gets an API response. An idle session
//! therefore keeps re-reporting whatever the windows were when it last spoke
//! to the API, possibly hours ago, and before this module existed each of
//! those redraws overwrote the recording — stamped `now` — on top of the
//! numbers an active session had just written. The tray and every status line
//! then flickered between the true usage and a stale one, and reported the
//! stale one as fresh.
//!
//! Two independent questions, answered from two different sources:
//!
//! - **Which numbers are current** is decided from the numbers themselves,
//!   per window ([`merge`], via [`UsageWindow::progress_cmp`]): within one
//!   window utilization never falls, and a later window always resets later,
//!   so any two readings of a limit order without trusting any clock. This is
//!   what stops a stale session from winning even at the moment its own
//!   transcript looks freshest — a prompt just typed into a session idle for
//!   two hours redraws with the two-hour-old windows before the response that
//!   would update them arrives.
//! - **How old the reading is** (`fetched_at`, which the scheduler's staleness
//!   rule ages) is when Claude Code last heard from the API, approximated by
//!   the session transcript's mtime ([`observed_at`]): Claude Code appends to
//!   it as each response streams in, and does not touch it while idle. It
//!   only ever *advances* the recorded time, and only from a reading that is
//!   not behind the recording on any window — a session cannot vouch for
//!   numbers newer than its own.

use std::cmp::Ordering;
use std::fs;
use std::path::Path;

use jiff::Timestamp;
use meter_core::{UsageSnapshot, UsageWindow};

/// When the session behind `transcript` last heard from the API, or `now`
/// when that cannot be told.
///
/// Falling back to `now` is what the bridge did unconditionally before, so a
/// payload without a transcript path (an older Claude Code, a hand-fed test)
/// degrades to the old behaviour rather than to "infinitely old". An mtime in
/// the future — clock skew, a network filesystem — is clamped, so no reading
/// can claim to be fresher than the moment it was recorded.
#[must_use]
pub fn observed_at(transcript: Option<&Path>, now: Timestamp) -> Timestamp {
    transcript
        .and_then(|path| fs::metadata(path).ok()?.modified().ok())
        .and_then(|modified| Timestamp::try_from(modified).ok())
        .map_or(now, |modified| modified.min(now))
}

/// The newest knowledge of both windows across `recorded` and `incoming`.
///
/// Each window is taken from whichever reading is ahead on it, independently
/// — a session that reported only the weekly window leaves the recorded
/// 5-hour one alone. Equal readings keep the recorded one, so an idle session
/// echoing what is already there yields exactly `recorded` back.
#[must_use]
pub fn merge(recorded: Option<&UsageSnapshot>, incoming: UsageSnapshot) -> UsageSnapshot {
    let Some(recorded) = recorded else {
        return incoming;
    };
    let (five_hour, five_behind) = pick(recorded.five_hour.as_ref(), incoming.five_hour);
    let (seven_day, seven_behind) = pick(recorded.seven_day.as_ref(), incoming.seven_day);
    // A reading behind on either window is stale, so its observation time
    // vouches for nothing.
    let fetched_at = if five_behind || seven_behind {
        recorded.fetched_at
    } else {
        recorded.fetched_at.max(incoming.fetched_at)
    };
    UsageSnapshot {
        five_hour,
        seven_day,
        fetched_at,
        ..incoming
    }
}

/// The window to keep, and whether `incoming` was behind the recorded one.
fn pick(
    recorded: Option<&UsageWindow>,
    incoming: Option<UsageWindow>,
) -> (Option<UsageWindow>, bool) {
    match (recorded, incoming) {
        (Some(recorded), Some(incoming)) => match incoming.progress_cmp(recorded) {
            Ordering::Greater => (Some(incoming), false),
            ordering => (Some(recorded.clone()), ordering == Ordering::Less),
        },
        (recorded, incoming) => (incoming.or_else(|| recorded.cloned()), false),
    }
}

/// A file under `dir` whose mtime is `modified`, standing in for a session
/// transcript last written then.
#[cfg(test)]
pub(super) fn transcript_written_at(dir: &Path, modified: Timestamp) -> std::path::PathBuf {
    let transcript = dir.join("session.jsonl");
    #[allow(clippy::unwrap_used)]
    fs::File::create(&transcript)
        .and_then(|file| file.set_modified(modified.into()))
        .unwrap();
    transcript
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    // Utilization is carried through untouched, so exact equality pins which
    // reading won.
    #![allow(clippy::float_cmp)]

    use super::*;
    use meter_core::LimitWindow;
    use pretty_assertions::assert_eq;

    fn at(time: &str) -> Timestamp {
        time.parse().unwrap()
    }

    fn window(kind: LimitWindow, utilization: f64, resets_at: &str) -> UsageWindow {
        UsageWindow {
            utilization,
            resets_at: at(resets_at),
            window: kind,
        }
    }

    fn reading(five: Option<f64>, seven: Option<f64>, fetched_at: &str) -> UsageSnapshot {
        UsageSnapshot {
            five_hour: five.map(|u| window(LimitWindow::FiveHour, u, "2026-08-02T15:00:00Z")),
            seven_day: seven.map(|u| window(LimitWindow::SevenDay, u, "2026-08-05T09:00:00Z")),
            scoped: Vec::new(),
            spend: None,
            fetched_at: at(fetched_at),
        }
    }

    /// `snapshot` with its 5-hour window resetting at `resets_at` instead.
    fn five_hour_resetting_at(mut snapshot: UsageSnapshot, resets_at: &str) -> UsageSnapshot {
        snapshot.five_hour.as_mut().unwrap().resets_at = at(resets_at);
        snapshot
    }

    #[test]
    fn with_nothing_recorded_the_incoming_reading_is_taken_as_is() {
        let incoming = reading(Some(10.0), Some(20.0), "2026-08-02T12:00:00Z");
        assert_eq!(merge(None, incoming.clone()), incoming);
    }

    /// The bug this module exists for: an idle session re-reporting old
    /// numbers must not overwrite the active session's, nor refresh their age.
    #[test]
    fn an_idle_sessions_stale_numbers_lose_and_vouch_for_nothing() {
        let active = reading(Some(40.0), Some(62.0), "2026-08-02T12:00:00Z");
        // Its transcript mtime is the newer of the two — the prompt-just-typed
        // case — and it still loses, because its numbers are behind.
        let idle = reading(Some(25.0), Some(60.0), "2026-08-02T12:05:00Z");
        assert_eq!(merge(Some(&active), idle), active);
    }

    #[test]
    fn an_active_session_moving_further_into_the_window_wins() {
        let recorded = reading(Some(40.0), Some(62.0), "2026-08-02T12:00:00Z");
        let fresh = reading(Some(41.0), Some(62.0), "2026-08-02T12:03:00Z");
        assert_eq!(merge(Some(&recorded), fresh.clone()), fresh);
    }

    /// Utilization only moves in whole percents, so an active session often
    /// re-reports identical numbers. That still confirms them as of its
    /// latest response, or steady use would read as stale.
    #[test]
    fn equal_numbers_from_a_fresher_session_advance_the_age_only() {
        let recorded = reading(Some(40.0), Some(62.0), "2026-08-02T12:00:00Z");
        let confirmed = reading(Some(40.0), Some(62.0), "2026-08-02T12:09:00Z");
        assert_eq!(
            merge(Some(&recorded), confirmed).fetched_at,
            at("2026-08-02T12:09:00Z")
        );
    }

    #[test]
    fn equal_numbers_from_an_older_observation_never_turn_the_clock_back() {
        let recorded = reading(Some(40.0), Some(62.0), "2026-08-02T12:09:00Z");
        let echo = reading(Some(40.0), Some(62.0), "2026-08-02T10:00:00Z");
        assert_eq!(merge(Some(&recorded), echo), recorded);
    }

    /// A window that has reset starts again from a low number; the later
    /// reset is what makes it newer despite the drop.
    #[test]
    fn a_later_window_beats_a_higher_number_from_the_previous_one() {
        let recorded = reading(Some(97.0), Some(62.0), "2026-08-02T12:00:00Z");
        let rolled = five_hour_resetting_at(
            reading(Some(3.0), Some(63.0), "2026-08-02T15:30:00Z"),
            "2026-08-02T20:30:00Z",
        );
        assert_eq!(merge(Some(&recorded), rolled.clone()), rolled);
    }

    #[test]
    fn the_previous_window_reported_late_loses_to_the_current_one() {
        let current = five_hour_resetting_at(
            reading(Some(3.0), Some(63.0), "2026-08-02T15:30:00Z"),
            "2026-08-02T20:30:00Z",
        );
        let previous = reading(Some(97.0), Some(63.0), "2026-08-02T15:40:00Z");
        assert_eq!(merge(Some(&current), previous), current);
    }

    /// Two readings of one window whose rounded resets disagree slightly are
    /// still compared on utilization, not on which reset happens to be later.
    #[test]
    fn reset_rounding_jitter_does_not_make_a_lower_reading_newer() {
        let recorded = reading(Some(40.0), None, "2026-08-02T12:00:00Z");
        let jittered = five_hour_resetting_at(
            reading(Some(30.0), None, "2026-08-02T12:05:00Z"),
            "2026-08-02T15:00:30Z",
        );
        assert_eq!(merge(Some(&recorded), jittered), recorded);
    }

    #[test]
    fn a_window_the_session_did_not_report_keeps_its_recorded_value() {
        let recorded = reading(Some(40.0), Some(62.0), "2026-08-02T12:00:00Z");
        let weekly_only = reading(None, Some(63.0), "2026-08-02T12:03:00Z");
        let merged = merge(Some(&recorded), weekly_only);
        assert_eq!(merged.five_hour.unwrap().utilization, 40.0);
        assert_eq!(merged.seven_day.unwrap().utilization, 63.0);
        assert_eq!(merged.fetched_at, at("2026-08-02T12:03:00Z"));
    }

    /// Ahead on one window, behind on the other: each window takes its newer
    /// value, but the session's clock is not trusted for the one it lost.
    #[test]
    fn a_mixed_reading_takes_each_newer_window_but_keeps_the_recorded_age() {
        let recorded = reading(Some(40.0), Some(62.0), "2026-08-02T12:00:00Z");
        let mixed = reading(Some(41.0), Some(61.0), "2026-08-02T12:03:00Z");
        let merged = merge(Some(&recorded), mixed);
        assert_eq!(merged.five_hour.unwrap().utilization, 41.0);
        assert_eq!(merged.seven_day.unwrap().utilization, 62.0);
        assert_eq!(merged.fetched_at, at("2026-08-02T12:00:00Z"));
    }

    #[test]
    fn the_observation_time_is_the_transcripts_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let modified = at("2026-08-02T11:00:00Z");
        let transcript = transcript_written_at(dir.path(), modified);
        assert_eq!(
            observed_at(Some(&transcript), at("2026-08-02T12:00:00Z")),
            modified
        );
    }

    #[test]
    fn an_unknowable_observation_time_falls_back_to_now() {
        let now = at("2026-08-02T12:00:00Z");
        assert_eq!(observed_at(None, now), now);
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(observed_at(Some(&dir.path().join("missing")), now), now);
    }

    #[test]
    fn a_future_mtime_is_clamped_to_now() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = transcript_written_at(dir.path(), at("2026-08-03T00:00:00Z"));
        let now = at("2026-08-02T12:00:00Z");
        assert_eq!(observed_at(Some(&transcript), now), now);
    }
}
