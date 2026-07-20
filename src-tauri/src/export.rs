//! `~/.claudemeter/usage.json` export for external consumers (issue #8).
//!
//! Written after every successful fetch so statusline scripts and other
//! external tools built against the Swift `ClaudeMeter`'s export keep working
//! unmodified when a user switches to this app. The path is shared with the
//! Swift app *intentionally* (see the README's coexistence note) — whichever
//! app fetched most recently wins, there is no locking or merging.
//!
//! The schema mirrors `ClaudeMeter`'s `UsageExportPayload`
//! (eddmann/ClaudeMeter#32): `session_usage`/`weekly_usage` are the headline
//! windows, `scoped_usage` is the general per-model form, and `sonnet_usage`
//! is kept as a deprecated alias — the first scoped entry whose display name
//! is "Sonnet" (case-insensitive, matching the Swift app) — for scripts
//! written against the older Sonnet-only shape.
//!
//! **Deliberate deviation from the Swift schema:** in `UsageExportPayload.swift`
//! (PR #32) `session_usage`/`weekly_usage` are non-optional — `toDomain()`
//! throws (skipping the write entirely) if a headline window is entirely
//! absent, and synthesizes a fallback `resets_at` if only that field is
//! missing. This app's domain model (`UsageSnapshot::five_hour`/`seven_day`,
//! predates issue #8) already collapses "missing" and "present but no
//! `resets_at`" into a single `None` with no fallback data to synthesize
//! from, so by the time `write()` sees the snapshot the distinction Swift's
//! fallback relies on is gone. Rather than invent a fallback reset time here,
//! this export emits `session_usage`/`weekly_usage` as JSON `null` in that
//! case — a schema difference consumers coming from the Swift app's
//! non-optional guarantee should be aware of; see
//! `missing_headline_windows_export_as_null_not_a_skipped_write` below.
//!
//! Write failures are logged (`eprintln!`) but never propagate: a refresh
//! that fetched real data successfully must not be treated as failed just
//! because the optional external export couldn't be written.

use std::io;
use std::path::Path;

use jiff::Timestamp;
use meter_core::{UsageSnapshot, UsageWindow};
use serde::Serialize;

use crate::io_util::atomic_write;

/// Directory name inside the user's home directory.
pub const EXPORT_DIR: &str = ".claudemeter";
/// File name inside [`EXPORT_DIR`].
pub const EXPORT_FILE: &str = "usage.json";

/// One usage limit in the exported shape: just utilization and reset time,
/// none of this app's internal `LimitWindow`/status types — the export
/// contract is deliberately narrower than the domain model so it can stay
/// stable while the domain model evolves.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportLimit {
    pub utilization: f64,
    pub reset_at: Timestamp,
}

impl From<&UsageWindow> for ExportLimit {
    fn from(window: &UsageWindow) -> Self {
        Self {
            utilization: window.utilization,
            reset_at: window.resets_at,
        }
    }
}

/// One model-scoped limit in the exported shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportScopedLimit {
    pub name: String,
    pub limit: ExportLimit,
    pub is_active: bool,
}

/// The public `~/.claudemeter/usage.json` contract. Kept separate from
/// [`UsageSnapshot`] so the domain model and this on-disk contract can change
/// shape independently — see the module docs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UsageExportPayload {
    /// The 5-hour headline window. `None` on the rare snapshot where the API
    /// omitted it (e.g. a missing `resets_at`) — still written as `null`
    /// rather than skipping the export entirely, so consumers see a complete
    /// but partial document instead of stale data. Nullable here, unlike the
    /// Swift app's non-optional field of the same name — see the module
    /// docs' "Deliberate deviation" note.
    pub session_usage: Option<ExportLimit>,
    /// The 7-day headline window; see `session_usage` for the `None` case
    /// and the deviation from the Swift schema.
    pub weekly_usage: Option<ExportLimit>,
    /// Every model-scoped limit, in snapshot order.
    pub scoped_usage: Vec<ExportScopedLimit>,
    /// Deprecated alias for the scoped limit named "Sonnet" (matched
    /// case-insensitively), kept so existing statusline scripts written
    /// against the older Sonnet-only export keep working.
    pub sonnet_usage: Option<ExportLimit>,
    pub last_updated: Timestamp,
}

impl From<&UsageSnapshot> for UsageExportPayload {
    fn from(snapshot: &UsageSnapshot) -> Self {
        let scoped_usage: Vec<ExportScopedLimit> = snapshot
            .scoped
            .iter()
            .map(|limit| ExportScopedLimit {
                name: limit.display_name.clone(),
                limit: ExportLimit::from(&limit.usage),
                is_active: limit.is_active,
            })
            .collect();
        let sonnet_usage = scoped_usage
            .iter()
            .find(|scoped| scoped.name.eq_ignore_ascii_case("sonnet"))
            .map(|scoped| scoped.limit.clone());
        Self {
            session_usage: snapshot.five_hour.as_ref().map(ExportLimit::from),
            weekly_usage: snapshot.seven_day.as_ref().map(ExportLimit::from),
            scoped_usage,
            sonnet_usage,
            last_updated: snapshot.fetched_at,
        }
    }
}

/// The full export path (`~/.claudemeter/usage.json`) given the user's home
/// directory.
pub fn export_path(home: &Path) -> std::path::PathBuf {
    home.join(EXPORT_DIR).join(EXPORT_FILE)
}

