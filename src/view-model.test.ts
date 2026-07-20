import { describe, expect, it } from "vitest";

import { DEMO_STATE } from "./demo";
import type { MeterState, ScopedLimit, UsageWindow } from "./types";
import { buildViewModel } from "./view-model";

const NOW = new Date("2026-07-17T12:00:00Z");

/** Every scoped model in the DEMO_STATE fixture opted in — the behaviour
 * most tests below still assert, distinct from the dedicated opt-in tests. */
const ALL_SHOWN = new Set(["Fable", "Sonnet"]);

function window(utilization: number, resetsInSecs: number): UsageWindow {
  return {
    utilization,
    resets_at: new Date(NOW.getTime() + resetsInSecs * 1000).toISOString(),
    window: "seven_day",
  };
}

function scoped(displayName: string, isActive: boolean, modelId: string | null = null): ScopedLimit {
  return {
    display_name: displayName,
    model_id: modelId,
    usage: window(42, 3 * 86_400),
    is_active: isActive,
  };
}

function state(overrides: Partial<MeterState>): MeterState {
  return {
    snapshot: null,
    staleness: "missing",
    phase: "polling",
    ...overrides,
  };
}

describe("buildViewModel — cards", () => {
  it("renders the fixture snapshot: both headline cards plus every named scoped model", () => {
    const viewModel = buildViewModel(DEMO_STATE, NOW, ALL_SHOWN);
    expect(viewModel.cards.map((c) => c.id)).toEqual([
      "five_hour",
      "seven_day",
      "scoped:Fable",
      "scoped:Sonnet",
    ]);
    // The acceptance-criteria case: a scoped model with no `model_id` must
    // still render — the card view-model doesn't even carry model_id, so
    // its absence in the source data can't break anything downstream.
    const fable = viewModel.cards.find((c) => c.id === "scoped:Fable");
    expect(fable?.title).toBe("Fable");
    expect(fable?.percent).toBe(50);
  });

  it("omits absent headline windows", () => {
    const viewModel = buildViewModel(
      state({
        snapshot: { five_hour: window(10, 1000), seven_day: null, scoped: [], spend: null, fetched_at: NOW.toISOString() },
      }),
      NOW,
      ALL_SHOWN,
    );
    expect(viewModel.cards.map((c) => c.id)).toEqual(["five_hour"]);
  });

  it("only renders visible (is_active) scoped limits", () => {
    const viewModel = buildViewModel(
      state({
        snapshot: {
          five_hour: null,
          seven_day: null,
          scoped: [scoped("Sonnet", true), scoped("CodeOnly", false)],
          spend: null, fetched_at: NOW.toISOString(),
        },
      }),
      NOW,
      ALL_SHOWN,
    );
    expect(viewModel.cards.map((c) => c.id)).toEqual(["scoped:Sonnet"]);
  });

  it("hides every scoped model by default (empty shown set)", () => {
    // Both models are `is_active`, but an empty `shownScopedModels` — the
    // default until the user opts in via Settings — keeps them out.
    const viewModel = buildViewModel(DEMO_STATE, NOW, new Set());
    expect(viewModel.cards.map((c) => c.id)).toEqual(["five_hour", "seven_day"]);
  });

  it("shows only the scoped models the user switched on", () => {
    const viewModel = buildViewModel(DEMO_STATE, NOW, new Set(["Fable"]));
    expect(viewModel.cards.map((c) => c.id)).toEqual(["five_hour", "seven_day", "scoped:Fable"]);
  });

  it("requires both is_active and opt-in — either gate alone is not enough", () => {
    const viewModel = buildViewModel(
      state({
        snapshot: {
          five_hour: null,
          seven_day: null,
          scoped: [scoped("Sonnet", true), scoped("CodeOnly", false)],
          spend: null, fetched_at: NOW.toISOString(),
        },
      }),
      NOW,
      // Both models opted in, but "CodeOnly" is not `is_active`.
      new Set(["Sonnet", "CodeOnly"]),
    );
    expect(viewModel.cards.map((c) => c.id)).toEqual(["scoped:Sonnet"]);
  });

  it("produces no cards without a snapshot", () => {
    expect(buildViewModel(state({}), NOW, ALL_SHOWN).cards).toEqual([]);
  });

  it("flags pacing risk per card", () => {
    const hot = window(90, 4 * 86_400); // ratio well above 1.2
    const viewModel = buildViewModel(
      state({
        snapshot: { five_hour: null, seven_day: hot, scoped: [], spend: null, fetched_at: NOW.toISOString() },
      }),
      NOW,
      ALL_SHOWN,
    );
    expect(viewModel.cards[0]?.atRisk).toBe(true);
  });
});

