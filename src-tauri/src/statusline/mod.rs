//! The `~/.claudemeter/statusline.json` contract — both directions.
//!
//! This module owns the file itself: what it holds, where it lives, how it is
//! written and how it is read back. [`bridge`] owns the other half, the
//! Claude Code status-line payload that feeds it.
//!
//! The file exists because Claude Code will hand plan usage to any command the
//! user names in `statusLine.command`, and it derives those numbers from
//! `anthropic-ratelimit-unified-*` response headers on the user's *own* API
//! traffic. So the whole path originates no claude.ai request, which makes it
//! the one usage source that sidesteps the Terms-of-Service problem in
//! `docs/terms-of-service.md` and needs no [`crate::consent`] gate.
//!
//! **The two files in `~/.claudemeter/` flow opposite ways**, which is why
//! this is not merged into `export.rs`:
//! - `usage.json` is this app's *output*, for external scripts — last writer
//!   wins with the Swift `ClaudeMeter`, no merging.
//! - `statusline.json` is this app's *input*, written by a Claude Code
//!   subprocess and read by the scheduler.
//!
//! Mixing both directions into one path would make "who wrote this, and may I
//! trust it?" unanswerable.

mod bridge;
pub mod setup;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use meter_core::{LimitWindow, UsageSnapshot, UsageWindow};
use serde::{Deserialize, Serialize};

use crate::export::{ExportLimit, claudemeter_path};
use crate::io_util::atomic_write;

pub use bridge::{SUBCOMMAND, execute, parse_args};
pub use setup::current_command;

/// File name inside `~/.claudemeter/`, beside `export.rs`'s `usage.json`.
/// Join it with [`crate::export::claudemeter_path`].
pub const STATUSLINE_FILE: &str = "statusline.json";
/// Bumped whenever [`StatusLinePayload`] changes shape incompatibly.
///
/// [`read`] accepts anything at or below this, so a newer app still reads a
/// file left by an older bridge. A *higher* number means the file was written
/// by a build that knows something this one does not, and is refused rather
/// than guessed at.
pub const SCHEMA_VERSION: u32 = 1;

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
    /// When the bridge observed the reading. This becomes the snapshot's
    /// `fetched_at`, so the scheduler's existing staleness rule reports
    /// "Claude Code has not run recently" with no special-casing.
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

/// Rebuild a domain window from the narrower on-disk shape, which carries no
/// window length of its own — the field it was read from names it.
const fn window(limit: &ExportLimit, kind: LimitWindow) -> UsageWindow {
    UsageWindow {
        utilization: limit.utilization,
        resets_at: limit.reset_at,
        window: kind,
    }
}

/// The recorded reading, or `None` when there is nothing usable to report.
///
/// Every failure collapses to `None` on purpose — absent, unreadable,
/// truncated, written by a newer build, or carrying no window at all. The
/// caller's remedy is identical in each case ("Claude Code has not reported
/// usage yet"), and a reader that distinguished them would be inventing
/// detail it cannot act on. Freshness is *not* judged here: the snapshot
/// carries `recorded_at` as its `fetched_at` and the scheduler's existing
/// staleness rule decides what counts as old.
#[must_use]
pub fn read(path: &Path) -> Option<UsageSnapshot> {
    let payload: StatusLinePayload = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    if payload.schema > SCHEMA_VERSION {
        return None;
    }
    let five_hour = payload
        .session_usage
        .as_ref()
        .map(|limit| window(limit, LimitWindow::FiveHour));
    let seven_day = payload
        .weekly_usage
        .as_ref()
        .map(|limit| window(limit, LimitWindow::SevenDay));
    if five_hour.is_none() && seven_day.is_none() {
        return None;
    }
    Some(UsageSnapshot {
        five_hour,
        seven_day,
        scoped: Vec::new(),
        spend: None,
        fetched_at: payload.recorded_at,
    })
}

/// Persist `snapshot`, replacing any previous reading. Atomic, so the
/// scheduler can read the file at any moment without seeing a half-written
/// document.
pub fn record(path: &Path, snapshot: &UsageSnapshot) -> io::Result<()> {
    let body = serde_json::to_string_pretty(&StatusLinePayload::from(snapshot))?;
    atomic_write(path, &body)
}

