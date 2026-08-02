//! `~/.claudemeter/statusline-config.json` — the settings the bridge needs
//! but cannot reach.
//!
//! The bridge is a short-lived process with no Tauri `App`, so it cannot ask
//! for the app data directory, and guessing it would mean reimplementing
//! Tauri's per-platform `app_data_dir` — a dependency's behaviour duplicated
//! in our code, which breaks silently if it ever changes. Instead the app
//! mirrors the handful of values that matter into `~/.claudemeter/`, beside
//! the files the bridge already uses.
//!
//! **Only what changes the numbers belongs here.** Whether the status line
//! *shows* pace is the `--pace` flag's business: the status line is its own
//! surface, and the tray sets the precedent that one surface's display
//! preference must not silently gate another's (see `tray::PaceOptions`).
//! [`StatuslineConfig::pace_tracking_enabled`] is different — it is the
//! feature's master switch, and "off" there means pacing does not exist
//! anywhere, including here.
//!
//! Written by [`crate::settings::SettingsState::update`], so it tracks every
//! settings change rather than only what was true at launch, and read fresh on
//! every render. Every failure falls back to [`StatuslineConfig::default`]: a
//! missing or damaged mirror must cost a slightly-wrong pace basis, never the
//! status line itself.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::io_util::{atomic_write, read_json};

/// File name inside `~/.claudemeter/`.
pub const CONFIG_FILE: &str = "statusline-config.json";
/// Bumped when this shape changes incompatibly; a higher number is refused
/// rather than guessed at, exactly like the recorded reading's schema.
pub const SCHEMA_VERSION: u32 = 1;

/// The default weekly pacing basis, matching `AppSettings::weekly_pace_days`.
const DEFAULT_WEEKLY_PACE_DAYS: u8 = 7;
/// The range `AppSettings::normalize` clamps to, re-applied on read so a
/// hand-edited mirror cannot feed an out-of-range span into the pace maths.
const WEEKLY_PACE_DAYS: std::ops::RangeInclusive<u8> = 5..=7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatuslineConfig {
    pub schema: u32,
    /// How many days the weekly quota is expected to be spread over. Not a
    /// preference as far as the bridge is concerned — it is the denominator
    /// of the weekly pace ratio, so a wrong value here is a wrong number on
    /// screen rather than a differently-styled one.
    pub weekly_pace_days: u8,
    /// The pace feature's master switch. Off means the status line shows no
    /// pace even with `--pace`, matching what the tray and popover do.
    pub pace_tracking_enabled: bool,
}

impl Default for StatuslineConfig {
    fn default() -> Self {
        Self {
            schema: SCHEMA_VERSION,
            weekly_pace_days: DEFAULT_WEEKLY_PACE_DAYS,
            pace_tracking_enabled: true,
        }
    }
}

impl StatuslineConfig {
    /// Build from the two settings that matter, taken as plain values rather
    /// than an `AppSettings` so `settings.rs` can write this mirror without
    /// this module having to depend on it back.
    #[must_use]
    pub const fn new(weekly_pace_days: u8, pace_tracking_enabled: bool) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            weekly_pace_days,
            pace_tracking_enabled,
        }
    }
}

/// Read the mirror, falling back to defaults for every failure — absent,
/// unreadable, corrupt, or written by a newer build.
#[must_use]
pub fn read(path: &Path) -> StatuslineConfig {
    let parsed =
        read_json::<StatuslineConfig>(path).filter(|config| config.schema <= SCHEMA_VERSION);
    parsed.map_or_else(StatuslineConfig::default, |config| StatuslineConfig {
        weekly_pace_days: config
            .weekly_pace_days
            .clamp(*WEEKLY_PACE_DAYS.start(), *WEEKLY_PACE_DAYS.end()),
        ..config
    })
}

/// Persist the mirror, replacing any previous one.
pub fn write(path: &Path, config: StatuslineConfig) -> io::Result<()> {
    atomic_write(path, &serde_json::to_string_pretty(&config)?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::export::claudemeter_path;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn written(dir: &tempfile::TempDir, body: &str) -> PathBuf {
        let path = claudemeter_path(dir.path(), CONFIG_FILE);
        atomic_write(&path, body).unwrap();
        path
    }

    #[test]
    fn the_mirror_sits_beside_the_other_claudemeter_files() {
        assert_eq!(
            claudemeter_path(Path::new("/home/example"), CONFIG_FILE),
            PathBuf::from("/home/example/.claudemeter/statusline-config.json")
        );
    }

    /// The default has to match `AppSettings`, or a status line set up before
    /// the app ever wrote the mirror would pace against a different week.
    #[test]
    fn the_default_matches_the_apps_own_defaults() {
        let config = StatuslineConfig::default();
        assert_eq!(config.weekly_pace_days, 7);
        assert!(config.pace_tracking_enabled);
    }

    #[test]
    fn a_written_config_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = claudemeter_path(dir.path(), CONFIG_FILE);
        write(&path, StatuslineConfig::new(5, false)).unwrap();
        let read_back = read(&path);
        assert_eq!(read_back.weekly_pace_days, 5);
        assert!(!read_back.pace_tracking_enabled);
    }

    /// A missing mirror is the ordinary state before the app has run once,
    /// and must leave the status line working rather than paceless-and-silent
    /// or, worse, broken.
    #[test]
    fn an_absent_mirror_reads_as_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            read(&claudemeter_path(dir.path(), CONFIG_FILE)),
            StatuslineConfig::default()
        );
    }

    #[test]
    fn a_corrupt_or_newer_mirror_also_reads_as_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            read(&written(&dir, "{ truncated")),
            StatuslineConfig::default()
        );
        let newer = serde_json::json!({
            "schema": SCHEMA_VERSION + 1,
            "weekly_pace_days": 5,
            "pace_tracking_enabled": false,
        });
        assert_eq!(
            read(&written(&dir, &newer.to_string())),
            StatuslineConfig::default()
        );
    }

    /// `AppSettings::normalize` clamps this on the app's side, but the mirror
    /// is a plain file a user may edit — and an out-of-range span would make
    /// the weekly ratio quietly wrong rather than obviously broken.
    #[test]
    fn an_out_of_range_pace_basis_is_clamped_on_read() {
        let dir = tempfile::tempdir().unwrap();
        for (written_days, expected) in [(0_u8, 5_u8), (1, 5), (5, 5), (7, 7), (30, 7)] {
            let body = serde_json::json!({
                "schema": 1,
                "weekly_pace_days": written_days,
                "pace_tracking_enabled": true,
            });
            assert_eq!(
                read(&written(&dir, &body.to_string())).weekly_pace_days,
                expected,
                "{written_days} days"
            );
        }
    }
}
