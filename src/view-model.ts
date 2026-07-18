// Pure view-model: `MeterState` + `now` -> what the popover renders. No DOM,
// no Tauri, no timers — fully unit-testable, mirroring the split
// `src-tauri/src/tray/model.rs` uses for the tray menu.

import { formatAge, roundPercent } from "./format";
import {
  type PaceBand,
  UNDERUSE_THRESHOLD,
  expectedUsagePercent,
  isAtRisk,
  paceBand,
  paceRatio,
  projectedEndPercent,
  projectedLimitDate,
  weeklyPacingDurationMs,
} from "./pacing";
import {
  DEFAULT_CRITICAL_THRESHOLD,
  DEFAULT_WARNING_THRESHOLD,
  type UsageStatus,
  statusFromUtilization,
} from "./status";
import type { LimitWindow, MeterState, UsageWindow } from "./types";

/** The projection line at the current burn rate. `null` when too little of
 * the window has elapsed to project (and usage is below the lockout floor).
 * Mirrors upstream `UsageCardView.projectionLine`. */
export type ProjectionViewModel =
  | { kind: "reached" }
  | { kind: "hits"; hitAt: string; secondsBeforeReset: number }
  | { kind: "ends"; endPercent: number; unusedPercent: number | null };

/** One usage card: a headline window or a named, visible scoped limit. */
export interface UsageCardViewModel {
  /** Stable DOM/list key: "five_hour", "seven_day", or `scoped:<name>`. */
  id: string;
  title: string;
  percent: number;
  status: UsageStatus;
  /** RFC 3339 instant; the countdown is recomputed from this every tick. */
  resetsAt: string;
  atRisk: boolean;
  /** Whether to append the exact reset wall-clock time (Settings toggle,
   * ClaudeMeter PR #26). */
  showResetTime: boolean;
  /** Drop the date from the reset clock (true only for the 5-hour session
   * card, which always resets today). */
  useTimeOnlyResetTime: boolean;
  /** Pace ratio (1.0 = sustainable), or `null` inside the grace period /
   * after reset — pace UI is suppressed when `null`. */
  paceRatio: number | null;
  /** Colour band for `paceRatio`, or `null` when there is no ratio. */
  paceBand: PaceBand | null;
  /** Utilization the plan expected by now (drives the expected-by-now tick
   * and the "N% expected" secondary), or `null`. */
  expectedPercent: number | null;
  /** Whether this card leads with the pace ratio instead of the quota % —
   * the pace-first setting, but only once a ratio exists to lead with. */
  paceFirst: boolean;
  /** Whether underuse is a meaningful signal here (weekly + scoped, never the
   * session card): gates the blue "quota unused" projection styling. */
  showsUnderuse: boolean;
  /** The current-rate projection line, or `null` when it can't be projected. */
  projection: ProjectionViewModel | null;
}

/** Coarse banner state driving the popover's top-of-card messaging. `"ok"`
 * means "healthy and fresh" — the caller renders no banner at all. */
export type BannerKind =
  | "loading"
  | "awaiting_session"
  | "session_expired"
  | "degraded"
  | "stale"
  | "ok";

export interface PopoverViewModel {
  bannerKind: BannerKind;
  /** One-line human summary, always present (mirrors the tray's status line). */
  statusLine: string;
  cards: UsageCardViewModel[];
  /** True when the session-key CTA form should be shown. */
  showSessionForm: boolean;
}

const HEADLINE_LABELS: Record<LimitWindow, string> = {
  five_hour: "5-hour",
  seven_day: "7-day",
};

/** Settings-derived options that shape every card (kept as a bag so the
 * per-card builder doesn't grow an ever-longer positional signature). */
interface CardOptions {
  showResetTime: boolean;
  warning: number;
  critical: number;
  weeklyPaceDays: number;
  paceFirst: boolean;
}

/** Build the current-rate projection descriptor, mirroring upstream
 * `UsageCardView.projectionLine`: a reached limit, a projected limit-hit
 * before reset, or the projected end-of-window percentage. */
function projectionFor(
  window: UsageWindow,
  now: Date,
  pacingMs: number | undefined,
  showsUnderuse: boolean,
  ratio: number | null,
): ProjectionViewModel | null {
  if (window.utilization >= 100) {
    return { kind: "reached" };
  }
  const hit = projectedLimitDate(window, now, pacingMs);
  if (hit) {
    return {
      kind: "hits",
      hitAt: hit.toISOString(),
      secondsBeforeReset: (new Date(window.resets_at).getTime() - hit.getTime()) / 1000,
    };
  }
  const end = projectedEndPercent(window, now, pacingMs);
  if (end === null) {
    return null;
  }
  const endPercent = Math.round(Math.min(end, 100));
  // Blue "quota may go unused" styling only where underuse is meaningful.
  const unusedPercent =
    showsUnderuse && ratio !== null && ratio < UNDERUSE_THRESHOLD ? 100 - endPercent : null;
  return { kind: "ends", endPercent, unusedPercent };
}

