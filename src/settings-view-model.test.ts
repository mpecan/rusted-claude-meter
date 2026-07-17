import { describe, expect, it } from "vitest";

import type { ScopedLimit, UsageSnapshot } from "./types";
import { DEFAULT_SETTINGS, scopedModelNames, toggleModel } from "./settings-view-model";

function limit(displayName: string): ScopedLimit {
  return {
    display_name: displayName,
    model_id: null,
    usage: { utilization: 10, resets_at: "2026-07-18T00:00:00Z", window: "seven_day" },
    is_active: true,
  };
}

function snapshot(scoped: ScopedLimit[]): UsageSnapshot {
  return {
    five_hour: null,
    seven_day: null,
    scoped,
    fetched_at: "2026-07-17T12:00:00Z",
  };
}

describe("DEFAULT_SETTINGS", () => {
  it("is opt-in and empty for scoped models, mirroring AppSettings::default", () => {
    expect(DEFAULT_SETTINGS.shown_scoped_models).toEqual([]);
  });
});

describe("scopedModelNames", () => {
  it("is empty without a snapshot", () => {
    expect(scopedModelNames(null)).toEqual([]);
  });

  it("lists every scoped model's display name in snapshot order", () => {
    expect(scopedModelNames(snapshot([limit("Sonnet"), limit("Fable")]))).toEqual([
      "Sonnet",
      "Fable",
    ]);
  });

  it("includes models regardless of is_active — a newly reported model must still appear", () => {
    const inactive = { ...limit("CodeOnly"), is_active: false };
    expect(scopedModelNames(snapshot([inactive]))).toEqual(["CodeOnly"]);
  });

  it("dedupes repeated display names", () => {
    expect(scopedModelNames(snapshot([limit("Fable"), limit("Fable")]))).toEqual(["Fable"]);
  });
});

describe("toggleModel", () => {
  it("adds a name that is not yet shown", () => {
    expect(toggleModel([], "Fable", true)).toEqual(["Fable"]);
    expect(toggleModel(["Sonnet"], "Fable", true)).toEqual(["Sonnet", "Fable"]);
  });

  it("removes a name that is shown", () => {
    expect(toggleModel(["Sonnet", "Fable"], "Sonnet", false)).toEqual(["Fable"]);
  });

  it("is idempotent: enabling an already-shown name changes nothing", () => {
    expect(toggleModel(["Fable"], "Fable", true)).toEqual(["Fable"]);
  });

  it("is idempotent: disabling an already-absent name changes nothing", () => {
    expect(toggleModel(["Fable"], "Sonnet", false)).toEqual(["Fable"]);
  });

  it("never mutates the input array", () => {
    const shown = ["Fable"];
    toggleModel(shown, "Sonnet", true);
    expect(shown).toEqual(["Fable"]);
  });
});
