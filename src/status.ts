// Traffic-light utilization classification for the popover. Unlike the tray
// (which mirrors `meter_core`'s fixed bands), the popover recolours green →
// amber → red at the *user's configured* warning/critical thresholds
// (Settings), so the meters escalate exactly where the user's notifications do.

export type UsageStatus = "safe" | "warning" | "critical";

/** Default thresholds when no settings are available (mirrors the app
 * defaults). */
export const DEFAULT_WARNING_THRESHOLD = 75.0;
export const DEFAULT_CRITICAL_THRESHOLD = 90.0;

/** Classify a utilization percentage against the given thresholds. `warning`
 * and `critical` are the user's configured 0-100 percentages; at/above
 * `critical` is critical, at/above `warning` is warning, else safe. */
export function statusFromUtilization(
  percent: number,
  warning: number = DEFAULT_WARNING_THRESHOLD,
  critical: number = DEFAULT_CRITICAL_THRESHOLD,
): UsageStatus {
  if (percent >= critical) {
    return "critical";
  }
  if (percent >= warning) {
    return "warning";
  }
  return "safe";
}

/** Short uppercase label for a status pill (redesign 1c cards). */
export function statusLabel(status: UsageStatus): string {
  switch (status) {
    case "critical":
      return "CRITICAL";
    case "warning":
      return "HIGH";
    case "safe":
      return "OK";
  }
}
