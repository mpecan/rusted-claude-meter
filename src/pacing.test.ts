import { describe, expect, it } from "vitest";

import { elapsedFraction, isAtRisk } from "./pacing";
import type { UsageWindow } from "./types";

const NOW = new Date("2026-07-17T12:00:00Z");

function fiveHourWindow(utilization: number, resetsInMinutes: number): UsageWindow {
  return {
    utilization,
    resets_at: new Date(NOW.getTime() + resetsInMinutes * 60_000).toISOString(),
    window: "five_hour",
  };
}

describe("elapsedFraction", () => {
  it("is midway through a five-hour window at 2.5h remaining", () => {
    const w = fiveHourWindow(40, 1 * 60);
    expect(elapsedFraction(w, NOW)).toBeCloseTo(0.8, 9);
  });

  it("clamps to 0 before the window start (clock skew / fresh window)", () => {
    const w = fiveHourWindow(0, 10 * 60);
    expect(elapsedFraction(w, NOW)).toBe(0);
  });

  it("clamps to 1 after the reset", () => {
    const w = fiveHourWindow(90, -60);
    expect(elapsedFraction(w, NOW)).toBe(1);
  });
});

describe("isAtRisk", () => {
  it("is not at risk at a sustainable pace", () => {
    // Half the window gone, half the budget used -> ratio 1.0.
    expect(isAtRisk(fiveHourWindow(50, 150), NOW)).toBe(false);
  });

  it("is at risk when burning faster than sustainable", () => {
    // Half the window gone, 80% used -> ratio 1.6.
    expect(isAtRisk(fiveHourWindow(80, 150), NOW)).toBe(true);
  });

  it("is not at risk in a barely-started window (below the elapsed floor)", () => {
    expect(isAtRisk(fiveHourWindow(5, 297), NOW)).toBe(false);
  });

  it("is not at risk once the window has already reset", () => {
    expect(isAtRisk(fiveHourWindow(90, -10), NOW)).toBe(false);
  });

  it("is not at risk when nothing has been used yet", () => {
    expect(isAtRisk(fiveHourWindow(0, 150), NOW)).toBe(false);
  });
});
