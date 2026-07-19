//! Typed, disk-persisted application settings (issue #6).
//!
//! Mirrors `cache.rs`'s decode-safety discipline: `settings.json` lives in
//! the app data dir behind a version envelope, and loading never errors — a
//! missing, corrupt, foreign-shaped or future-versioned file yields
//! [`AppSettings::default`] instead. On top of that, `#[serde(default)]` on
//! the struct itself means an *older* saved file (missing fields a newer
//! build added) still decodes field-by-field instead of being rejected
//! wholesale, and an unrecognised field left behind by a newer build is
//! silently ignored (no `deny_unknown_fields`). Together these two layers
//! are what "old saved settings must decode safely" means in the issue.
//!
//! `shown_scoped_models` is opt-in and empty by default: a model reported in
//! a snapshot for the first time appears in Settings but stays out of the
//! popover/tray menu until the user switches it on (see
//! `tray::model::menu_model` and `src/view-model.ts`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use meter_render::IconStyle;
use serde::{Deserialize, Serialize};

use crate::io_util::atomic_write;
use crate::scheduler::RefreshInterval;

/// File name inside the app data dir.
pub const SETTINGS_FILE: &str = "settings.json";

/// Bumped whenever the persisted shape changes incompatibly; readers treat
/// any other version as absent (falling back to defaults) instead of
/// guessing. Field additions/removals do *not* need a bump — `#[serde(default)]`
/// already handles those; this is only for a true breaking rewrite.
const SETTINGS_VERSION: u32 = 1;

/// The default for [`AppSettings::monochrome`]: matches the tray's previous
/// hardcoded behaviour (`tray::mod::MONO`) so a fresh install renders
/// exactly as before until the user opts out. macOS menu-bar icons should be
/// templates so the system recolours them for light/dark appearance; Linux
/// trays have no template concept, so colour carries state there.
const fn default_monochrome() -> bool {
    cfg!(target_os = "macos")
}

/// Every user-configurable setting, persisted as one JSON document.
///
/// Every field has a plain-data default (see [`Default`]), and every field
/// decodes independently — see the module docs for what that buys forward-
/// and backward-compatibility-wise.
/// How the popover lays out its usage meters (redesign directions 1a/1c). The
/// frontend switches renderers on this; the shell only persists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PopoverLayout {
    /// Compact hairline-split meter rows in one panel (design 1a).
    #[default]
    Rows,
    /// Roomier tinted status cards with a status pill (design 1c).
    Cards,
}

// Four independent on/off toggles (`notify_on_reset`, `monochrome`,
// `show_reset_time`, `pace_first_display`), each persisted and round-tripped
// on its own — none combine into a state machine, so splitting them into
// two-variant enums per clippy's suggestion would only add ceremony without
// removing any invalid state.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Display names of scoped limits the popover/tray are allowed to show.
    /// Opt-in: empty by default, keyed on `ScopedLimit::display_name` (the
    /// API's `model_id` is currently always null).
    pub shown_scoped_models: Vec<String>,
    pub refresh_interval: RefreshInterval,
    /// Utilization percentage (0-100) at which a notification is considered
    /// a warning (issue #7 consumes this; #6 only persists it).
    pub warning_threshold: f64,
    /// Utilization percentage (0-100) at which a notification is considered
    /// critical.
    pub critical_threshold: f64,
    /// Whether a window resetting ("5-hour limit reset") fires its own
    /// notification (issue #7). Threshold-crossing notifications are always
    /// on; this only gates the extra, noisier reset notice. Off by default —
    /// the 5-hour window resets several times a day.
    pub notify_on_reset: bool,
    pub icon_style: IconStyle,
    pub monochrome: bool,
    /// Whether each popover card appends the exact reset wall-clock time next
    /// to the relative countdown ("resets in 2h 14m (11:30 PM)"). On by
    /// default, matching `ClaudeMeter` PR #26. `#[serde(default)]` on the
    /// struct fills this from `Default` for settings files written before the
    /// field existed, so an upgrade keeps the user's other choices.
    pub show_reset_time: bool,
    /// Which popover layout the frontend renders (redesign 1a/1c). Persisted
    /// here; back-compat via `#[serde(default)]` like the fields above.
    pub popover_layout: PopoverLayout,
    /// How many days of the week the weekly quota is expected to be paced
    /// over (issue #16's working-week option) — 5, 6 or 7. Clamped to
    /// `5..=7` by [`Self::normalize`] on every write and load, so a
    /// hand-edited or pre-issue-#16 settings file can never feed an
    /// out-of-range span into `UsageSnapshot::pace_signal`. Defaults to 7
    /// (the full week), matching upstream's `ClaudeMeter` default.
    pub weekly_pace_days: u8,
    /// Whether the tray/popover lead with the pace ratio instead of the raw
    /// quota percentage (upstream's `DisplayModePicker`). Off by default: a
    /// fresh install shows the same quota-first icon it always has, and the
    /// flame/snowflake badge only appears once the user opts in.
    pub pace_first_display: bool,
    /// Master switch for the whole pace-tracking feature (issue #16). When
    /// off, the app behaves as if pacing does not exist: the popover cards
    /// drop their projections / pace line / verdict badge, and the tray shows
    /// no pace ratio or flame/snowflake regardless of `pace_first_display`.
    /// The sub-settings (`weekly_pace_days`, `pace_first_display`) keep their
    /// stored values so re-enabling restores the prior configuration. On by
    /// default so the feature is visible; users can disable the section
    /// wholesale from Settings.
    pub pace_tracking_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            shown_scoped_models: Vec::new(),
            refresh_interval: RefreshInterval::default(),
            warning_threshold: 75.0,
            critical_threshold: 90.0,
            notify_on_reset: false,
            icon_style: IconStyle::Battery,
            monochrome: default_monochrome(),
            show_reset_time: true,
            popover_layout: PopoverLayout::Rows,
            weekly_pace_days: 7,
            pace_first_display: false,
            pace_tracking_enabled: true,
        }
    }
}