/// Persist `snapshot` as the public export, replacing any previous file.
/// Writes to a sibling temp file and renames so a crash mid-write (or a
/// concurrent read by an external script) can never observe a truncated
/// file, and so the Swift app's own atomic writer never races a partial read
/// from this one either.
pub fn write(path: &Path, snapshot: &UsageSnapshot) -> io::Result<()> {
    let payload = UsageExportPayload::from(snapshot);
    let body = serde_json::to_string_pretty(&payload)?;
    atomic_write(path, &body)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use meter_core::{LimitWindow, ScopedLimit};
    use pretty_assertions::assert_eq;
    use std::fs;
    use std::path::PathBuf;

    fn window(utilization: f64, resets_at: &str, kind: LimitWindow) -> UsageWindow {
        UsageWindow {
            utilization,
            resets_at: resets_at.parse().unwrap(),
            window: kind,
        }
    }

    fn snapshot() -> UsageSnapshot {
        UsageSnapshot {
            five_hour: Some(window(42.5, "2026-07-17T15:00:00Z", LimitWindow::FiveHour)),
            seven_day: Some(window(60.0, "2026-07-20T00:00:00Z", LimitWindow::SevenDay)),
            scoped: vec![
                ScopedLimit {
                    display_name: "Fable".to_owned(),
                    model_id: None,
                    usage: window(12.0, "2026-07-20T00:00:00Z", LimitWindow::SevenDay),
                    is_active: true,
                },
                ScopedLimit {
                    display_name: "Sonnet".to_owned(),
                    model_id: None,
                    usage: window(30.0, "2026-07-20T00:00:00Z", LimitWindow::SevenDay),
                    is_active: false,
                },
            ],
            spend: None,
            fetched_at: "2026-07-17T12:00:00Z".parse().unwrap(),
        }
    }

    fn export_file(dir: &tempfile::TempDir) -> PathBuf {
        export_path(dir.path())
    }

    /// Golden-file test: pins the exact shape/field names of the schema.
    /// Any change here is a breaking change to external statusline scripts
    /// and must be deliberate.
    #[test]
    fn golden_export_schema() {
        let payload = UsageExportPayload::from(&snapshot());
        let json = serde_json::to_value(&payload).unwrap();
        let expected = serde_json::json!({
            "session_usage": {
                "utilization": 42.5,
                "reset_at": "2026-07-17T15:00:00Z",
            },
            "weekly_usage": {
                "utilization": 60.0,
                "reset_at": "2026-07-20T00:00:00Z",
            },
            "scoped_usage": [
                {
                    "name": "Fable",
                    "limit": { "utilization": 12.0, "reset_at": "2026-07-20T00:00:00Z" },
                    "is_active": true,
                },
                {
                    "name": "Sonnet",
                    "limit": { "utilization": 30.0, "reset_at": "2026-07-20T00:00:00Z" },
                    "is_active": false,
                },
            ],
            "sonnet_usage": { "utilization": 30.0, "reset_at": "2026-07-20T00:00:00Z" },
            "last_updated": "2026-07-17T12:00:00Z",
        });
        assert_eq!(json, expected);
    }

    #[test]
    fn sonnet_usage_compat_field_mirrors_the_scoped_sonnet_entry() {
        let payload = UsageExportPayload::from(&snapshot());
        assert_eq!(
            payload.sonnet_usage,
            Some(ExportLimit {
                utilization: 30.0,
                reset_at: "2026-07-20T00:00:00Z".parse().unwrap(),
            })
        );
    }

    #[test]
    fn sonnet_usage_is_matched_case_insensitively() {
        let mut snap = snapshot();
        snap.scoped[1].display_name = "SONNET".to_owned();
        let payload = UsageExportPayload::from(&snap);
        assert!(payload.sonnet_usage.is_some());
    }

    #[test]
    fn sonnet_usage_is_none_without_a_sonnet_scoped_limit() {
        let mut snap = snapshot();
        snap.scoped.retain(|s| s.display_name != "Sonnet");
        let payload = UsageExportPayload::from(&snap);
        assert_eq!(payload.sonnet_usage, None);
    }

    #[test]
    fn missing_headline_windows_export_as_null_not_a_skipped_write() {
        let snap = UsageSnapshot {
            five_hour: None,
            seven_day: None,
            scoped: vec![],
            spend: None,
            fetched_at: "2026-07-17T12:00:00Z".parse().unwrap(),
        };
        let payload = UsageExportPayload::from(&snap);
        assert_eq!(payload.session_usage, None);
        assert_eq!(payload.weekly_usage, None);
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["session_usage"], serde_json::Value::Null);
        assert_eq!(json["weekly_usage"], serde_json::Value::Null);
    }

    #[test]
    fn write_round_trips_and_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join("nested/home")
            .join(EXPORT_DIR)
            .join(EXPORT_FILE);
        write(&path, &snapshot()).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        let decoded: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(decoded["scoped_usage"][0]["name"], "Fable");
        assert_eq!(decoded["last_updated"], "2026-07-17T12:00:00Z");
    }

    #[test]
    fn write_never_leaves_a_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = export_file(&dir);
        write(&path, &snapshot()).unwrap();
        assert!(path.exists());
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn write_replaces_a_previous_export_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = export_file(&dir);
        write(&path, &snapshot()).unwrap();
        let mut newer = snapshot();
        newer.fetched_at = "2026-07-17T13:00:00Z".parse().unwrap();
        write(&path, &newer).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        let decoded: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(decoded["last_updated"], "2026-07-17T13:00:00Z");
    }

    #[test]
    fn export_path_joins_home_dir_dot_dir_and_file() {
        let home = PathBuf::from("/home/example");
        assert_eq!(
            export_path(&home),
            PathBuf::from("/home/example/.claudemeter/usage.json")
        );
    }
}
