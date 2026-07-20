// Display formatting: percentages, coarse durations and the reset
// countdown. `shortDuration`/`RESET_SOON_GRACE_SECS` mirror
// `src-tauri/src/tray/model.rs` so the popover and the tray menu read the
// same way.

import type { Money } from "./types";

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

const RESET_TIME_ONLY = new Intl.DateTimeFormat(undefined, { timeStyle: "short" });
const RESET_DATE_TIME = new Intl.DateTimeFormat(undefined, {
  month: "short",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
});

/** The exact reset wall-clock time, in the user's locale/timezone, shown in
 * parentheses next to the countdown (ClaudeMeter PR #26). `timeOnly` (the
 * 5-hour session card) drops the date — "11:30 PM" — since it always resets
 * today; every other window keeps the month and day but no year —
 * "Jul 19, 11:00 AM". */
export function formatResetClock(resetsAt: Date, timeOnly: boolean): string {
  return (timeOnly ? RESET_TIME_ONLY : RESET_DATE_TIME).format(resetsAt);
}

function pluralize(singular: string, count: number): string {
  return count === 1 ? singular : `${singular}s`;
}

/** Verbose remaining-time phrase for the pace projection line, mirroring
 * upstream's `UsageLimit.resetDescription` minus its "in " prefix: "50
 * minutes", "3 hours", "2 days 3 hours". Rounds up so it never understates
 * how long is left. */
export function describeRemaining(totalSecs: number): string {
  const minute = 60;
  const hour = 60 * minute;
  const day = 24 * hour;
  if (totalSecs <= 0) {
    return "now";
  }
  if (totalSecs < hour) {
    const minutes = Math.max(1, Math.ceil(totalSecs / minute));
    return `${minutes} ${pluralize("minute", minutes)}`;
  }
  if (totalSecs < day) {
    const hours = Math.ceil(totalSecs / hour);
    return `${hours} ${pluralize("hour", hours)}`;
  }
  const roundedHours = Math.ceil(totalSecs / hour);
  const days = Math.floor(roundedHours / 24);
  const hours = roundedHours % 24;
  if (hours === 0) {
    return `${days} ${pluralize("day", days)}`;
  }
  return `${days} ${pluralize("day", days)} ${hours} ${pluralize("hour", hours)}`;
}

/** Known currency glyphs. Anything else falls back to showing the ISO code
 * after the amount, matching the Rust `currency_symbol` on the tray side. */
const CURRENCY_SYMBOLS: Record<string, string> = {
  USD: "$",
  EUR: "€",
  GBP: "£",
  JPY: "¥",
};

/** Format a `Money` in its own currency, e.g. `"€0.35"`, `"$125.00"`, or
 * `"1000.00 SEK"` for a currency without a known glyph. Mirrors the tray's Rust
 * `format_money` (`src-tauri/src/tray/model/mod.rs`). The value comes from the
 * API in minor units with the currency's decimal-place count, so the exact
 * figure is preserved without floating-point rounding. A negative amount (which
 * should not normally occur) keeps a leading `-` so a bad figure stays visible. */
export function formatMoney(money: Money): string {
  const sign = money.minor < 0 ? "-" : "";
  const abs = Math.abs(Math.trunc(money.minor));
  const divisor = 10 ** money.exponent;
  const major = Math.floor(abs / divisor);
  const amount =
    money.exponent === 0
      ? String(major)
      : `${major}.${String(abs % divisor).padStart(money.exponent, "0")}`;
  const symbol = CURRENCY_SYMBOLS[money.currency];
  return symbol ? `${sign}${symbol}${amount}` : `${sign}${amount} ${money.currency}`;
}

/** Wall-clock time for a projected limit-hit: time-only when it lands today
 * (the 5-hour session case), month/day + time otherwise (a multi-day weekly
 * projection) so it isn't shown as a bare clock time that reads like today. */
export function formatHitTime(hitAt: Date, now: Date): string {
  const sameDay =
    hitAt.getFullYear() === now.getFullYear() &&
    hitAt.getMonth() === now.getMonth() &&
    hitAt.getDate() === now.getDate();
  return (sameDay ? RESET_TIME_ONLY : RESET_DATE_TIME).format(hitAt);
}
