// Demo/mock data for development outside a Tauri shell (`npm run dev`).
// Mirrors `crates/meter-api/tests/fixtures/usage_response.json` post-decode
// (see `crates/meter-api/tests/decode.rs`): the same headline percentages,
// the same two named scoped limits, and — the acceptance-criteria case —
// Fable carries no `model_id` while Sonnet does.

import type { MeterState, Spend } from "./types";

export const DEMO_STATE: MeterState = {
  snapshot: {
    five_hour: {
      utilization: 34.0,
      resets_at: "2026-07-17T15:00:00Z",
      window: "five_hour",
    },
    seven_day: {
      utilization: 61.0,
      resets_at: "2026-07-21T09:00:00Z",
      window: "seven_day",
    },
    scoped: [
      {
        display_name: "Fable",
        model_id: null,
        usage: {
          utilization: 50.0,
          resets_at: "2026-07-21T09:00:00Z",
          window: "seven_day",
        },
        is_active: true,
      },
      {
        display_name: "Sonnet",
        model_id: "claude-sonnet-5",
        usage: {
          utilization: 82.5,
          resets_at: "2026-07-21T09:00:00Z",
          window: "seven_day",
        },
        is_active: true,
      },
    ],
    spend: null,
    fetched_at: "2026-07-17T12:00:00Z",
  },
  staleness: "fresh",
  phase: "polling",
};

/** A demo spend object in the real captured shape (money as minor units +
 * currency + exponent). €125.00 used of a €2000.00 budget — used both for the
 * token/cost preview and the cost-summary card shown alongside the allowance
 * cards. */
export const DEMO_SPEND: Spend = {
  used: { minor: 12_500, currency: "EUR", exponent: 2 },
  limit: { minor: 200_000, currency: "EUR", exponent: 2 },
  cap: { minor: 200_000, currency: "EUR", exponent: 2 },
  enabled: true,
};

/** A token/cost account: no allowance limits, only a spend object — the
 * auto-detected cost view (`?mode=cost`). */
export const DEMO_COST_STATE: MeterState = {
  snapshot: {
    five_hour: null,
    seven_day: null,
    scoped: [],
    spend: DEMO_SPEND,
    fetched_at: "2026-07-17T12:00:00Z",
  },
  staleness: "fresh",
  phase: "polling",
};

/** The allowance account plus a spend object, so the allowance view's
 * cost-summary card is exercisable in a plain browser (`?mode=allowance`). */
export const DEMO_ALLOWANCE_WITH_COST_STATE: MeterState = {
  ...DEMO_STATE,
  snapshot: { ...DEMO_STATE.snapshot!, spend: DEMO_SPEND },
};
