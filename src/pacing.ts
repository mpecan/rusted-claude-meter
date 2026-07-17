// Pacing-risk assessment. Mirrors `meter_core::pacing::PacingAssessment`
// exactly (same risk threshold, same minimum-elapsed guard) so the popover's
// flame badge lights up under the same conditions as the tray icon's.

import type { LimitWindow, UsageWindow } from "./types";

/** Burning usage more than 20% faster than a sustainable, even pace counts
 * as at-risk (mirrors `meter_core::pacing::RISK_THRESHOLD`). */
export const RISK_THRESHOLD = 1.2;

/** Ignore pacing until this fraction of the window has elapsed; ratios
 * against a nearly-empty denominator are noise, not signal. */
const MIN_ELAPSED_FRACTION = 0.05;

const WINDOW_DURATION_MS: Record<LimitWindow, number> = {
  five_hour: 5 * 60 * 60 * 1000,
  seven_day: 7 * 24 * 60 * 60 * 1000,
};

/** Fraction of the window that has elapsed at `now`, clamped to `0..=1`.
 * The window start is derived from `resets_at` minus the window length. */
export function elapsedFraction(window: UsageWindow, now: Date): number {
  const duration = WINDOW_DURATION_MS[window.window];
  const remainingMs = new Date(window.resets_at).getTime() - now.getTime();
  const fraction = 1 - remainingMs / duration;
  return Math.min(Math.max(fraction, 0), 1);
}

/** Whether a window is pacing faster than sustainable at `now`.
 *
 * False while less than `MIN_ELAPSED_FRACTION` of the window has elapsed,
 * once the window has already reset, or when nothing has been used yet. */
export function isAtRisk(window: UsageWindow, now: Date): boolean {
  const elapsed = elapsedFraction(window, now);
  if (elapsed < MIN_ELAPSED_FRACTION || elapsed >= 1.0 || window.utilization <= 0) {
    return false;
  }
  const ratio = window.utilization / 100 / elapsed;
  return ratio > RISK_THRESHOLD;
}
