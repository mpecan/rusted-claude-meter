// Pure view-model: `MeterState` + `now` -> what the popover renders. No DOM,
// no Tauri, no timers — fully unit-testable, mirroring the split
// `src-tauri/src/tray/model.rs` uses for the tray menu.

import { formatAge, roundPercent } from "./format";
import { isAtRisk } from "./pacing";
import { type UsageStatus, statusFromUtilization } from "./status";
import type { LimitWindow, MeterState, UsageWindow } from "./types";

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

function cardFor(id: string, title: string, window: UsageWindow, now: Date): UsageCardViewModel {
  return {
    id,
    title,
    percent: roundPercent(window.utilization),
    status: statusFromUtilization(window.utilization),
    resetsAt: window.resets_at,
    atRisk: isAtRisk(window, now),
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
): PopoverViewModel {
  const cards: UsageCardViewModel[] = [];
  const snapshot = state.snapshot;
  if (snapshot) {
    if (snapshot.five_hour) {
      cards.push(cardFor("five_hour", HEADLINE_LABELS.five_hour, snapshot.five_hour, now));
    }
    if (snapshot.seven_day) {
      cards.push(cardFor("seven_day", HEADLINE_LABELS.seven_day, snapshot.seven_day, now));
    }
    for (const limit of snapshot.scoped) {
      // Only visible (active) *and* opted-in scoped limits render as cards.
      // `is_active` is real API data (plan doesn't include it, surface-only
      // scope, ...); `shownScopedModels` is the user's own Settings choice —
      // both gates must pass.
      if (!limit.is_active || !shownScopedModels.has(limit.display_name)) {
        continue;
      }
      cards.push(cardFor(`scoped:${limit.display_name}`, limit.display_name, limit.usage, now));
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