/// The recorded file's path for the current user, from `$HOME` — what both
/// target platforms define. The GUI resolves this through Tauri's path API
/// instead, but the bridge runs long before, and instead of, an `App`.
#[must_use]
pub fn default_path() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    (!home.as_os_str().is_empty()).then(|| claudemeter_path(&home, STATUSLINE_FILE))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    // Utilization is carried through byte-for-byte — no arithmetic touches
    // it — so exact equality is the assertion that pins the round trip.
    #![allow(clippy::float_cmp)]

    use super::*;
    use pretty_assertions::assert_eq;

    fn now() -> Timestamp {
        "2026-08-02T12:00:00Z".parse().unwrap()
    }

    fn limit(utilization: f64, reset_at: &str) -> ExportLimit {
        ExportLimit {
            utilization,
            reset_at: reset_at.parse().unwrap(),
        }
    }

    fn payload() -> StatusLinePayload {
        StatusLinePayload {
            schema: SCHEMA_VERSION,
            session_usage: Some(limit(37.4, "2026-08-02T15:00:00Z")),
            weekly_usage: Some(limit(61.2, "2026-08-05T09:00:00Z")),
            recorded_at: now(),
        }
    }

    fn write(dir: &tempfile::TempDir, payload: &StatusLinePayload) -> PathBuf {
        let path = claudemeter_path(dir.path(), STATUSLINE_FILE);
        atomic_write(&path, &serde_json::to_string(payload).unwrap()).unwrap();
        path
    }

    /// Golden-file test: pins the on-disk contract both halves are written
    /// against. Any change here is a breaking change.
    #[test]
    fn golden_recorded_schema() {
        assert_eq!(
            serde_json::to_value(payload()).unwrap(),
            serde_json::json!({
                "schema": 1,
                "session_usage": { "utilization": 37.4, "reset_at": "2026-08-02T15:00:00Z" },
                "weekly_usage": { "utilization": 61.2, "reset_at": "2026-08-05T09:00:00Z" },
                "recorded_at": "2026-08-02T12:00:00Z",
            })
        );
    }

    #[test]
    fn a_recorded_snapshot_reads_back_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = claudemeter_path(dir.path(), STATUSLINE_FILE);
        let written = read(&write(&dir, &payload())).unwrap();
        record(&path, &written).unwrap();
        assert_eq!(read(&path).unwrap(), written);
    }

    /// The reader must restore the window lengths the narrower on-disk shape
    /// drops, or pacing and reset copy would treat a weekly limit as a
    /// 5-hour one.
    #[test]
    fn reading_restores_the_window_lengths_the_file_does_not_carry() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = read(&write(&dir, &payload())).unwrap();
        assert_eq!(snapshot.five_hour.unwrap().window, LimitWindow::FiveHour);
        assert_eq!(snapshot.seven_day.unwrap().window, LimitWindow::SevenDay);
    }

    /// `recorded_at` becomes `fetched_at` so the scheduler's existing
    /// staleness rule ages the reading without knowing where it came from.
    #[test]
    fn the_recorded_time_becomes_the_snapshots_fetch_time() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = read(&write(&dir, &payload())).unwrap();
        assert_eq!(snapshot.fetched_at, now());
    }

    #[test]
    fn a_single_window_still_reads() {
        let dir = tempfile::tempdir().unwrap();
        let mut only_weekly = payload();
        only_weekly.session_usage = None;
        let snapshot = read(&write(&dir, &only_weekly)).unwrap();
        assert_eq!(snapshot.five_hour, None);
        assert_eq!(snapshot.seven_day.unwrap().utilization, 61.2);
    }

    #[test]
    fn a_file_with_no_windows_at_all_reads_as_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut empty = payload();
        empty.session_usage = None;
        empty.weekly_usage = None;
        assert_eq!(read(&write(&dir, &empty)), None);
    }

    /// A file from a build that knows something this one does not is refused
    /// rather than guessed at.
    #[test]
    fn a_newer_schema_is_refused_but_an_older_one_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let mut newer = payload();
        newer.schema = SCHEMA_VERSION + 1;
        assert_eq!(read(&write(&dir, &newer)), None);
        let mut older = payload();
        older.schema = 0;
        assert!(read(&write(&dir, &older)).is_some());
    }

    #[test]
    fn an_absent_or_corrupt_file_reads_as_nothing_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(&claudemeter_path(dir.path(), STATUSLINE_FILE)), None);
        let path = dir.path().join("corrupt.json");
        atomic_write(&path, "{ truncated").unwrap();
        assert_eq!(read(&path), None);
    }
}
