// Pure view-model helpers for the Settings panel: no DOM, no Tauri, fully
// unit-testable, mirroring the split `src-tauri/src/tray/model.rs` and
// `src-tauri/src/settings.rs` use.

import type {
  AppSettings,
  IconPreview,
  IconStyle,
  LinuxDesktop,
  UsageSnapshot,
} from "./types";

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
  pace_tracking_enabled: true,
  usage_mode: "auto",
  debug_logging: false,
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

/** Minimum fraction of the panel's height a tray icon should actually draw at
 * before it stops being readable.
 *
 * Plasma pins a tray item's width to its height (`AbstractItem.qml` sets
 * `implicitWidth: itemSize`) and fits the artwork inside that square
 * preserving aspect, so a `W x H` icon renders at `H / W` of the panel height.
 * The wide styles that bake in a percentage number lose badly: Battery is
 * 66x22, so it draws at a third of panel height. */
export const SQUARE_TRAY_MIN_FILL = 0.8;

/** How much of Plasma's square tray cell this style's artwork fills
 * vertically: `1` for anything at least as tall as it is wide, `height /
 * width` otherwise. Derived from the preview's own dimensions rather than a
 * hardcoded style list, so adding a style cannot leave this stale. */
export function squareTrayFill(preview: IconPreview): number {
  if (preview.width <= 0 || preview.height <= 0) {
    return 0;
  }
  return preview.width <= preview.height ? 1 : preview.height / preview.width;
}

/** The styles that stay readable in a square tray cell, in display order. */
export function squareTrayStyles(previews: readonly IconPreview[]): IconStyle[] {
  return previews
    .filter((preview) => squareTrayFill(preview) >= SQUARE_TRAY_MIN_FILL)
    .map((preview) => preview.style);
}

/** Whether to nudge the user toward a squarer tray icon, how badly the current
 * one is squashed, and which ones to offer.
 *
 * Only on Plasma, only when the *current* style is one of the ones that draws
 * small, and only when there is something better to switch to — so the hint
 * disappears the moment it has been acted on, and never appears on a desktop
 * where it would be wrong. `null` means "say nothing".
 *
 * `fill` is returned so the message can state the real figure: Battery is a
 * third of panel height but Minimal is a half, and both trigger this. */
export function squareTrayHint(
  desktop: LinuxDesktop,
  current: IconStyle,
  previews: readonly IconPreview[],
): { fill: number; alternatives: IconStyle[] } | null {
  if (desktop !== "kde") {
    return null;
  }
  const currentPreview = previews.find((preview) => preview.style === current);
  if (!currentPreview) {
    return null;
  }
  const fill = squareTrayFill(currentPreview);
  if (fill >= SQUARE_TRAY_MIN_FILL) {
    return null;
  }
  const alternatives = squareTrayStyles(previews);
  return alternatives.length > 0 ? { fill, alternatives } : null;
}