const WINDOW_MS = { five_hour: 5 * 60 * 60 * 1000, seven_day: 7 * 24 * 60 * 60 * 1000 };

/** A window with `elapsed` fraction of its span gone and `utilization` used. */
function paced(
  utilization: number,
  elapsed: number,
  win: "five_hour" | "seven_day",
): UsageWindow {
  return {
    utilization,
    resets_at: new Date(NOW.getTime() + WINDOW_MS[win] * (1 - elapsed)).toISOString(),
    window: win,
  };
}

function pacedState(five: UsageWindow | null, seven: UsageWindow | null): MeterState {
  return state({
    snapshot: { five_hour: five, seven_day: seven, scoped: [], spend: null, fetched_at: NOW.toISOString() },
  });
}

describe("buildViewModel — pace", () => {
  it("carries the pace ratio, band and expected-by-now percent per card", () => {
    // 60% used at 50% elapsed -> ratio 1.2 (sustainable upper edge), expected 50%.
    const vm = buildViewModel(pacedState(paced(60, 0.5, "five_hour"), null), NOW, new Set());
    const card = vm.cards[0]!;
    expect(card.paceRatio).toBeCloseTo(1.2, 2);
    expect(card.paceBand).toBe("sustainable");
    expect(card.expectedPercent).toBeCloseTo(50, 2);
  });

  it("only signals underuse on the weekly card, never the session card", () => {
    // Both underusing (ratio 0.4); only the weekly card's projection may show it.
    const vm = buildViewModel(
      pacedState(paced(20, 0.5, "five_hour"), paced(20, 0.5, "seven_day")),
      NOW,
      new Set(),
    );
    const session = vm.cards.find((c) => c.id === "five_hour")!;
    const weekly = vm.cards.find((c) => c.id === "seven_day")!;
    expect(session.showsUnderuse).toBe(false);
    expect(weekly.showsUnderuse).toBe(true);
    // The session's projection never carries an "unused" figure.
    expect(session.projection).toMatchObject({ kind: "ends", unusedPercent: null });
    expect(weekly.projection).toMatchObject({ kind: "ends", unusedPercent: 60 });
  });

  it("swaps to pace-first only once a ratio exists to lead with", () => {
    // Fresh window (2% elapsed): ratio suppressed, so pace-first stays off.
    const fresh = buildViewModel(pacedState(paced(10, 0.02, "five_hour"), null), NOW, new Set(), { paceFirst: true });
    expect(fresh.cards[0]!.paceRatio).toBeNull();
    expect(fresh.cards[0]!.paceFirst).toBe(false);
    // Established window: pace-first engages.
    const live = buildViewModel(pacedState(paced(60, 0.5, "five_hour"), null), NOW, new Set(), { paceFirst: true });
    expect(live.cards[0]!.paceFirst).toBe(true);
  });

  it("projects a limit hit before reset when burning fast", () => {
    const vm = buildViewModel(pacedState(paced(60, 0.5, "five_hour"), null), NOW, new Set());
    expect(vm.cards[0]!.projection).toMatchObject({ kind: "hits" });
  });

  it("computes no pace at all when pace tracking is disabled (master switch)", () => {
    const vm = buildViewModel(pacedState(paced(60, 0.5, "five_hour"), null), NOW, new Set(), {
      paceFirst: true,
      paceTrackingEnabled: false,
    });
    const card = vm.cards[0]!;
    expect(card.paceRatio).toBeNull();
    expect(card.paceBand).toBeNull();
    expect(card.expectedPercent).toBeNull();
    expect(card.projection).toBeNull();
    expect(card.paceFirst).toBe(false);
  });

  it("treats a scoped five-hour limit as session-cadence, not weekly", () => {
    // A scoped limit whose own window is a five-hour kind must pace over its
    // full 5-hour window (like the headline session card), not the weekly
    // 5/6/7-day span — the cadence is keyed off window.window, not the
    // synthetic card id (response.rs maps `five_hour`-prefixed scoped kinds to
    // LimitWindow::FiveHour). Mis-paced as weekly, 50% of a 5-hour window is
    // ~1.5% of a 7-day span, so the ratio would balloon to overuse.
    const scopedFive: ScopedLimit = {
      display_name: "Sonnet",
      model_id: null,
      usage: paced(60, 0.5, "five_hour"),
      is_active: true,
    };
    const vm = buildViewModel(
      state({
        snapshot: { five_hour: null, seven_day: null, scoped: [scopedFive], spend: null, fetched_at: NOW.toISOString() },
      }),
      NOW,
      new Set(["Sonnet"]),
    );
    const card = vm.cards.find((c) => c.id === "scoped:Sonnet")!;
    // 60% used at 50% of the 5-hour window -> sustainable (1.2), expected 50%.
    expect(card.paceBand).toBe("sustainable");
    expect(card.expectedPercent).toBeCloseTo(50, 2);
    // Session cadence never signals underuse.
    expect(card.showsUnderuse).toBe(false);
  });

  it("applies the weekly pace basis to the weekly card", () => {
    // 40% at 2/7 days: 7-day basis is overuse (1.4), 5-day basis is on-pace (1.0).
    const seven = buildViewModel(pacedState(null, paced(40, 2 / 7, "seven_day")), NOW, new Set(), { weeklyPaceDays: 7 });
    const five = buildViewModel(pacedState(null, paced(40, 2 / 7, "seven_day")), NOW, new Set(), { weeklyPaceDays: 5 });
    expect(seven.cards[0]!.paceBand).toBe("overuse");
    expect(five.cards[0]!.paceBand).toBe("sustainable");
  });
});

