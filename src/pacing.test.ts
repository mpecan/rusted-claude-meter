import { describe, expect, it } from "vitest";

import {
  UNDERUSE_THRESHOLD,
  elapsedFraction,
  expectedUsagePercent,
  isAtRisk,
  paceBand,
  paceRatio,
  projectedEndPercent,
  projectedLimitDate,
  weeklyPacingDurationMs,
} from "./pacing";
import type { LimitWindow, UsageWindow } from "./types";

const NOW = new Date("2026-07-17T12:00:00Z");

const WINDOW_MS: Record<LimitWindow, number> = {
  five_hour: 5 * 60 * 60 * 1000,
  seven_day: 7 * 24 * 60 * 60 * 1000,
};

const FIVE_DAYS_MS = weeklyPacingDurationMs(5);

function fiveHourWindow(utilization: number, resetsInMinutes: number): UsageWindow {
  return {
    utilization,
    resets_at: new Date(NOW.getTime() + resetsInMinutes * 60_000).toISOString(),
    window: "five_hour",
  };
}

/** A window with `utilization` used and `elapsedFraction` of `window`
 * elapsed at NOW — the TS mirror of the Rust/Swift `limit(...)` oracle
 * helper, so the same numbers pin both sides. */
function limit(utilization: number, elapsed: number, window: LimitWindow): UsageWindow {
  const remaining = WINDOW_MS[window] * (1 - elapsed);
  return {
    utilization,
    resets_at: new Date(NOW.getTime() + remaining).toISOString(),
    window,
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

describe("paceRatio", () => {
  it("is 1.0 at a sustainable pace", () => {
    expect(paceRatio(limit(50, 0.5, "five_hour"), NOW)).toBeCloseTo(1.0, 2);
  });

  it("is above 1 when burning fast (50% at 25% elapsed = 2.0)", () => {
    expect(paceRatio(limit(50, 0.25, "five_hour"), NOW)).toBeCloseTo(2.0, 2);
  });

  it("is below 1 when underusing (20% at 50% elapsed = 0.4)", () => {
    expect(paceRatio(limit(20, 0.5, "seven_day"), NOW)).toBeCloseTo(0.4, 2);
  });

  it("caps utilization at 100 (110% at 90% elapsed = 100/90)", () => {
    expect(paceRatio(limit(110, 0.9, "five_hour"), NOW)).toBeCloseTo(100 / 90, 2);
  });

  it("is null within the grace period below the usage floor", () => {
    expect(paceRatio(limit(1, 0.02, "five_hour"), NOW)).toBeNull();
  });

  it("surfaces within the grace period once usage clears the floor (10% at 2% = 5.0)", () => {
    expect(paceRatio(limit(10, 0.02, "five_hour"), NOW)).toBeCloseTo(5.0, 2);
  });

  it("is null once past reset", () => {
    expect(paceRatio(fiveHourWindow(50, -1), NOW)).toBeNull();
  });

  it("expects a faster burn on a 5-day basis (40% at 2/7 days = 1.0)", () => {
    expect(paceRatio(limit(40, 2 / 7, "seven_day"), NOW, FIVE_DAYS_MS)).toBeCloseTo(1.0, 2);
  });

  it("caps elapsed at the pacing span past it (day 6 of 7, 5-day basis = 0.7)", () => {
    expect(paceRatio(limit(70, 6 / 7, "seven_day"), NOW, FIVE_DAYS_MS)).toBeCloseTo(0.7, 2);
  });
});

describe("expectedUsagePercent", () => {
  it("is the elapsed fraction of the window ×100", () => {
    expect(expectedUsagePercent(limit(0, 0.5, "five_hour"), NOW)).toBeCloseTo(50, 2);
  });

  it("caps at 100 past the pacing span", () => {
    expect(expectedUsagePercent(limit(0, 6 / 7, "seven_day"), NOW, FIVE_DAYS_MS)).toBeCloseTo(
      100,
      2,
    );
  });
});

describe("projectedEndPercent", () => {
  it("extrapolates the current rate (40% at 50% elapsed -> 80%)", () => {
    expect(projectedEndPercent(limit(40, 0.5, "five_hour"), NOW)).toBeCloseTo(80, 1);
  });

  it("is null past reset", () => {
    expect(projectedEndPercent(fiveHourWindow(50, -1), NOW)).toBeNull();
  });

  it("is null within the grace period below the usage floor", () => {
    expect(projectedEndPercent(limit(1, 0.02, "five_hour"), NOW)).toBeNull();
  });

  it("surfaces within the grace period once usage clears the floor (10% at 2% -> 500%)", () => {
    expect(projectedEndPercent(limit(10, 0.02, "five_hour"), NOW)).toBeCloseTo(500, 0);
  });
});

describe("projectedLimitDate", () => {
  it("lands before reset when burning fast (60% at 50% elapsed -> 5/6 of window)", () => {
    const w = limit(60, 0.5, "five_hour");
    const hit = projectedLimitDate(w, NOW);
    expect(hit).not.toBeNull();
    const resetsAt = new Date(w.resets_at).getTime();
    expect(hit!.getTime()).toBeLessThan(resetsAt);
    const windowStart = resetsAt - WINDOW_MS.five_hour;
    const hitFraction = (hit!.getTime() - windowStart) / WINDOW_MS.five_hour;
    expect(hitFraction).toBeCloseTo(5 / 6, 2);
  });

  it("is null on a sustainable pace", () => {
    expect(projectedLimitDate(limit(50, 0.5, "five_hour"), NOW)).toBeNull();
  });

  it("is null when already exceeded", () => {
    expect(projectedLimitDate(limit(105, 0.5, "five_hour"), NOW)).toBeNull();
  });

  it("still warns on a front-loaded burst within the grace period", () => {
    // 60% burned in the first 2%: usage clears the floor, so the ratio now
    // surfaces (-> 30) alongside the lockout projection.
    const w = limit(60, 0.02, "five_hour");
    expect(paceRatio(w, NOW)).toBeCloseTo(30, 2);
    const hit = projectedLimitDate(w, NOW);
    expect(hit).not.toBeNull();
    expect(hit!.getTime()).toBeLessThan(new Date(w.resets_at).getTime());
  });

  it("is null for trivial early usage below the floor", () => {
    expect(projectedLimitDate(limit(1, 0.01, "five_hour"), NOW)).toBeNull();
  });

  it("respects the pacing basis without contradiction", () => {
    // 60% at 4/7 of the week paced over 5 days: under-pace, so no limit-hit
    // and the projected end stays below 100%.
    const w = limit(60, 4 / 7, "seven_day");
    expect(paceRatio(w, NOW, FIVE_DAYS_MS)!).toBeLessThan(UNDERUSE_THRESHOLD);
    expect(projectedLimitDate(w, NOW, FIVE_DAYS_MS)).toBeNull();
    expect(projectedEndPercent(w, NOW, FIVE_DAYS_MS)!).toBeLessThan(100);
  });
});

describe("paceBand", () => {
  it("classifies each tier at its boundaries", () => {
    expect(paceBand(0.4)).toBe("underuse");
    expect(paceBand(0.8)).toBe("sustainable");
    expect(paceBand(1.0)).toBe("sustainable");
    expect(paceBand(1.1)).toBe("overuse");
    expect(paceBand(1.2)).toBe("overuse");
    expect(paceBand(1.3)).toBe("heavy_overuse");
    expect(paceBand(3.0)).toBe("heavy_overuse");
  });
});

describe("isAtRisk", () => {
  it("is not at risk at a sustainable pace", () => {
    expect(isAtRisk(fiveHourWindow(50, 150), NOW)).toBe(false);
  });

  it("is at risk when burning faster than sustainable", () => {
    // Half the window gone, 80% used -> ratio 1.6.
    expect(isAtRisk(fiveHourWindow(80, 150), NOW)).toBe(true);
  });

  it("is not at risk in a barely-started window (below the usage floor)", () => {
    expect(isAtRisk(fiveHourWindow(1, 297), NOW)).toBe(false);
  });

  it("is not at risk once the window has already reset", () => {
    expect(isAtRisk(fiveHourWindow(90, -10), NOW)).toBe(false);
  });

  it("is not at risk when nothing has been used yet", () => {
    expect(isAtRisk(fiveHourWindow(0, 150), NOW)).toBe(false);
  });

  it("caps utilization at 100 (110% used late is capped to ratio 100/90)", () => {
    // 110% used at 90% elapsed: capped ratio 100/90 ≈ 1.11 (not 110/90), which
    // is over the 1.0 line -> at risk.
    expect(isAtRisk(fiveHourWindow(110, 30), NOW)).toBe(true);
  });
});
