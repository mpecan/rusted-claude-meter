// Display formatting: percentages, coarse durations and the reset
// countdown. `shortDuration`/`RESET_SOON_GRACE_SECS` mirror
// `src-tauri/src/tray/model.rs` so the popover and the tray menu read the
// same way.

/** Round a raw utilization percentage to the whole number cards display,
 * clamped to `0..=100` (the API can report utilization above 100). */
export function roundPercent(percent: number): number {
  return Math.round(Math.min(Math.max(percent, 0), 100));
}

/** A reset moment this recently in the past still reads "resets soon";
 * beyond it the line says how long ago the window reset. */
const RESET_SOON_GRACE_SECS = 5 * 60;

/** Coarse human duration: "3d 4h", "2h 15m", "12m", "under 1m". */
export function shortDuration(totalSecs: number): string {
  const secs = Math.max(totalSecs, 0);
  const days = Math.floor(secs / 86_400);
  const hours = Math.floor((secs % 86_400) / 3_600);
  const minutes = Math.floor((secs % 3_600) / 60);
  if (days > 0) {
    return `${days}d ${hours}h`;
  }
  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  if (minutes > 0) {
    return `${minutes}m`;
  }
  return "under 1m";
}

/** "resets in 2h 14m" / "resets soon" / "reset 2d 3h ago". Recomputed purely
 * from `resetsAt` and `now`, so a caller can call this every tick without
 * touching the network. */
export function formatCountdown(resetsAt: Date, now: Date): string {
  const remainingSecs = Math.floor((resetsAt.getTime() - now.getTime()) / 1000);
  if (remainingSecs > 0) {
    return `resets in ${shortDuration(remainingSecs)}`;
  }
  if (remainingSecs > -RESET_SOON_GRACE_SECS) {
    return "resets soon";
  }
  return `reset ${shortDuration(-remainingSecs)} ago`;
}

/** "3m ago" / "under 1m ago", for surfacing the age of stale/cached data. */
export function formatAge(fetchedAt: Date, now: Date): string {
  const ageSecs = Math.floor((now.getTime() - fetchedAt.getTime()) / 1000);
  return `${shortDuration(ageSecs)} ago`;
}
