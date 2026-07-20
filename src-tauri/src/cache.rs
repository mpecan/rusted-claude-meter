//! Disk cache for the last good usage snapshot.
//!
//! `usage_cache.json` lives in the app data dir so a restart renders the
//! tray instantly from the previous run's data while the first fetch is in
//! flight. Loading is decode-safe by construction: a missing, corrupt,
//! foreign or older-format file yields `None`, never an error — the cache
//! is an optimization, not a source of truth.

use std::fs;
use std::io;
use std::path::Path;

use meter_core::UsageSnapshot;
use serde::{Deserialize, Serialize};

use crate::io_util::atomic_write;

/// File name inside the app data dir.
pub const CACHE_FILE: &str = "usage_cache.json";

/// Bumped whenever the persisted shape changes incompatibly; readers treat
/// any other version as absent instead of guessing.
const CACHE_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
struct DiskCache {
    version: u32,
    snapshot: UsageSnapshot,
}

#[derive(Debug, Serialize)]
struct DiskCacheRef<'a> {
    version: u32,
    snapshot: &'a UsageSnapshot,
}

/// Load the cached snapshot, or `None` when there is nothing usable.
pub fn load(path: &Path) -> Option<UsageSnapshot> {
    let raw = fs::read_to_string(path).ok()?;
    let decoded: DiskCache = serde_json::from_str(&raw).ok()?;
    (decoded.version == CACHE_VERSION).then_some(decoded.snapshot)
}

/// Persist `snapshot`, replacing any previous cache. Writes to a sibling
/// temp file and renames so a crash mid-write cannot leave a truncated
/// cache behind (see `io_util::atomic_write`).
pub fn save(path: &Path, snapshot: &UsageSnapshot) -> io::Result<()> {
    let body = serde_json::to_string(&DiskCacheRef {
        version: CACHE_VERSION,
        snapshot,
    })?;
    atomic_write(path, &body)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use jiff::Timestamp;
    use meter_core::{LimitWindow, ScopedLimit, UsageWindow};
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn snapshot() -> UsageSnapshot {
        UsageSnapshot {
            five_hour: Some(UsageWindow {
                utilization: 42.5,
                resets_at: "2026-07-17T15:00:00Z".parse().unwrap(),
                window: LimitWindow::FiveHour,
            }),
            seven_day: None,
            scoped: vec![ScopedLimit {
                display_name: "Fable".to_owned(),
                model_id: None,
                usage: UsageWindow {
                    utilization: 12.0,
                    resets_at: "2026-07-20T00:00:00Z".parse().unwrap(),
                    window: LimitWindow::SevenDay,
                },
                is_active: true,
            }],
            spend: None,
            fetched_at: "2026-07-17T12:00:00Z".parse::<Timestamp>().unwrap(),
        }
    }

    fn cache_path(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join(CACHE_FILE)
    }

    #[test]
    fn round_trips_a_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = cache_path(&dir);
        save(&path, &snapshot()).unwrap();
        assert_eq!(load(&path), Some(snapshot()));
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/app-data").join(CACHE_FILE);
        save(&path, &snapshot()).unwrap();
        assert_eq!(load(&path), Some(snapshot()));
    }

    #[test]
    fn save_replaces_a_previous_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = cache_path(&dir);
        save(&path, &snapshot()).unwrap();
        let mut newer = snapshot();
        newer.fetched_at = "2026-07-17T13:00:00Z".parse().unwrap();
        save(&path, &newer).unwrap();
        assert_eq!(load(&path), Some(newer));
    }

    #[test]
    fn missing_file_loads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(&cache_path(&dir)), None);
    }

    #[test]
    fn corrupt_json_loads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = cache_path(&dir);
        fs::write(&path, "{ this is not json").unwrap();
        assert_eq!(load(&path), None);
    }

    #[test]
    fn foreign_json_shape_loads_as_none() {
        // An old cache format (bare snapshot, no version envelope) must
        // decode safely to "no cache", never crash or error.
        let dir = tempfile::tempdir().unwrap();
        let path = cache_path(&dir);
        fs::write(&path, serde_json::to_string(&snapshot()).unwrap()).unwrap();
        assert_eq!(load(&path), None);
    }

    #[test]
    fn future_cache_version_loads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = cache_path(&dir);
        save(&path, &snapshot()).unwrap();
        let bumped = fs::read_to_string(&path)
            .unwrap()
            .replace("\"version\":1", "\"version\":999");
        fs::write(&path, bumped).unwrap();
        assert_eq!(load(&path), None);
    }
}
