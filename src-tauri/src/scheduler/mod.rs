//! Polling scheduler: keeps a current [`MeterState`] available at all times.
//!
//! Split in three: [`core`] is the pure decision machine (cadence, backoff,
//! TTL, drift, staleness), [`transport`] performs one classified refresh
//! attempt, and [`run_loop`] here is the thin async driver that wires them
//! together under real time. The driver is generic over [`Clock`] and
//! [`UsageTransport`], so its behaviour is tested with fakes and tokio's
//! paused time — no real network or wall-clock waits.

pub mod core;
#[cfg(test)]
mod mock_integration;
#[cfg(test)]
mod test_support;
pub mod transport;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use jiff::Timestamp;
use tokio::sync::Notify;

pub use self::core::{MeterState, Phase, RefreshInterval, SchedulerCore, Staleness};
pub use self::transport::LiveTransport;

use self::core::FetchOutcome;
use self::transport::UsageTransport;

use crate::cache;
use crate::export;

/// Tauri event carrying a [`MeterState`] payload on every change.
pub const USAGE_STATE_EVENT: &str = "usage-state";

/// Upper bound on one uninterrupted sleep, so wake-from-sleep drift is
/// noticed within this long of resuming even mid way through a long delay.
const DRIFT_CHECK_SLICE: Duration = Duration::from_secs(30);

/// Time source for the driver. Wall and monotonic readings are separate on
/// purpose: their divergence is how sleep is detected (see
/// [`SchedulerCore::observe_clocks`]).
pub trait Clock: Send + Sync {
    fn wall(&self) -> Timestamp;
    /// Monotonic reading since an arbitrary fixed origin. Pauses during
    /// system sleep on macOS and Linux.
    fn monotonic(&self) -> Duration;
}

/// Production clock: `jiff` wall time + `std::time::Instant`.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    origin: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn wall(&self) -> Timestamp {
        Timestamp::now()
    }

    fn monotonic(&self) -> Duration {
        self.origin.elapsed()
    }
}

/// Managed Tauri state: lets commands read the current state and wake the
/// polling loop (new session key, manual refresh, interval change). Cloning
/// is shallow — every clone shares the same core and wakeup channel, which
/// is how the command surface and [`run_loop`] stay in sync.
#[derive(Clone)]
pub struct SchedulerHandle {
    core: Arc<Mutex<SchedulerCore>>,
    notify: Arc<Notify>,
}

impl SchedulerHandle {
    pub const fn new(core: Arc<Mutex<SchedulerCore>>, notify: Arc<Notify>) -> Self {
        Self { core, notify }
    }

    /// Wake the loop for a TTL-guarded refresh attempt. The loop broadcasts
    /// state after every wakeup, even when the TTL serves it from memory.
    pub fn request_refresh(&self) {
        self.notify.notify_one();
    }

    /// The user stored a new session key: clear any parked
    /// (expired/awaiting-session) or backoff phase, then wake the loop for a
    /// TTL-guarded attempt with the new key.
    pub fn resume_polling(&self) {
        lock(&self.core).resume();
        self.notify.notify_one();
    }

    /// The session key is gone: record it directly — no fetch is needed to
    /// learn it — and wake the loop so the awaiting-session state is
    /// broadcast immediately instead of on the next scheduled tick.
    pub fn mark_no_session(&self) {
        lock(&self.core).record(FetchOutcome::NoSession);
        self.notify.notify_one();
    }

    /// Change the polling cadence and reschedule immediately.
    pub fn set_interval(&self, interval: RefreshInterval) {
        lock(&self.core).set_interval(interval);
        self.notify.notify_one();
    }

    /// Current state snapshot, for pull-style consumers (initial UI render).
    pub fn state_now(&self) -> MeterState {
        lock(&self.core).state(Timestamp::now())
    }
}

fn lock(core: &Mutex<SchedulerCore>) -> MutexGuard<'_, SchedulerCore> {
    core.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Where a successful fetch is persisted, bundled into one value so
/// [`run_loop`] stays under the workspace's `too_many_arguments` limit. Both
/// are independently optional: the disk cache needs the app data dir to
/// resolve, and the public export (issue #8) needs the home dir — either can
/// fail to resolve on an unusual platform without the other being affected.
#[derive(Debug, Clone, Default)]
pub struct PersistPaths {
    pub cache: Option<PathBuf>,
    pub export: Option<PathBuf>,
}