impl AppSettings {
    /// Clamp both thresholds to a sane `0..=100` range, then enforce
    /// `warning_threshold <= critical_threshold`. Called on every write so a
    /// stray out-of-range or inverted pair from the frontend (the Settings
    /// UI exposes them as two independent 0-100 sliders with no
    /// cross-constraint) or a future hand-edited settings file can never
    /// propagate into a notification comparison — `notify.rs`'s
    /// "Warning fires before/with Critical" guarantee depends on this
    /// ordering holding for every `NotificationThresholds` sourced from
    /// settings.
    fn normalize(&mut self) {
        self.warning_threshold = self.warning_threshold.clamp(0.0, 100.0);
        self.critical_threshold = self.critical_threshold.clamp(0.0, 100.0);
        if self.critical_threshold < self.warning_threshold {
            self.critical_threshold = self.warning_threshold;
        }
        self.weekly_pace_days = self.weekly_pace_days.clamp(5, 7);
    }
}

#[derive(Debug, Deserialize)]
struct DiskSettings {
    version: u32,
    #[serde(default)]
    settings: AppSettings,
}

#[derive(Debug, Serialize)]
struct DiskSettingsRef<'a> {
    version: u32,
    settings: &'a AppSettings,
}

/// Load the persisted settings, or [`AppSettings::default`] when there is
/// nothing usable (missing file, corrupt JSON, foreign shape, or a future
/// version this build doesn't understand). Never errors: settings are an
/// optimization over sane defaults, not a source of truth the app cannot
/// run without.
pub fn load(path: &Path) -> AppSettings {
    let mut settings = fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<DiskSettings>(&raw).ok())
        .filter(|decoded| decoded.version == SETTINGS_VERSION)
        .map_or_else(AppSettings::default, |decoded| decoded.settings);
    settings.normalize();
    settings
}

/// Persist `settings`, replacing any previous file. Writes to a sibling temp
/// file and renames so a crash mid-write cannot leave a truncated file
/// behind (`io_util::atomic_write`, same discipline as `cache::save`).
pub fn save(path: &Path, settings: &AppSettings) -> io::Result<()> {
    let body = serde_json::to_string(&DiskSettingsRef {
        version: SETTINGS_VERSION,
        settings,
    })?;
    atomic_write(path, &body)
}

/// Managed Tauri state: the in-memory settings plus where to persist them.
/// `path` is `None` when the app data dir couldn't be resolved (mirrors
/// `cache_path` in `lib.rs`) — settings still work for the running session,
/// they just don't survive a restart.
pub struct SettingsState {
    path: Option<PathBuf>,
    settings: Mutex<AppSettings>,
}

impl SettingsState {
    pub const fn new(path: Option<PathBuf>, settings: AppSettings) -> Self {
        Self {
            path,
            settings: Mutex::new(settings),
        }
    }

    /// The current settings.
    pub fn get(&self) -> AppSettings {
        self.lock().clone()
    }

