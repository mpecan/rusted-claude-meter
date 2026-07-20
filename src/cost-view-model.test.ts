import { describe, expect, it } from "vitest";

import { buildCostViewModel, effectiveUsageMode, hasLimits, suggestedMode } from "./cost-view-model";
import { DEMO_COST_STATE, DEMO_SPEND, DEMO_STATE } from "./demo";
import type { Money, Spend, UsageSnapshot } from "./types";

const FETCHED = "2026-07-17T12:00:00Z";

const money = (minor: number, currency = "USD", exponent = 2): Money => ({
  minor,
  currency,
  exponent,
});

function snapshot(overrides: Partial<UsageSnapshot>): UsageSnapshot {
  return {
    five_hour: null,
    seven_day: null,
    scoped: [],
    spend: null,
    fetched_at: FETCHED,
    ...overrides,
  };
}

function spend(overrides: Partial<Spend>): Spend {
  return {
    used: null,
    limit: null,
    cap: null,
    enabled: false,
    ...overrides,
  };
}

describe("hasLimits", () => {
  it("is false without a snapshot", () => {
    expect(hasLimits(null)).toBe(false);
  });

  it("is false when no headline or scoped limit is present", () => {
    expect(hasLimits(snapshot({}))).toBe(false);
  });

  it("counts a headline window", () => {
    expect(
      hasLimits(snapshot({ five_hour: { utilization: 10, resets_at: FETCHED, window: "five_hour" } })),
    ).toBe(true);
  });

  it("counts a lone scoped limit", () => {
    expect(
      hasLimits(
        snapshot({
          scoped: [
            {
              display_name: "Sonnet",
              model_id: null,
              usage: { utilization: 10, resets_at: FETCHED, window: "seven_day" },
              is_active: true,
            },
          ],
        }),
      ),
    ).toBe(true);
  });
});

describe("suggestedMode / effectiveUsageMode", () => {
  it("suggests allowance without a snapshot", () => {
    expect(suggestedMode(null)).toBe("allowance");
    expect(effectiveUsageMode("auto", null)).toBe("allowance");
  });

  it("suggests allowance when limits are present (even alongside spend)", () => {
    const withBoth = snapshot({
      five_hour: { utilization: 10, resets_at: FETCHED, window: "five_hour" },
      spend: spend({ used: money(1000) }),
    });
    expect(suggestedMode(withBoth)).toBe("allowance");
    expect(effectiveUsageMode("auto", withBoth)).toBe("allowance");
  });

  it("suggests cost when there are no limits but spend is present", () => {
    const costOnly = snapshot({ spend: spend({ used: money(1000) }) });
    expect(suggestedMode(costOnly)).toBe("cost");
    expect(effectiveUsageMode("auto", costOnly)).toBe("cost");
  });

  it("stays on allowance when there are neither limits nor spend", () => {
    expect(suggestedMode(snapshot({}))).toBe("allowance");
    expect(effectiveUsageMode("auto", snapshot({}))).toBe("allowance");
  });

  it("returns a pinned mode unchanged regardless of the snapshot", () => {
    const costOnly = snapshot({ spend: spend({ used: money(1000) }) });
    expect(effectiveUsageMode("allowance", costOnly)).toBe("allowance");
    // Cost pinned even when the account reports no spend at all.
    expect(effectiveUsageMode("cost", snapshot({}))).toBe("cost");
  });

  it("agrees with the demo states auto-detection", () => {
    expect(effectiveUsageMode("auto", DEMO_STATE.snapshot)).toBe("allowance");
    expect(effectiveUsageMode("auto", DEMO_COST_STATE.snapshot)).toBe("cost");
  });
});

describe("buildCostViewModel", () => {
  it("maps the real spend shape (used, budget gauge, currency)", () => {
    // DEMO_SPEND: €125.00 of a €2000.00 budget = 6.25% -> 6%, safe. The cap
    // equals the limit, so it is not repeated.
    const vm = buildCostViewModel(DEMO_SPEND);
    expect(vm.used).toBe("€125.00");
    expect(vm.gauge).toEqual({ percent: 6, status: "safe", budget: "€2000.00" });
    expect(vm.cap).toBeNull();
  });

  it("has no gauge without a budget", () => {
    const vm = buildCostViewModel(spend({ used: money(2500) }));
    expect(vm.used).toBe("$25.00");
    expect(vm.gauge).toBeNull();
  });

  it("gauges against the cap when there is no limit", () => {
    const vm = buildCostViewModel(spend({ used: money(50_000), cap: money(200_000) }));
    expect(vm.gauge).toEqual({ percent: 25, status: "safe", budget: "$2000.00" });
    // The cap is the gauge budget here, so it is not repeated as a separate line.
    expect(vm.cap).toBeNull();
  });

  it("surfaces the hard cap only when it differs from the limit", () => {
    const equal = buildCostViewModel(
      spend({ used: money(2500), limit: money(50_000), cap: money(50_000) }),
    );
    expect(equal.cap).toBeNull();
    const distinct = buildCostViewModel(
      spend({ used: money(2500), limit: money(50_000), cap: money(80_000) }),
    );
    expect(distinct.cap).toBe("$800.00");
  });

  it("has no gauge for a non-positive budget (avoids divide-by-zero)", () => {
    expect(buildCostViewModel(spend({ used: money(2500), limit: money(0) })).gauge).toBeNull();
  });

  it("escalates the gauge status against the thresholds and clamps overspend", () => {
    // 60000 / 50000 = 120% -> clamped to 100 for the bar, critical status.
    const vm = buildCostViewModel(spend({ used: money(60_000), limit: money(50_000) }));
    expect(vm.gauge).toEqual({ percent: 100, status: "critical", budget: "$500.00" });
  });

  it("honours custom warning/critical thresholds for the gauge", () => {
    // 45% used: warning at 40 makes it "warning".
    const vm = buildCostViewModel(spend({ used: money(22_500), limit: money(50_000) }), 40, 80);
    expect(vm.gauge?.status).toBe("warning");
  });

  it("maps an all-null (unsurfaced stub) spend to an empty view-model", () => {
    const vm = buildCostViewModel(spend({}));
    expect(vm.used).toBeNull();
    expect(vm.gauge).toBeNull();
    expect(vm.cap).toBeNull();
  });
});