/// Jitter in `0.0..1.0` derived from the wall clock's sub-second nanos —
/// plenty for spreading retries without pulling in an RNG dependency.
fn wall_jitter(wall: Timestamp) -> f64 {
    f64::from(wall.subsec_nanosecond().unsigned_abs()) / 1e9
}

/// The polling loop. Runs forever; every decision is delegated to
/// [`SchedulerCore`] via the shared `handle`. `persist.cache` (when given)
/// receives the latest good snapshot after every successful fetch so
/// restarts render instantly; `persist.export` (when given) receives the
/// same snapshot mapped into the public `usage.json` contract for external
/// consumers (issue #8).
pub async fn run_loop<T: UsageTransport, C: Clock>(
    transport: T,
    clock: C,
    handle: SchedulerHandle,
    persist: PersistPaths,
    on_state: impl Fn(MeterState) + Send + Sync + 'static,
) {
    let SchedulerHandle { core, notify } = handle;
    lock(&core).observe_clocks(clock.wall(), clock.monotonic());
    // Broadcast the restored-from-disk state before the first fetch.
    on_state(lock(&core).state(clock.wall()));

    // The first attempt is "forced": served from the disk cache when that is
    // still within the TTL, so an app restart does not double-hit the API.
    let mut forced = true;
    loop {
        if lock(&core).should_fetch(clock.wall(), forced) {
            let outcome = transport.fetch().await;
            if let FetchOutcome::Success(snapshot) = &outcome {
                if let Some(path) = &persist.cache {
                    // Cache write failure is not a refresh failure; the
                    // in-memory snapshot stays authoritative.
                    let _ = cache::save(path, snapshot);
                }
                if let Some(path) = &persist.export {
                    // Same discipline: logged, never fatal to the refresh
                    // (issue #8's acceptance criterion).
                    if let Err(error) = export::write(path, snapshot) {
                        eprintln!("usage.json export failed: {error}");
                    }
                }
            }
            lock(&core).record(outcome);
        }
        // Broadcast unconditionally: even a wakeup the TTL served from
        // memory may carry a local state change (session key cleared or
        // replaced), and it must become observable now, not next tick.
        on_state(lock(&core).state(clock.wall()));

        let delay = lock(&core).next_delay(wall_jitter(clock.wall()));
        forced = match delay {
            // Paused: nothing to retry until an external wakeup. The wakeup
            // itself decides whether polling resumes (`resume_polling`) —
            // a plain refresh request on a dead session stays parked.
            None => {
                notify.notified().await;
                true
            }
            Some(delay) => wait_for_next_tick(&clock, &core, &notify, delay).await,
        };
    }
}

