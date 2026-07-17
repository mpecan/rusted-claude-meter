import { describe, expect, it } from "vitest";

import { DEMO_STATE } from "./demo";
import type { MeterState, ScopedLimit, UsageWindow } from "./types";
import { buildViewModel } from "./view-model";

const NOW = new Date("2026-07-17T12:00:00Z");

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
    const viewModel = buildViewModel(DEMO_STATE, NOW);
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
        snapshot: { five_hour: window(10, 1000), seven_day: null, scoped: [], fetched_at: NOW.toISOString() },
      }),
      NOW,
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
          fetched_at: NOW.toISOString(),
        },
      }),
      NOW,
    );
    expect(viewModel.cards.map((c) => c.id)).toEqual(["scoped:Sonnet"]);
  });

  it("produces no cards without a snapshot", () => {
    expect(buildViewModel(state({}), NOW).cards).toEqual([]);
  });

  it("flags pacing risk per card", () => {
    const hot = window(90, 4 * 86_400); // ratio well above 1.2
    const viewModel = buildViewModel(
      state({
        snapshot: { five_hour: null, seven_day: hot, scoped: [], fetched_at: NOW.toISOString() },
      }),
      NOW,
    );
    expect(viewModel.cards[0]?.atRisk).toBe(true);
  });
});

describe("buildViewModel — banner and status line", () => {
  it("is 'loading' before the first snapshot arrives", () => {
    const viewModel = buildViewModel(state({ phase: "polling", staleness: "missing" }), NOW);
    expect(viewModel.bannerKind).toBe("loading");
    expect(viewModel.statusLine).toBe("Waiting for first update…");
    expect(viewModel.showSessionForm).toBe(false);
  });

  it("is 'ok' with no banner text change when fresh", () => {
    const viewModel = buildViewModel(DEMO_STATE, NOW);
    expect(viewModel.bannerKind).toBe("ok");
    expect(viewModel.statusLine).toBe("Updated under 1m ago");
  });

  it("is 'stale' with the cached snapshot's age when polling but stale", () => {
    const aged: MeterState = {
      ...DEMO_STATE,
      staleness: "stale",
      snapshot: { ...DEMO_STATE.snapshot!, fetched_at: new Date(NOW.getTime() - 25 * 60_000).toISOString() },
    };
    const viewModel = buildViewModel(aged, NOW);
    expect(viewModel.bannerKind).toBe("stale");
    expect(viewModel.statusLine).toBe("Stale — updated 25m ago");
  });

  it("shows the session-key CTA and its message when awaiting a session", () => {
    const viewModel = buildViewModel(state({ phase: "awaiting_session" }), NOW);
    expect(viewModel.bannerKind).toBe("awaiting_session");
    expect(viewModel.showSessionForm).toBe(true);
    expect(viewModel.statusLine).toBe("No session key — paste one below to get started");
  });

  it("surfaces cached data age alongside the session-expired CTA", () => {
    const aged: MeterState = { ...DEMO_STATE, phase: "session_expired" };
    const viewModel = buildViewModel(aged, NOW);
    expect(viewModel.bannerKind).toBe("session_expired");
    expect(viewModel.showSessionForm).toBe(true);
    expect(viewModel.statusLine).toBe("Session expired — showing data from under 1m ago");
  });

  it("reads as 'degraded' with cards still shown from the last good snapshot", () => {
    const degraded: MeterState = { ...DEMO_STATE, phase: "degraded" };
    const viewModel = buildViewModel(degraded, NOW);
    expect(viewModel.bannerKind).toBe("degraded");
    expect(viewModel.showSessionForm).toBe(false);
    expect(viewModel.cards.length).toBeGreaterThan(0);
    expect(viewModel.statusLine).toBe("Connection trouble — data from under 1m ago");
  });
});
