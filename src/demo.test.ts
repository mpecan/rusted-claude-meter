// Tripwire against fixture/demo drift: `DEMO_STATE` is a hand-transcribed
// TypeScript literal of the *decoded* shape of
// `crates/meter-api/tests/fixtures/usage_response.json` (see demo.ts's
// header comment and `crates/meter-api/tests/decode.rs`). Nothing else
// checks that the two stay in sync, so this test re-derives the decoded
// snapshot from the raw fixture JSON — mirroring
// `meter_api::UsageResponse::into_snapshot`'s mapping rules — and asserts it
// against `DEMO_STATE`. A future fixture edit that isn't mirrored into
// demo.ts fails here instead of drifting silently.

import { describe, expect, it } from "vitest";

import { DEMO_STATE } from "./demo";
import rawFixture from "../crates/meter-api/tests/fixtures/usage_response.json";
import type { LimitWindow, ScopedLimit, UsageWindow } from "./types";

const HEADLINE_KINDS = new Set(["five_hour", "seven_day"]);

interface RawWindow {
  utilization: number;
  resets_at: string | null;
}

interface RawLimit {
  kind: string;
  percent: number | null;
  resets_at: string | null;
  is_active: boolean;
  scope: { model: { id: string | null; display_name: string | null } | null } | null;
}

interface RawResponse {
  five_hour: RawWindow | null;
  seven_day: RawWindow | null;
  limits: RawLimit[];
}

function windowForKind(kind: string): LimitWindow {
  return kind.startsWith("five_hour") ? "five_hour" : "seven_day";
}

function headlineWindow(raw: RawWindow | null, window: LimitWindow): UsageWindow | null {
  if (!raw || raw.resets_at === null) {
    return null;
  }
  return { utilization: raw.utilization, resets_at: raw.resets_at, window };
}

function scopedLimit(raw: RawLimit): ScopedLimit | null {
  const displayName = raw.scope?.model?.display_name ?? null;
  if (displayName === null || raw.percent === null || raw.resets_at === null) {
    return null;
  }
  return {
    display_name: displayName,
    model_id: raw.scope?.model?.id ?? null,
    usage: { utilization: raw.percent, resets_at: raw.resets_at, window: windowForKind(raw.kind) },
    is_active: raw.is_active,
  };
}

describe("DEMO_STATE vs. the Rust fixture", () => {
  it("mirrors the decoded shape of usage_response.json", () => {
    const raw = rawFixture as unknown as RawResponse;
    const scoped = raw.limits
      .filter((limit) => !HEADLINE_KINDS.has(limit.kind))
      .map(scopedLimit)
      .filter((limit): limit is ScopedLimit => limit !== null);

    expect(DEMO_STATE.snapshot?.five_hour).toEqual(headlineWindow(raw.five_hour, "five_hour"));
    expect(DEMO_STATE.snapshot?.seven_day).toEqual(headlineWindow(raw.seven_day, "seven_day"));
    expect(DEMO_STATE.snapshot?.scoped).toEqual(scoped);
  });
});
