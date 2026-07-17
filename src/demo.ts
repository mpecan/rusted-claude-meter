// Demo/mock data for development outside a Tauri shell (`npm run dev`).
// Mirrors `crates/meter-api/tests/fixtures/usage_response.json` post-decode
// (see `crates/meter-api/tests/decode.rs`): the same headline percentages,
// the same two named scoped limits, and — the acceptance-criteria case —
// Fable carries no `model_id` while Sonnet does.

import type { MeterState } from "./types";

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
    fetched_at: "2026-07-17T12:00:00Z",
  },
  staleness: "fresh",
  phase: "polling",
};
