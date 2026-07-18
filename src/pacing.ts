// Pacing math. Mirrors `meter_core::pacing` (and its Swift oracle,
// `ClaudeMeter`'s `UsageLimit`/`PacePalette`) NUMERICALLY EXACTLY — the same
// constants and formulas — so the popover's pace ratio, projections and
// bands light up under the same conditions as the tray icon's. The vitest
// cases in `pacing.test.ts` pin the same upstream oracle numbers the Rust
// `pacing.rs` tests do; keep the two in lockstep.

import type { LimitWindow, UsageWindow } from "./types";

/** Burning usage more than 20% faster than a sustainable, even pace counts
 * as at-risk (mirrors `meter_core::pacing::RISK_THRESHOLD`). */
export const RISK_THRESHOLD = 1.2;

/** Below this ratio the weekly quota is likely to go unused before reset
 * (mirrors `meter_core::pacing::UNDERUSE_THRESHOLD`). */
export const UNDERUSE_THRESHOLD = 0.8;

/** Above this ratio overuse is shown as heavy — red rather than orange
 * (mirrors `meter_core::pacing::HEAVY_OVERUSE_THRESHOLD`). */
export const HEAVY_OVERUSE_THRESHOLD = 2.5;

/** Minimum utilization before a limit-hit is projected: below it an early
 * front-loaded burst is noise; at or above it a lockout warning fires
 * immediately, bypassing `MIN_ELAPSED_FRACTION` (mirrors
 * `meter_core::pacing::MIN_USAGE_FOR_PROJECTION`). */
export const MIN_USAGE_FOR_PROJECTION = 5.0;

/** Ignore pacing until this fraction of the window has elapsed; ratios
 * against a nearly-empty denominator are noise, not signal. */
const MIN_ELAPSED_FRACTION = 0.05;

const WINDOW_DURATION_MS: Record<LimitWindow, number> = {
  five_hour: 5 * 60 * 60 * 1000,
  seven_day: 7 * 24 * 60 * 60 * 1000,
};

/** The span, in milliseconds, a weekly quota is expected to be consumed over
 * given a pace-days setting (5–7). Mirrors
 * `meter_core::pacing::weekly_pacing_duration`. */
export function weeklyPacingDurationMs(days: number): number {
  return days * 24 * 60 * 60 * 1000;
}

/** The discrete pace band a ratio falls into. Snake-case values match
 * `meter_core::PaceBand`'s serde spelling. */
export type PaceBand = "underuse" | "sustainable" | "overuse" | "heavy_overuse";

/** Classify a pace ratio. Blue underuse (`<0.8×`), green sustainable
 * (`0.8–1.2×`), orange overuse (`1.2–2.5×`), red heavy overuse (`>2.5×`).
 * Mirrors `meter_core::PaceBand::from_ratio`. */
export function paceBand(ratio: number): PaceBand {
  if (ratio < UNDERUSE_THRESHOLD) {
    return "underuse";
  }
  if (ratio <= RISK_THRESHOLD) {
    return "sustainable";
  }
  if (ratio <= HEAVY_OVERUSE_THRESHOLD) {
    return "overuse";
  }
  return "heavy_overuse";
}

/** Fraction of the window that has elapsed at `now`, clamped to `0..=1`.
 * The window start is derived from `resets_at` minus the window length. */
export function elapsedFraction(window: UsageWindow, now: Date): number {
  const duration = WINDOW_DURATION_MS[window.window];
  const remainingMs = new Date(window.resets_at).getTime() - now.getTime();
  const fraction = 1 - remainingMs / duration;
  return Math.min(Math.max(fraction, 0), 1);
}

/** Milliseconds elapsed since the window started (`resets_at - window` up to
 * `now`). `null` once the window has reset (`resets_at <= now`). May be
 * non-positive under clock skew; callers guard as needed. Mirrors
 * `meter_core::pacing::UsageWindow::elapsed_secs`. */
function elapsedMs(window: UsageWindow, now: Date): number | null {
  const resetsAt = new Date(window.resets_at).getTime();
  const nowMs = now.getTime();
  if (resetsAt <= nowMs) {
    return null;
  }
  const windowMs = WINDOW_DURATION_MS[window.window];
  return windowMs - (resetsAt - nowMs);
}