/// Sleep `delay` in slices, watching for external wakeups and for
/// wall-vs-monotonic drift (wake from sleep). Returns whether the next
/// fetch attempt should be treated as forced (TTL-guarded).
async fn wait_for_next_tick<C: Clock>(
    clock: &C,
    core: &Mutex<SchedulerCore>,
    notify: &Notify,
    delay: Duration,
) -> bool {
    let mut remaining = delay;
    while !remaining.is_zero() {
        let slice = remaining.min(DRIFT_CHECK_SLICE);
        tokio::select! {
            () = notify.notified() => return true,
            () = tokio::time::sleep(slice) => {}
        }
        if lock(core).observe_clocks(clock.wall(), clock.monotonic()) {
            return true;
        }
        remaining = remaining.saturating_sub(slice);
    }
    false
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::core::Phase;
    use super::*;
    use jiff::SignedDuration;
    use meter_core::UsageSnapshot;
    use pretty_assertions::assert_eq;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn base() -> Timestamp {
        "2026-07-17T12:00:00Z".parse().unwrap()
    }

    fn snapshot_at(fetched_at: Timestamp) -> UsageSnapshot {
        UsageSnapshot {
            five_hour: None,
            seven_day: None,
            scoped: vec![],
            fetched_at,
        }
    }

    fn empty_snapshot() -> UsageSnapshot {
        snapshot_at(Timestamp::now())
    }

    /// Test clock with independently advancing wall and monotonic readings —
    /// unequal advances simulate system sleep (monotonic pauses, wall runs).
    #[derive(Clone)]
    struct FakeClock {
        now: Arc<Mutex<(Timestamp, Duration)>>,
    }

    impl FakeClock {
        fn new(wall: Timestamp) -> Self {
            Self {
                now: Arc::new(Mutex::new((wall, Duration::ZERO))),
            }
        }

        fn advance(&self, wall_secs: i64, monotonic_secs: u64) {
            let mut now = self.now.lock().unwrap();
            now.0 += SignedDuration::from_secs(wall_secs);
            now.1 += Duration::from_secs(monotonic_secs);
        }
    }

    impl Clock for FakeClock {
        fn wall(&self) -> Timestamp {
            self.now.lock().unwrap().0
        }

        fn monotonic(&self) -> Duration {
            self.now.lock().unwrap().1
        }
    }

    /// Counts fetches; returns the outcomes it was scripted with, repeating
    /// the last one forever.
    struct ScriptedTransport {
        count: Arc<AtomicUsize>,
        script: Vec<FetchOutcome>,
    }

    impl ScriptedTransport {
        fn success(count: Arc<AtomicUsize>) -> Self {
            Self {
                count,
                script: vec![],
            }
        }
    }

    impl UsageTransport for ScriptedTransport {
        fn fetch(&self) -> impl Future<Output = FetchOutcome> + Send {
            let attempt = self.count.fetch_add(1, Ordering::SeqCst);
            let outcome = self
                .script
                .get(attempt)
                .or_else(|| self.script.last())
                .cloned()
                .unwrap_or_else(|| FetchOutcome::Success(empty_snapshot()));
            async move { outcome }
        }
    }

    fn spawn_loop(
        transport: ScriptedTransport,
        core: Arc<Mutex<SchedulerCore>>,
        notify: Arc<Notify>,
    ) -> tokio::task::JoinHandle<()> {
        spawn_loop_with(transport, SystemClock::default(), core, notify, |_| {})
    }

    fn spawn_loop_with<C: Clock + 'static>(
        transport: ScriptedTransport,
        clock: C,
        core: Arc<Mutex<SchedulerCore>>,
        notify: Arc<Notify>,
        on_state: impl Fn(MeterState) + Send + Sync + 'static,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(run_loop(
            transport,
            clock,
            SchedulerHandle::new(core, notify),
            PersistPaths::default(),
            on_state,
        ))
    }

    /// Yield virtual time until `predicate` holds (bounded, so a broken loop
    /// fails the test instead of hanging it).
    async fn wait_until(predicate: impl Fn() -> bool) {
        for _ in 0..1000 {
            if predicate() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(predicate(), "condition not reached in virtual time");
    }

    #[tokio::test(start_paused = true)]
    async fn loop_fetches_immediately_and_then_on_cadence() {
        let count = Arc::new(AtomicUsize::new(0));
        let core = Arc::new(Mutex::new(SchedulerCore::new(
            RefreshInterval::OneMinute,
            None,
        )));
        let notify = Arc::new(Notify::new());
        let task = spawn_loop(
            ScriptedTransport::success(Arc::clone(&count)),
            Arc::clone(&core),
            Arc::clone(&notify),
        );

        // t = 0, 60, 120, 180 → four fetches by t = 190.
        tokio::time::sleep(Duration::from_secs(190)).await;
        task.abort();
        assert_eq!(count.load(Ordering::SeqCst), 4);
    }

    #[tokio::test(start_paused = true)]
    async fn expired_session_parks_the_loop_until_notified() {
        let count = Arc::new(AtomicUsize::new(0));
        let core = Arc::new(Mutex::new(SchedulerCore::new(
            RefreshInterval::OneMinute,
            None,
        )));
        let notify = Arc::new(Notify::new());
        let transport = ScriptedTransport {
            count: Arc::clone(&count),
            script: vec![
                FetchOutcome::Unauthorized,
                FetchOutcome::Success(empty_snapshot()),
            ],
        };
        let task = spawn_loop(transport, Arc::clone(&core), Arc::clone(&notify));

        wait_until(|| lock(&core).state(Timestamp::now()).phase == Phase::SessionExpired).await;
        // Parked: hours of virtual time pass with no further attempts.
        tokio::time::sleep(Duration::from_hours(1)).await;
        assert_eq!(count.load(Ordering::SeqCst), 1);

        // A new session key wakes it and polling resumes.
        SchedulerHandle::new(Arc::clone(&core), Arc::clone(&notify)).resume_polling();
        wait_until(|| count.load(Ordering::SeqCst) >= 2).await;
        wait_until(|| lock(&core).state(Timestamp::now()).phase == Phase::Polling).await;
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn transient_failures_back_off_instead_of_polling_on_cadence() {
        let count = Arc::new(AtomicUsize::new(0));
        let core = Arc::new(Mutex::new(SchedulerCore::new(
            RefreshInterval::OneMinute,
            None,
        )));
        let notify = Arc::new(Notify::new());
        let transport = ScriptedTransport {
            count: Arc::clone(&count),
            script: vec![FetchOutcome::Transient],
        };
        let task = spawn_loop(transport, Arc::clone(&core), Arc::clone(&notify));

        // Backoff steps are 5–10, 10–20, 20–40, 40–80s (jitter-dependent):
        // after 26s at most three attempts can have fired (5+10+20 > 26 in
        // the fastest case means attempt 4 cannot arrive before t = 35).
        tokio::time::sleep(Duration::from_secs(26)).await;
        let after_26s = count.load(Ordering::SeqCst);
        assert!(
            (2..=4).contains(&after_26s),
            "expected backoff pacing, got {after_26s} attempts in 26s"
        );
        // On the plain 60s cadence there would be ~60 attempts by now.
        tokio::time::sleep(Duration::from_secs(3600 - 26)).await;
        let after_1h = count.load(Ordering::SeqCst);
        assert!(
            after_1h <= 30,
            "expected capped backoff, got {after_1h} attempts in an hour"
        );
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn clock_jump_mid_wait_forces_an_early_refresh() {
        let count = Arc::new(AtomicUsize::new(0));
        let clock = FakeClock::new(base());
        let core = Arc::new(Mutex::new(SchedulerCore::new(
            RefreshInterval::TenMinutes,
            None,
        )));
        let notify = Arc::new(Notify::new());
        let transport = ScriptedTransport {
            count: Arc::clone(&count),
            script: vec![FetchOutcome::Success(snapshot_at(base()))],
        };
        let task = spawn_loop_with(
            transport,
            clock.clone(),
            Arc::clone(&core),
            Arc::clone(&notify),
            |_| {},
        );

        wait_until(|| count.load(Ordering::SeqCst) == 1).await;
        // One drift-check slice with matched (zero) advances: no refresh.
        tokio::time::sleep(DRIFT_CHECK_SLICE).await;
        assert_eq!(count.load(Ordering::SeqCst), 1);

        // Asleep for ten minutes: wall runs ahead, monotonic stands still.
        // The next slice notices the drift and forces a refresh mid-wait,
        // long before the 10-minute interval would have elapsed.
        clock.advance(600, 0);
        tokio::time::sleep(DRIFT_CHECK_SLICE).await;
        wait_until(|| count.load(Ordering::SeqCst) == 2).await;
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn refresh_request_mid_wait_fetches_early_once_ttl_expires() {
        let count = Arc::new(AtomicUsize::new(0));
        let clock = FakeClock::new(base());
        let core = Arc::new(Mutex::new(SchedulerCore::new(
            RefreshInterval::TenMinutes,
            None,
        )));
        let notify = Arc::new(Notify::new());
        let handle = SchedulerHandle::new(Arc::clone(&core), Arc::clone(&notify));
        let transport = ScriptedTransport {
            count: Arc::clone(&count),
            script: vec![FetchOutcome::Success(snapshot_at(base()))],
        };
        let task = spawn_loop_with(transport, clock.clone(), core, notify, |_| {});

        wait_until(|| count.load(Ordering::SeqCst) == 1).await;
        // Fresh snapshot: the request is served from memory, no fetch.
        handle.request_refresh();
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(count.load(Ordering::SeqCst), 1);

        // Once the snapshot ages past the TTL, the same request interrupts
        // the in-progress wait instead of waiting out the full interval.
        clock.advance(60, 60);
        handle.request_refresh();
        wait_until(|| count.load(Ordering::SeqCst) == 2).await;
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn cleared_session_broadcasts_awaiting_state_without_a_fetch() {
        let count = Arc::new(AtomicUsize::new(0));
        let clock = FakeClock::new(base());
        let core = Arc::new(Mutex::new(SchedulerCore::new(
            RefreshInterval::OneMinute,
            None,
        )));
        let notify = Arc::new(Notify::new());
        let handle = SchedulerHandle::new(Arc::clone(&core), Arc::clone(&notify));
        let phases = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&phases);
        let transport = ScriptedTransport {
            count: Arc::clone(&count),
            script: vec![FetchOutcome::Success(snapshot_at(base()))],
        };
        let task = spawn_loop_with(transport, clock.clone(), core, notify, move |state| {
            sink.lock().unwrap().push(state.phase);
        });

        wait_until(|| count.load(Ordering::SeqCst) == 1).await;
        // The snapshot is fresh, so the wakeup fetches nothing — but the
        // cleared session must still be broadcast immediately, not on the
        // next scheduled tick.
        handle.mark_no_session();
        wait_until(|| phases.lock().unwrap().last() == Some(&Phase::AwaitingSession)).await;
        assert_eq!(count.load(Ordering::SeqCst), 1);

        // And the loop parks: hours pass with no further attempts.
        tokio::time::sleep(Duration::from_hours(1)).await;
        assert_eq!(count.load(Ordering::SeqCst), 1);
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn successful_fetch_writes_both_the_cache_and_the_public_export() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("usage_cache.json");
        let export_path = dir.path().join(".claudemeter").join("usage.json");
        let count = Arc::new(AtomicUsize::new(0));
        let core = Arc::new(Mutex::new(SchedulerCore::new(
            RefreshInterval::OneMinute,
            None,
        )));
        let notify = Arc::new(Notify::new());
        let handle = SchedulerHandle::new(Arc::clone(&core), Arc::clone(&notify));
        let transport = ScriptedTransport {
            count: Arc::clone(&count),
            script: vec![FetchOutcome::Success(snapshot_at(base()))],
        };
        let task = tokio::spawn(run_loop(
            transport,
            FakeClock::new(base()),
            handle,
            PersistPaths {
                cache: Some(cache_path.clone()),
                export: Some(export_path.clone()),
            },
            |_| {},
        ));

        wait_until(|| count.load(Ordering::SeqCst) == 1).await;
        assert!(cache_path.exists(), "disk cache was not written");
        assert!(export_path.exists(), "public export was not written");
        let exported: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&export_path).unwrap()).unwrap();
        assert_eq!(exported["last_updated"], "2026-07-17T12:00:00Z");
        task.abort();
    }

    /// Pins the acceptance criterion "export failures are logged but never
    /// fail the refresh" as actual behaviour, not just a code comment: point
    /// `persist.export` at a path whose parent already exists as a *regular
    /// file*, so `fs::create_dir_all` fails deterministically and portably
    /// (a directory can never be created where a file already sits) — then
    /// assert the loop still records the fetch as a success (state phase
    /// stays `Polling`, the disk cache is still written, no panic) despite
    /// the export write failing.
    #[tokio::test(start_paused = true)]
    async fn export_write_failure_does_not_fail_the_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("usage_cache.json");
        // `blocker` is a regular file, so `create_dir_all(dir/"blocker")`
        // (export.rs's parent-directory step for a path nested under it)
        // must fail.
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"not a directory").unwrap();
        let export_path = blocker.join("usage.json");

        let count = Arc::new(AtomicUsize::new(0));
        let core = Arc::new(Mutex::new(SchedulerCore::new(
            RefreshInterval::OneMinute,
            None,
        )));
        let notify = Arc::new(Notify::new());
        let handle = SchedulerHandle::new(Arc::clone(&core), Arc::clone(&notify));
        let phases = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&phases);
        let transport = ScriptedTransport {
            count: Arc::clone(&count),
            script: vec![FetchOutcome::Success(snapshot_at(base()))],
        };
        let task = tokio::spawn(run_loop(
            transport,
            FakeClock::new(base()),
            handle,
            PersistPaths {
                cache: Some(cache_path.clone()),
                export: Some(export_path.clone()),
            },
            move |state| sink.lock().unwrap().push(state.phase),
        ));

        wait_until(|| count.load(Ordering::SeqCst) == 1).await;
        wait_until(|| !phases.lock().unwrap().is_empty()).await;

        assert!(
            cache_path.exists(),
            "disk cache must still be written when the export write fails"
        );
        assert!(
            !export_path.exists(),
            "export write was expected to fail, but a file appeared"
        );
        assert_eq!(
            phases.lock().unwrap().last(),
            Some(&Phase::Polling),
            "a failed export write must not turn a successful fetch into a non-Polling phase"
        );
        task.abort();
    }

    #[test]
    fn handle_reports_state_and_reschedules_on_interval_change() {
        let core = Arc::new(Mutex::new(SchedulerCore::new(
            RefreshInterval::OneMinute,
            None,
        )));
        let handle = SchedulerHandle::new(Arc::clone(&core), Arc::new(Notify::new()));
        assert_eq!(handle.state_now().phase, Phase::Polling);

        handle.set_interval(RefreshInterval::TenMinutes);
        assert_eq!(lock(&core).next_delay(0.0), Some(Duration::from_mins(10)));
    }

    #[test]
    fn wall_jitter_is_always_in_unit_range() {
        for secs in [0i64, 1, 1_752_753_600, i64::from(u32::MAX)] {
            let jitter = wall_jitter(Timestamp::new(secs, 999_999_999).unwrap());
            assert!((0.0..1.0).contains(&jitter), "jitter {jitter} out of range");
        }
    }
}
