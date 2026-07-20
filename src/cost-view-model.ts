// Pure view-model for the token/cost usage view (Enterprise accounts, and the
// cost-summary card shown alongside the allowance cards). No DOM, no Tauri —
// fully unit-testable, mirroring the split `view-model.ts` uses for the
// percentage windows.
//
// The effective-mode helpers mirror `meter_core`'s `UsageSnapshot::has_limits`
// / `suggested_mode` and `UsageMode::effective` (crates/meter-core/src/{
// snapshot.rs, mode.rs}) so the frontend resolves Auto to the same concrete
// view the tray does. Money stays as i64 cents until it reaches
// `formatMoney` (see format.ts and the tray's Rust `format_money`).

import { formatMoney, roundPercent } from "./format";
import { type UsageStatus, statusFromUtilization } from "./status";
import type { Spend, UsageMode, UsageSnapshot } from "./types";

/** The concrete view a resolved [`UsageMode`] picks — never `"auto"`. */
export type EffectiveUsageMode = "allowance" | "cost";

/** Whether the account reports any allowance limit — a headline window or a
 * model-scoped one. Mirrors `UsageSnapshot::has_limits`. `null`/absent
 * snapshots have no limits. */
export function hasLimits(snapshot: UsageSnapshot | null): boolean {
  if (!snapshot) {
    return false;
  }
  return snapshot.five_hour !== null || snapshot.seven_day !== null || snapshot.scoped.length > 0;
}

/** The view auto-detection picks for a snapshot: the allowance view when
 * limits are present, otherwise the cost view — but only when there is spend
 * data to show. A snapshot with neither limits nor spend stays on the
 * allowance view. Mirrors `UsageSnapshot::suggested_mode`. */
export function suggestedMode(snapshot: UsageSnapshot | null): EffectiveUsageMode {
  if (!snapshot) {
    return "allowance";
  }
  return hasLimits(snapshot) || snapshot.spend === null ? "allowance" : "cost";
}

/** Resolve a (possibly `auto`) usage mode against a snapshot to the concrete
 * view to render. A pinned `allowance`/`cost` is returned unchanged; `auto`
 * follows [`suggestedMode`]. Mirrors `UsageMode::effective`. */
export function effectiveUsageMode(
  mode: UsageMode,
  snapshot: UsageSnapshot | null,
): EffectiveUsageMode {
  return mode === "auto" ? suggestedMode(snapshot) : mode;
}

/** The spend-budget gauge, shown only when a spend limit (or cap) is known. */
export interface CostGaugeViewModel {
  /** Spend-to-date as a percentage of the budget, rounded and clamped to 0-100
   * for the bar width. */
  percent: number;
  /** Traffic-light status from the (unclamped) fraction, so an overspend
   * reads critical. */
  status: UsageStatus;
  /** The formatted spend budget ("€2000.00"). */
  budget: string;
}

/** Everything the cost view / cost-summary card renders from one `Spend`. */
export interface CostViewModel {
  /** Formatted spend to date ("€0.35"), or `null` when unknown. */
  used: string | null;
  /** The spend-budget gauge, or `null` when there is no budget to gauge
   * against. */
  gauge: CostGaugeViewModel | null;
  /** Formatted hard cap ("€2000.00"), shown only when it differs from the
   * gauge's budget (i.e. a cap distinct from the limit), else `null`. */
  cap: string | null;
}

/** Build the cost view-model from a `Spend`. `warning`/`critical` are the
 * user's configured thresholds so the spend gauge escalates exactly where the
 * usage meters do. Mirrors the tray's `cost_usage_lines`/`cost_icon`. */
export function buildCostViewModel(
  spend: Spend,
  warning?: number,
  critical?: number,
): CostViewModel {
  const used = spend.used ? formatMoney(spend.used) : null;
  const gauge = costGauge(spend, warning, critical);
  // Surface the hard cap only when there is a separate limit it exceeds. With no
  // limit the cap *is* the gauge's budget, so repeating it would be redundant.
  const cap =
    spend.cap && spend.limit && spend.cap.minor !== spend.limit.minor
      ? formatMoney(spend.cap)
      : null;
  return { used, gauge, cap };
}

/** The gauge for a spend budget: spend-to-date over the limit (falling back to
 * the hard cap) as a status-coloured percentage. `null` when either figure is
 * missing or the budget is non-positive (mirrors `Spend::fraction_used`). */
function costGauge(spend: Spend, warning?: number, critical?: number): CostGaugeViewModel | null {
  const used = spend.used;
  const budget = spend.limit ?? spend.cap;
  if (!used || !budget || budget.minor <= 0) {
    return null;
  }
  const percentValue = (used.minor / budget.minor) * 100;
  return {
    percent: roundPercent(percentValue),
    status: statusFromUtilization(percentValue, warning, critical),
    budget: formatMoney(budget),
  };
}
