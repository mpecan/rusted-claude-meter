//! Pure scheduler state machine: no I/O, no real clocks, no network.
//!
//! Every decision the polling loop makes — when to fetch, how long to wait,
//! whether the machine just woke from sleep, how stale the cached snapshot
//! is — lives here as a plain function of injected values, so all of it is
//! unit-testable without tokio or HTTP. The async driver in
//! [`super::run_loop`] only feeds this machine and executes its decisions.

use std::time::Duration;

use jiff::Timestamp;
use meter_core::UsageSnapshot;
use serde::{Deserialize, Serialize};

/// In-memory freshness window: a forced refresh (wake, manual, new key)
/// within this age is served from the cached snapshot instead of the
/// network. Slightly below the shortest polling interval so scheduled ticks
/// are never suppressed.
const MEMORY_TTL: Duration = Duration::from_secs(55);

/// First backoff step after a transient failure.
const BACKOFF_BASE_SECS: u32 = 10;
/// Backoff ceiling: never wait longer than this between retries.
const BACKOFF_CAP_SECS: u32 = 900;
/// Wall-clock time may run ahead of the monotonic clock by at most this many
/// seconds before we conclude the machine slept and force a refresh.
const DRIFT_THRESHOLD_SECS: f64 = 30.0;
/// A snapshot older than this many refresh intervals is reported stale.
const STALE_AFTER_INTERVALS: u32 = 2;

/// The user-selectable polling cadence (mirrors `ClaudeMeter`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RefreshInterval {
    #[default]
    OneMinute,
    FiveMinutes,
    TenMinutes,
}

impl RefreshInterval {
    pub const fn duration(self) -> Duration {
        match self {
            Self::OneMinute => Duration::from_mins(1),
            Self::FiveMinutes => Duration::from_mins(5),
            Self::TenMinutes => Duration::from_mins(10),
        }
    }
}

/// The result of one refresh attempt, already classified by the transport.
#[derive(Debug, Clone, PartialEq)]
pub enum FetchOutcome {
    Success(UsageSnapshot),
    /// No session key is stored; polling pauses until the user provides one.
    NoSession,
    /// HTTP 401: the key expired. Polling pauses — retrying cannot help and
    /// would only hammer the API (no retry storm).
    Unauthorized,
    /// Retryable failure (blocked, 5xx, network, decode): backs off
    /// exponentially with jitter.
    Transient,
}

/// Where the scheduler currently is, surfaced to the tray and UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Healthy: polling on the configured interval.
    Polling,
    /// Last refresh failed transiently; retrying with backoff.
    Degraded,
    /// No session key stored; waiting for the user.
    AwaitingSession,
    /// Session key rejected (401); waiting for a new key.
    SessionExpired,
}

/// Explicit freshness of the cached snapshot: a failed refresh never wipes
/// the last good data, so consumers must be told how old it is instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Staleness {
    /// No snapshot has ever been fetched (or restored from disk).
    Missing,
    Fresh,
    Stale,
}

/// The single source of truth broadcast to tray and UI via Tauri events.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeterState {
    pub snapshot: Option<UsageSnapshot>,
    pub staleness: Staleness,
    pub phase: Phase,
}

/// The scheduler's decision core. See the module docs for the design.
#[derive(Debug)]
pub struct SchedulerCore {
    interval: RefreshInterval,
    failures: u32,
    phase: Phase,
    snapshot: Option<UsageSnapshot>,
    last_wall: Option<Timestamp>,
    last_monotonic: Option<Duration>,
}

impl SchedulerCore {
    /// `initial` is the snapshot restored from the disk cache, if any, so a
    /// restart renders instantly.
    pub const fn new(interval: RefreshInterval, initial: Option<UsageSnapshot>) -> Self {
        Self {
            interval,
            failures: 0,
            phase: Phase::Polling,
            snapshot: initial,
            last_wall: None,
            last_monotonic: None,
        }
    }

    pub const fn set_interval(&mut self, interval: RefreshInterval) {
        self.interval = interval;
    }

    /// Whether a refresh attempt should actually hit the network.
    ///
    /// Scheduled ticks (`forced == false`) always fetch — the interval is
    /// longer than the TTL by construction. Forced refreshes (wake from
    /// sleep, manual, new session key) are served from the in-memory
    /// snapshot when it is younger than [`MEMORY_TTL`], so a burst of
    /// wake/manual events cannot hammer the API.
    pub fn should_fetch(&self, now: Timestamp, forced: bool) -> bool {
        if !forced {
            return true;
        }
        !self.snapshot.as_ref().is_some_and(|snapshot| {
            now.duration_since(snapshot.fetched_at).as_secs_f64() < MEMORY_TTL.as_secs_f64()
        })
    }