function cardFor(
  id: string,
  title: string,
  window: UsageWindow,
  now: Date,
  opts: CardOptions,
): UsageCardViewModel {
  // The 5-hour session paces over its full window and never signals underuse;
  // the weekly and scoped weekly cards pace over the configured 5/6/7-day span.
  const isSession = id === "five_hour";
  const pacingMs = isSession ? undefined : weeklyPacingDurationMs(opts.weeklyPaceDays);
  const showsUnderuse = !isSession;
  const ratio = paceRatio(window, now, pacingMs);
  return {
    id,
    title,
    percent: roundPercent(window.utilization),
    status: statusFromUtilization(window.utilization, opts.warning, opts.critical),
    resetsAt: window.resets_at,
    atRisk: isAtRisk(window, now),
    showResetTime: opts.showResetTime,
    // Only the 5-hour session window always resets today, so it alone drops
    // the date from its reset clock.
    useTimeOnlyResetTime: isSession,
    paceRatio: ratio,
    paceBand: ratio === null ? null : paceBand(ratio),
    expectedPercent: expectedUsagePercent(window, now, pacingMs),
    // Pace-first only takes effect once there is a ratio to lead with;
    // otherwise the card falls back to the quota-first layout (upstream's
    // `if isPaceFirst, let paceRatio` guard).
    paceFirst: opts.paceFirst && ratio !== null,
    showsUnderuse,
    projection: projectionFor(window, now, pacingMs, showsUnderuse, ratio),
  };
}

/** Build the popover's full view-model from one broadcast state.
 *
 * `shownScopedModels` is the user's opt-in set of scoped-model display names
 * from Settings (issue #6, `AppSettings.shown_scoped_models`) — empty by
 * default, so a freshly reported model stays out of the popover until
 * switched on, mirroring `tray::model::menu_model` on the Rust side. */
export function buildViewModel(
  state: MeterState,
  now: Date,
  shownScopedModels: ReadonlySet<string>,
  showResetTime = true,
  warning: number = DEFAULT_WARNING_THRESHOLD,
  critical: number = DEFAULT_CRITICAL_THRESHOLD,
  weeklyPaceDays = 7,
  paceFirst = false,
): PopoverViewModel {
  const opts: CardOptions = { showResetTime, warning, critical, weeklyPaceDays, paceFirst };
  const cards: UsageCardViewModel[] = [];
  const snapshot = state.snapshot;
  if (snapshot) {
    if (snapshot.five_hour) {
      cards.push(cardFor("five_hour", HEADLINE_LABELS.five_hour, snapshot.five_hour, now, opts));
    }
    if (snapshot.seven_day) {
      cards.push(cardFor("seven_day", HEADLINE_LABELS.seven_day, snapshot.seven_day, now, opts));
    }
    for (const limit of snapshot.scoped) {
      // Only visible (active) *and* opted-in scoped limits render as cards.
      // `is_active` is real API data (plan doesn't include it, surface-only
      // scope, ...); `shownScopedModels` is the user's own Settings choice —
      // both gates must pass.
      if (!limit.is_active || !shownScopedModels.has(limit.display_name)) {
        continue;
      }
      cards.push(cardFor(`scoped:${limit.display_name}`, limit.display_name, limit.usage, now, opts));
    }
  }

  return {
    bannerKind: bannerKind(state),
    statusLine: statusLine(state, now),
    cards,
    showSessionForm: state.phase === "awaiting_session" || state.phase === "session_expired",
  };
}

function bannerKind(state: MeterState): BannerKind {
  switch (state.phase) {
    case "awaiting_session":
      return "awaiting_session";
    case "session_expired":
      return "session_expired";
    case "degraded":
      return "degraded";
    case "polling":
      if (!state.snapshot) {
        return "loading";
      }
      return state.staleness === "stale" ? "stale" : "ok";
  }
}

/** The one-line phase/freshness summary. Whenever a cached snapshot is still
 * shown while polling is paused or failing, its age is surfaced here so the
 * cards are never presented as current data. Mirrors
 * `src-tauri/src/tray/model.rs::status_line`. */
function statusLine(state: MeterState, now: Date): string {
  const age = state.snapshot ? formatAge(new Date(state.snapshot.fetched_at), now) : null;
  switch (state.phase) {
    case "awaiting_session":
      return age === null
        ? "No session key — paste one below to get started"
        : `No session key — showing data from ${age}`;
    case "session_expired":
      return age === null
        ? "Session expired — paste a new key below"
        : `Session expired — showing data from ${age}`;
    case "degraded":
      return age === null ? "Connection trouble — retrying" : `Connection trouble — data from ${age}`;
    case "polling":
      if (age === null) {
        return "Waiting for first update…";
      }
      return state.staleness === "stale" ? `Stale — updated ${age}` : `Updated ${age}`;
  }
}