/** Utilization percentage the pace plan expects by now (0–100): the elapsed
 * fraction of the *pacing span*, ×100, capped at 100%. `pacingMs` is the span
 * the quota is expected to be consumed over; omit for the full window. `null`
 * when the window has reset, the span is non-positive, or less than
 * `MIN_ELAPSED_FRACTION` of the span has elapsed. Mirrors
 * `UsageWindow::expected_usage_percent`. */
export function expectedUsagePercent(
  window: UsageWindow,
  now: Date,
  pacingMs?: number,
): number | null {
  const pacing = pacingMs ?? WINDOW_DURATION_MS[window.window];
  if (pacing <= 0) {
    return null;
  }
  const elapsed = elapsedMs(window, now);
  if (elapsed === null) {
    return null;
  }
  const fraction = Math.min(elapsed / pacing, 1.0);
  if (fraction < MIN_ELAPSED_FRACTION) {
    return null;
  }
  return fraction * 100;
}

/** Ratio of usage fraction to elapsed-time fraction of the pacing span:
 * `min(utilization, 100) / expectedUsagePercent`. 1.0 is exactly sustainable,
 * `>1` burning faster, `<1` underusing. `null` under the same conditions as
 * `expectedUsagePercent`. Mirrors `UsageWindow::pace_ratio`. */
export function paceRatio(window: UsageWindow, now: Date, pacingMs?: number): number | null {
  const expected = expectedUsagePercent(window, now, pacingMs);
  if (expected === null) {
    return null;
  }
  return Math.min(window.utilization, 100) / expected;
}

/** Projected utilization percentage at the pacing deadline if the current
 * average rate holds, extrapolated to `max(pacingMs ?? window, elapsed)` so it
 * shares a time basis with `paceRatio`. `null` when the window has reset or
 * less than `MIN_ELAPSED_FRACTION` of the *window* has elapsed. Mirrors
 * `UsageWindow::projected_end_percent`. */
export function projectedEndPercent(
  window: UsageWindow,
  now: Date,
  pacingMs?: number,
): number | null {
  const windowMs = WINDOW_DURATION_MS[window.window];
  const elapsed = elapsedMs(window, now);
  if (elapsed === null || elapsed < windowMs * MIN_ELAPSED_FRACTION) {
    return null;
  }
  // Never project a horizon shorter than what has already elapsed.
  const horizon = Math.max(pacingMs ?? windowMs, elapsed);
  return window.utilization * (horizon / elapsed);
}

/** When the limit will be hit at the current average rate, if that lands on or
 * before the pacing deadline; `null` otherwise (or if already exceeded / the
 * window has reset). Unlike `projectedEndPercent`, fires as soon as
 * utilization clears `MIN_USAGE_FOR_PROJECTION` — bypassing the elapsed-time
 * grace but honouring the utilization floor. Mirrors
 * `UsageWindow::projected_limit_date`. */
export function projectedLimitDate(
  window: UsageWindow,
  now: Date,
  pacingMs?: number,
): Date | null {
  const resetsAt = new Date(window.resets_at).getTime();
  if (
    window.utilization >= 100.0 ||
    window.utilization < MIN_USAGE_FOR_PROJECTION ||
    resetsAt <= now.getTime()
  ) {
    return null;
  }
  const windowMs = WINDOW_DURATION_MS[window.window];
  const elapsed = elapsedMs(window, now);
  if (elapsed === null || elapsed <= 0) {
    return null;
  }
  const hitOffset = (elapsed * 100) / window.utilization;
  const deadlineOffset = Math.max(pacingMs ?? windowMs, elapsed);
  if (hitOffset >= deadlineOffset) {
    return null;
  }
  // hitDate = windowStart + hitOffset = resets_at - window + hitOffset.
  return new Date(resetsAt - windowMs + hitOffset);
}

/** Whether a window is pacing faster than sustainable at `now`. Shares its
 * ratio math with `paceRatio`, mirroring
 * `meter_core::pacing::PacingAssessment::for_window`: false when nothing has
 * been used yet, while less than `MIN_ELAPSED_FRACTION` of the window has
 * elapsed, or once the window has already reset. */
export function isAtRisk(window: UsageWindow, now: Date): boolean {
  if (window.utilization <= 0) {
    return false;
  }
  const ratio = paceRatio(window, now);
  return ratio !== null && ratio > RISK_THRESHOLD;
}