    /// Fold one refresh result into the state. A failure of any kind keeps
    /// the previous snapshot untouched.
    pub fn record(&mut self, outcome: FetchOutcome) {
        match outcome {
            FetchOutcome::Success(snapshot) => {
                self.snapshot = Some(snapshot);
                self.failures = 0;
                self.phase = Phase::Polling;
            }
            FetchOutcome::NoSession => {
                self.failures = 0;
                self.phase = Phase::AwaitingSession;
            }
            FetchOutcome::Unauthorized => {
                self.failures = 0;
                self.phase = Phase::SessionExpired;
            }
            FetchOutcome::Transient => {
                self.failures = self.failures.saturating_add(1);
                self.phase = Phase::Degraded;
            }
        }
    }

    /// Clear any paused/backing-off state so polling resumes, e.g. after the
    /// user stored a new session key or asked for a manual refresh.
    pub const fn resume(&mut self) {
        self.failures = 0;
        self.phase = Phase::Polling;
    }

    /// How long to wait before the next refresh attempt.
    ///
    /// `None` means "do not poll again until externally woken": the session
    /// is missing or expired and retrying cannot succeed. `jitter` must be
    /// in `0.0..=1.0`; it spreads backoff retries over the second half of
    /// the exponential step ("equal jitter") so many clients recovering from
    /// the same outage do not stampede.
    pub fn next_delay(&self, jitter: f64) -> Option<Duration> {
        match self.phase {
            Phase::AwaitingSession | Phase::SessionExpired => None,
            Phase::Polling => Some(self.interval.duration()),
            Phase::Degraded => Some(backoff_delay(self.failures, jitter)),
        }
    }

    /// Feed one (wall, monotonic) clock observation; returns `true` when the
    /// wall clock has run ahead of the monotonic clock by more than
    /// [`DRIFT_THRESHOLD_SECS`] since the previous observation — the
    /// signature of the machine having slept — in which case the caller
    /// should force a refresh. Cross-platform: monotonic clocks pause during
    /// system sleep on macOS and Linux while wall time keeps running.
    pub fn observe_clocks(&mut self, wall: Timestamp, monotonic: Duration) -> bool {
        let drifted = match (self.last_wall, self.last_monotonic) {
            (Some(last_wall), Some(last_monotonic)) => {
                let wall_delta = wall.duration_since(last_wall).as_secs_f64();
                let monotonic_delta = monotonic.saturating_sub(last_monotonic).as_secs_f64();
                wall_delta - monotonic_delta > DRIFT_THRESHOLD_SECS
            }
            _ => false,
        };
        self.last_wall = Some(wall);
        self.last_monotonic = Some(monotonic);
        drifted
    }

    /// The broadcastable view of the current state.
    pub fn state(&self, now: Timestamp) -> MeterState {
        MeterState {
            snapshot: self.snapshot.clone(),
            staleness: self.staleness(now),
            phase: self.phase,
        }
    }

    fn staleness(&self, now: Timestamp) -> Staleness {
        self.snapshot
            .as_ref()
            .map_or(Staleness::Missing, |snapshot| {
                let age = now.duration_since(snapshot.fetched_at).as_secs_f64();
                let stale_after =
                    self.interval.duration().as_secs_f64() * f64::from(STALE_AFTER_INTERVALS);
                if age > stale_after {
                    Staleness::Stale
                } else {
                    Staleness::Fresh
                }
            })
    }
}