describe("buildViewModel — banner and status line", () => {
  it("is 'loading' before the first snapshot arrives", () => {
    const viewModel = buildViewModel(state({ phase: "polling", staleness: "missing" }), NOW, ALL_SHOWN);
    expect(viewModel.bannerKind).toBe("loading");
    expect(viewModel.statusLine).toBe("Waiting for first update…");
    expect(viewModel.showSessionForm).toBe(false);
  });

  it("is 'ok' with no banner text change when fresh", () => {
    const viewModel = buildViewModel(DEMO_STATE, NOW, ALL_SHOWN);
    expect(viewModel.bannerKind).toBe("ok");
    expect(viewModel.statusLine).toBe("Updated under 1m ago");
  });

  it("is 'stale' with the cached snapshot's age when polling but stale", () => {
    const aged: MeterState = {
      ...DEMO_STATE,
      staleness: "stale",
      snapshot: { ...DEMO_STATE.snapshot!, fetched_at: new Date(NOW.getTime() - 25 * 60_000).toISOString() },
    };
    const viewModel = buildViewModel(aged, NOW, ALL_SHOWN);
    expect(viewModel.bannerKind).toBe("stale");
    expect(viewModel.statusLine).toBe("Stale — updated 25m ago");
  });

  it("shows the session-key CTA and its message when awaiting a session", () => {
    const viewModel = buildViewModel(state({ phase: "awaiting_session" }), NOW, ALL_SHOWN);
    expect(viewModel.bannerKind).toBe("awaiting_session");
    expect(viewModel.showSessionForm).toBe(true);
    expect(viewModel.statusLine).toBe("No session key — paste one below to get started");
  });

  it("surfaces cached data age alongside the session-expired CTA", () => {
    const aged: MeterState = { ...DEMO_STATE, phase: "session_expired" };
    const viewModel = buildViewModel(aged, NOW, ALL_SHOWN);
    expect(viewModel.bannerKind).toBe("session_expired");
    expect(viewModel.showSessionForm).toBe(true);
    expect(viewModel.statusLine).toBe("Session expired — showing data from under 1m ago");
  });

  it("reads as 'degraded' with cards still shown from the last good snapshot", () => {
    const degraded: MeterState = { ...DEMO_STATE, phase: "degraded" };
    const viewModel = buildViewModel(degraded, NOW, ALL_SHOWN);
    expect(viewModel.bannerKind).toBe("degraded");
    expect(viewModel.showSessionForm).toBe(false);
    expect(viewModel.cards.length).toBeGreaterThan(0);
    expect(viewModel.statusLine).toBe("Connection trouble — data from under 1m ago");
  });
});
