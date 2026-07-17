// Traffic-light utilization thresholds. Mirrors
// `meter_core::status::UsageStatus` exactly (same constants, same
// boundaries) so the popover agrees with the tray icon and menu about what
// "safe", "warning" and "critical" mean.

/** Utilization below this is "safe". */
export const WARNING_THRESHOLD = 50.0;
/** Utilization at or above this is "critical". */
export const CRITICAL_THRESHOLD = 80.0;

export type UsageStatus = "safe" | "warning" | "critical";

/** Classify a utilization percentage (0-100 scale; values above 100 are critical). */
export function statusFromUtilization(percent: number): UsageStatus {
  if (percent >= CRITICAL_THRESHOLD) {
    return "critical";
  }
  if (percent >= WARNING_THRESHOLD) {
    return "warning";
  }
  return "safe";
}