/// Exponential backoff with equal jitter, capped at [`BACKOFF_CAP_SECS`]:
/// `delay = step * (0.5 + 0.5 * jitter)` where `step = base * 2^(n-1)`.
fn backoff_delay(failures: u32, jitter: f64) -> Duration {
    let shift = failures.saturating_sub(1).min(16);
    let step = BACKOFF_BASE_SECS
        .saturating_mul(1 << shift)
        .min(BACKOFF_CAP_SECS);
    let scaled = f64::from(step) * jitter.clamp(0.0, 1.0).mul_add(0.5, 0.5);
    Duration::from_secs_f64(scaled)
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

    fn snapshot_at(fetched_at: Timestamp) -> UsageSnapshot {
        UsageSnapshot {
            five_hour: None,
            seven_day: None,
            scoped: vec![],
            spend: None,
            fetched_at,
        }
    }

    fn core_with_snapshot(age_secs: i64) -> SchedulerCore {
        SchedulerCore::new(
            RefreshInterval::OneMinute,
            Some(snapshot_at(now() - SignedDuration::from_secs(age_secs))),
        )
    }

    #[test]
    fn intervals_mirror_claude_meter() {
        assert_eq!(
            RefreshInterval::OneMinute.duration(),
            Duration::from_mins(1)
        );
        assert_eq!(
            RefreshInterval::FiveMinutes.duration(),
            Duration::from_mins(5)
        );
        assert_eq!(
            RefreshInterval::TenMinutes.duration(),
            Duration::from_mins(10)
        );
    }

    #[test]
    fn healthy_cadence_is_the_configured_interval() {
        let mut core = SchedulerCore::new(RefreshInterval::FiveMinutes, None);
        core.record(FetchOutcome::Success(snapshot_at(now())));
        assert_eq!(core.next_delay(0.7), Some(Duration::from_mins(5)));

        core.set_interval(RefreshInterval::TenMinutes);
        assert_eq!(core.next_delay(0.2), Some(Duration::from_mins(10)));
    }

    #[test]
    fn backoff_doubles_and_caps_at_full_jitter() {
        let mut core = SchedulerCore::new(RefreshInterval::OneMinute, None);
        let mut delays = Vec::new();
        for _ in 0..9 {
            core.record(FetchOutcome::Transient);
            delays.push(core.next_delay(1.0).unwrap().as_secs());
        }
        assert_eq!(delays, vec![10, 20, 40, 80, 160, 320, 640, 900, 900]);
    }

    #[test]
    fn backoff_jitter_spreads_over_the_second_half_of_the_step() {
        let mut core = SchedulerCore::new(RefreshInterval::OneMinute, None);
        core.record(FetchOutcome::Transient);
        core.record(FetchOutcome::Transient);
        // Step is 20s: jitter 0.0 → 10s, jitter 0.5 → 15s, jitter 1.0 → 20s.
        assert_eq!(core.next_delay(0.0), Some(Duration::from_secs(10)));
        assert_eq!(core.next_delay(0.5), Some(Duration::from_secs(15)));
        assert_eq!(core.next_delay(1.0), Some(Duration::from_secs(20)));
    }

    #[test]
    fn backoff_saturates_instead_of_overflowing_on_many_failures() {
        let mut core = SchedulerCore::new(RefreshInterval::OneMinute, None);
        for _ in 0..1000 {
            core.record(FetchOutcome::Transient);
        }
        assert_eq!(core.next_delay(1.0), Some(Duration::from_mins(15)));
    }

    #[test]
    fn success_resets_the_backoff_sequence() {
        let mut core = SchedulerCore::new(RefreshInterval::OneMinute, None);
        core.record(FetchOutcome::Transient);
        core.record(FetchOutcome::Transient);
        core.record(FetchOutcome::Success(snapshot_at(now())));
        assert_eq!(core.next_delay(1.0), Some(Duration::from_mins(1)));
        // The next failure starts the sequence from the beginning.
        core.record(FetchOutcome::Transient);
        assert_eq!(core.next_delay(1.0), Some(Duration::from_secs(10)));
    }

    #[test]
    fn unauthorized_pauses_polling_entirely() {
        let mut core = SchedulerCore::new(RefreshInterval::OneMinute, None);
        core.record(FetchOutcome::Unauthorized);
        assert_eq!(core.next_delay(0.5), None);
        assert_eq!(core.state(now()).phase, Phase::SessionExpired);
    }

    #[test]
    fn missing_session_pauses_polling_entirely() {
        let mut core = SchedulerCore::new(RefreshInterval::OneMinute, None);
        core.record(FetchOutcome::NoSession);
        assert_eq!(core.next_delay(0.5), None);
        assert_eq!(core.state(now()).phase, Phase::AwaitingSession);
    }

    #[test]
    fn resume_restores_polling_after_expiry() {
        let mut core = SchedulerCore::new(RefreshInterval::OneMinute, None);
        core.record(FetchOutcome::Unauthorized);
        core.resume();
        assert_eq!(core.next_delay(0.5), Some(Duration::from_mins(1)));
        assert_eq!(core.state(now()).phase, Phase::Polling);
    }

    #[test]
    fn failures_never_wipe_the_last_good_snapshot() {
        let mut core = SchedulerCore::new(RefreshInterval::OneMinute, None);
        let snapshot = snapshot_at(now());
        core.record(FetchOutcome::Success(snapshot.clone()));
        core.record(FetchOutcome::Transient);
        core.record(FetchOutcome::Unauthorized);
        core.record(FetchOutcome::NoSession);
        assert_eq!(core.state(now()).snapshot, Some(snapshot));
    }

    #[test]
    fn scheduled_ticks_always_fetch() {
        let core = core_with_snapshot(1);
        assert!(core.should_fetch(now(), false));
    }

    #[test]
    fn forced_refresh_is_served_from_a_fresh_snapshot() {
        assert!(!core_with_snapshot(10).should_fetch(now(), true));
        assert!(!core_with_snapshot(54).should_fetch(now(), true));
    }

    #[test]
    fn forced_refresh_fetches_once_the_ttl_expires() {
        assert!(core_with_snapshot(55).should_fetch(now(), true));
        assert!(core_with_snapshot(120).should_fetch(now(), true));
    }

    #[test]
    fn forced_refresh_fetches_when_nothing_is_cached() {
        let core = SchedulerCore::new(RefreshInterval::OneMinute, None);
        assert!(core.should_fetch(now(), true));
    }

    #[test]
    fn first_clock_observation_never_reports_drift() {
        let mut core = SchedulerCore::new(RefreshInterval::OneMinute, None);
        assert!(!core.observe_clocks(now(), Duration::from_secs(100)));
    }

    #[test]
    fn matched_clock_advances_are_not_drift() {
        let mut core = SchedulerCore::new(RefreshInterval::OneMinute, None);
        core.observe_clocks(now(), Duration::from_secs(100));
        let drifted = core.observe_clocks(
            now() + SignedDuration::from_secs(30),
            Duration::from_secs(130),
        );
        assert!(!drifted);
    }

    #[test]
    fn wall_clock_running_ahead_of_monotonic_is_drift() {
        // Asleep for 10 minutes: wall advanced 630s, monotonic only 30s.
        let mut core = SchedulerCore::new(RefreshInterval::OneMinute, None);
        core.observe_clocks(now(), Duration::from_secs(100));
        let drifted = core.observe_clocks(
            now() + SignedDuration::from_secs(630),
            Duration::from_secs(130),
        );
        assert!(drifted);
    }

    #[test]
    fn drift_just_under_the_threshold_is_tolerated() {
        let mut core = SchedulerCore::new(RefreshInterval::OneMinute, None);
        core.observe_clocks(now(), Duration::from_secs(100));
        let drifted = core.observe_clocks(
            now() + SignedDuration::from_secs(59),
            Duration::from_secs(130),
        );
        assert!(!drifted);
    }

    #[test]
    fn drift_detection_resets_its_baseline_after_firing() {
        let mut core = SchedulerCore::new(RefreshInterval::OneMinute, None);
        core.observe_clocks(now(), Duration::from_secs(100));
        assert!(core.observe_clocks(
            now() + SignedDuration::from_secs(630),
            Duration::from_secs(130)
        ));
        // The next matched advance is measured from the new baseline.
        assert!(!core.observe_clocks(
            now() + SignedDuration::from_secs(660),
            Duration::from_secs(160)
        ));
    }

    #[test]
    fn staleness_is_missing_without_a_snapshot() {
        let core = SchedulerCore::new(RefreshInterval::OneMinute, None);
        assert_eq!(core.state(now()).staleness, Staleness::Missing);
    }

    #[test]
    fn staleness_flips_after_two_missed_intervals() {
        assert_eq!(
            core_with_snapshot(60).state(now()).staleness,
            Staleness::Fresh
        );
        assert_eq!(
            core_with_snapshot(121).state(now()).staleness,
            Staleness::Stale
        );
    }

    #[test]
    fn meter_state_serializes_snake_case_discriminants() {
        let mut core = SchedulerCore::new(RefreshInterval::OneMinute, None);
        core.record(FetchOutcome::Unauthorized);
        let json = serde_json::to_value(core.state(now())).unwrap();
        assert_eq!(json["phase"], "session_expired");
        assert_eq!(json["staleness"], "missing");
        assert_eq!(json["snapshot"], serde_json::Value::Null);
    }
}
