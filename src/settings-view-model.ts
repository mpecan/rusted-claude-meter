// Pure view-model helpers for the Settings panel: no DOM, no Tauri, fully
// unit-testable, mirroring the split `src-tauri/src/tray/model.rs` and
// `src-tauri/src/settings.rs` use.

import type { AppSettings, UsageSnapshot } from "./types";

/** The settings a fresh install starts from, before `getSettings()`
 * resolves. Mirrors `settings::AppSettings::default()` on the Rust side,
 * except `monochrome` — the real default is platform-dependent there
 * (macOS: true, Linux: false) and gets overwritten by the first
 * `getSettings()` response either way, so a neutral placeholder is fine
 * here. */
export const DEFAULT_SETTINGS: AppSettings = {
  shown_scoped_models: [],
  refresh_interval: "one_minute",
  warning_threshold: 75,
  critical_threshold: 90,
  notify_on_reset: false,
  icon_style: "battery",
  monochrome: false,
  show_reset_time: true,
  popover_layout: "rows",
  weekly_pace_days: 7,
  pace_first_display: false,
};

/** Deduped, snapshot-order list of every scoped model's display name in the
 * latest snapshot — the source for Settings' one-toggle-per-model list. A
 * model reported for the first time appears here immediately, before the
 * user has switched it on (`shown_scoped_models` is opt-in and empty by
 * default). */
export function scopedModelNames(snapshot: UsageSnapshot | null): string[] {
  if (!snapshot) {
    return [];
  }
  const seen = new Set<string>();
  const names: string[] = [];
  for (const limit of snapshot.scoped) {
    if (!seen.has(limit.display_name)) {
      seen.add(limit.display_name);
      names.push(limit.display_name);
    }
  }
  return names;
}

/** Add or remove `name` from a `shown_scoped_models` list, preserving order
 * and never duplicating an entry. Pure: the caller persists the result via
 * `setShownScopedModels`. */
export function toggleModel(shown: readonly string[], name: string, enabled: boolean): string[] {
  const has = shown.includes(name);
  if (enabled && !has) {
    return [...shown, name];
  }
  if (!enabled && has) {
    return shown.filter((candidate) => candidate !== name);
  }
  return [...shown];
}