    /// Apply `mutate` to the in-memory settings, normalize thresholds, then
    /// persist the result (best-effort: a disk write failure does not undo
    /// the in-memory change, mirroring the scheduler's cache-write
    /// discipline). Returns the settings as they now stand, so a command can
    /// hand the resolved/clamped value straight back to the frontend.
    pub fn update(&self, mutate: impl FnOnce(&mut AppSettings)) -> AppSettings {
        let mut guard = self.lock();
        mutate(&mut guard);
        guard.normalize();
        let snapshot = guard.clone();
        drop(guard);
        if let Some(path) = &self.path {
            let _ = save(path, &snapshot);
        }
        snapshot
    }

    fn lock(&self) -> MutexGuard<'_, AppSettings> {
        self.settings.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::float_cmp)]

    use super::*;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn settings_path(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join(SETTINGS_FILE)
    }

    fn sample() -> AppSettings {
        AppSettings {
            shown_scoped_models: vec!["Fable".to_owned(), "Sonnet".to_owned()],
            refresh_interval: RefreshInterval::FiveMinutes,
            warning_threshold: 60.0,
            critical_threshold: 85.0,
            notify_on_reset: true,
            icon_style: IconStyle::Gauge,
            monochrome: !default_monochrome(),
            show_reset_time: false,
            popover_layout: PopoverLayout::Cards,
            weekly_pace_days: 5,
            pace_first_display: true,
            pace_tracking_enabled: false,
        }
    }

    #[test]
    fn round_trips_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        save(&path, &sample()).unwrap();
        assert_eq!(load(&path), sample());
    }

    #[test]
    fn missing_file_loads_as_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(&settings_path(&dir)), AppSettings::default());
    }

    #[test]
    fn default_is_empty_and_opt_in_for_scoped_models() {
        assert!(AppSettings::default().shown_scoped_models.is_empty());
    }

    #[test]
    fn corrupt_json_loads_as_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(load(&path), AppSettings::default());
    }

    #[test]
    fn foreign_json_shape_loads_as_default() {
        // A bare settings object with no version envelope (e.g. a manually
        // crafted or very old file) must decode safely to defaults.
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        fs::write(&path, serde_json::to_string(&sample()).unwrap()).unwrap();
        assert_eq!(load(&path), AppSettings::default());
    }

    #[test]
    fn future_settings_version_loads_as_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        save(&path, &sample()).unwrap();
        let bumped = fs::read_to_string(&path)
            .unwrap()
            .replace("\"version\":1", "\"version\":999");
        fs::write(&path, bumped).unwrap();
        assert_eq!(load(&path), AppSettings::default());
    }

    #[test]
    fn missing_fields_are_defaulted_field_by_field() {
        // An "old" save that only ever wrote `shown_scoped_models` (as if a
        // future build removes/adds fields) must still decode: everything
        // else falls back to `AppSettings::default()`, not a hard error.
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        fs::write(
            &path,
            r#"{"version":1,"settings":{"shown_scoped_models":["Fable"]}}"#,
        )
        .unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.shown_scoped_models, vec!["Fable".to_owned()]);
        assert_eq!(loaded.refresh_interval, RefreshInterval::default());
        assert_eq!(loaded.warning_threshold, 75.0);
        assert_eq!(loaded.critical_threshold, 90.0);
        assert!(!loaded.notify_on_reset);
        assert_eq!(loaded.icon_style, IconStyle::Battery);
        assert_eq!(loaded.monochrome, default_monochrome());
        assert_eq!(loaded.weekly_pace_days, 7);
        assert!(!loaded.pace_first_display);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        // A field a newer build introduced (or a stray typo) must not break
        // decoding for an older build reading the same file.
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        fs::write(
            &path,
            r#"{"version":1,"settings":{"shown_scoped_models":[],"totally_new_field":42}}"#,
        )
        .unwrap();
        assert_eq!(load(&path), AppSettings::default());
    }

    #[test]
    fn empty_settings_object_defaults_every_field() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        fs::write(&path, r#"{"version":1,"settings":{}}"#).unwrap();
        assert_eq!(load(&path), AppSettings::default());
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/app-data").join(SETTINGS_FILE);
        save(&path, &sample()).unwrap();
        assert_eq!(load(&path), sample());
    }

    #[test]
    fn save_replaces_a_previous_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        save(&path, &sample()).unwrap();
        let mut newer = sample();
        newer.icon_style = IconStyle::Minimal;
        save(&path, &newer).unwrap();
        assert_eq!(load(&path), newer);
    }

    #[test]
    fn state_update_persists_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        let state = SettingsState::new(Some(path.clone()), AppSettings::default());

        let result = state.update(|s| s.shown_scoped_models.push("Fable".to_owned()));
        assert_eq!(result.shown_scoped_models, vec!["Fable".to_owned()]);
        assert_eq!(state.get().shown_scoped_models, vec!["Fable".to_owned()]);
        assert_eq!(load(&path).shown_scoped_models, vec!["Fable".to_owned()]);
    }

    #[test]
    fn load_clamps_out_of_range_thresholds_from_disk() {
        // A hand-edited (or future buggy) settings.json can carry an
        // out-of-range threshold; `load()` must clamp it just like
        // `SettingsState::update` does, so the guarantee `normalize`'s
        // docstring makes actually holds on the load path too.
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        fs::write(
            &path,
            r#"{"version":1,"settings":{"warning_threshold":-10.0,"critical_threshold":250.0}}"#,
        )
        .unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.warning_threshold, 0.0);
        assert_eq!(loaded.critical_threshold, 100.0);
    }

    #[test]
    fn state_update_clamps_thresholds() {
        let state = SettingsState::new(None, AppSettings::default());
        let result = state.update(|s| {
            s.warning_threshold = -10.0;
            s.critical_threshold = 250.0;
        });
        assert_eq!(result.warning_threshold, 0.0);
        assert_eq!(result.critical_threshold, 100.0);
    }

    #[test]
    fn normalize_raises_critical_to_meet_an_inverted_warning_threshold() {
        // Settings UI exposes warning/critical as two independent sliders
        // with no cross-constraint; normalize() must still guarantee
        // warning <= critical afterwards so notify.rs's severity-ordering
        // contract holds for every threshold pair sourced from settings.
        let state = SettingsState::new(None, AppSettings::default());
        let result = state.update(|s| {
            s.warning_threshold = 95.0;
            s.critical_threshold = 50.0;
        });
        assert_eq!(result.warning_threshold, 95.0);
        assert_eq!(result.critical_threshold, 95.0);
        assert!(result.warning_threshold <= result.critical_threshold);
    }

    #[test]
    fn load_raises_an_inverted_pair_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        fs::write(
            &path,
            r#"{"version":1,"settings":{"warning_threshold":90.0,"critical_threshold":50.0}}"#,
        )
        .unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.warning_threshold, 90.0);
        assert_eq!(loaded.critical_threshold, 90.0);
    }

    #[test]
    fn state_without_a_path_still_updates_in_memory() {
        let state = SettingsState::new(None, AppSettings::default());
        let result = state.update(|s| s.monochrome = !s.monochrome);
        assert_eq!(state.get().monochrome, result.monochrome);
    }

    #[test]
    fn notify_on_reset_defaults_to_off_and_round_trips() {
        assert!(!AppSettings::default().notify_on_reset);
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        let mut settings = sample();
        settings.notify_on_reset = true;
        save(&path, &settings).unwrap();
        assert!(load(&path).notify_on_reset);
    }

    #[test]
    fn weekly_pace_days_defaults_to_the_full_week_and_pace_first_display_defaults_off() {
        let default = AppSettings::default();
        assert_eq!(default.weekly_pace_days, 7);
        assert!(!default.pace_first_display);
        // Pace tracking is on by default (the feature is visible; disable-able).
        assert!(default.pace_tracking_enabled);
    }

    #[test]
    fn weekly_pace_days_and_pace_first_display_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        save(&path, &sample()).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.weekly_pace_days, 5);
        assert!(loaded.pace_first_display);
    }

    #[test]
    fn normalize_clamps_weekly_pace_days_to_five_through_seven() {
        // A hand-edited (or future buggy) settings file could carry a span
        // outside the 5/6/7-day working-week option; `load` and
        // `SettingsState::update` must both clamp it, mirroring the
        // threshold-clamping guarantee above, so `UsageSnapshot::pace_signal`
        // never sees an out-of-range pacing span.
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        fs::write(&path, r#"{"version":1,"settings":{"weekly_pace_days":2}}"#).unwrap();
        assert_eq!(load(&path).weekly_pace_days, 5);

        fs::write(&path, r#"{"version":1,"settings":{"weekly_pace_days":9}}"#).unwrap();
        assert_eq!(load(&path).weekly_pace_days, 7);

        let state = SettingsState::new(None, AppSettings::default());
        let result = state.update(|s| s.weekly_pace_days = 1);
        assert_eq!(result.weekly_pace_days, 5);
        let result = state.update(|s| s.weekly_pace_days = 200);
        assert_eq!(result.weekly_pace_days, 7);
    }
}
