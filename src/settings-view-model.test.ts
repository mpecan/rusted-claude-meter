import { describe, expect, it } from "vitest";

import type { IconPreview, IconStyle, ScopedLimit, UsageSnapshot } from "./types";
import {
  DEFAULT_SETTINGS,
  scopedModelNames,
  squareTrayFill,
  squareTrayHint,
  squareTrayStyles,
  toggleModel,
} from "./settings-view-model";

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
    spend: null,
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

describe("square tray hint (Plasma's square cell)", () => {
  // Real dimensions from `IconStyle::logical_size` — the wide styles bake in a
  // percentage number, the glyph-only ones stay near-square.
  const preview = (style: IconStyle, width: number): IconPreview => ({
    style,
    width,
    height: 22,
    rgba: [],
  });
  const previews: IconPreview[] = [
    preview("battery", 66),
    preview("circular", 26),
    preview("minimal", 44),
    preview("segments", 34),
    preview("dual_bar", 70),
    preview("gauge", 26),
  ];

  it("measures the fraction of panel height a style actually draws at", () => {
    // Battery is 3:1, so a square cell renders it at a third of panel height —
    // the number behind the KDE note in CLAUDE.md.
    expect(squareTrayFill(preview("battery", 66))).toBeCloseTo(1 / 3, 5);
    expect(squareTrayFill(preview("circular", 26))).toBeCloseTo(22 / 26, 5);
  });

  it("treats an icon at least as tall as it is wide as filling the cell", () => {
    expect(squareTrayFill({ style: "gauge", width: 22, height: 22, rgba: [] })).toBe(1);
    expect(squareTrayFill({ style: "gauge", width: 16, height: 22, rgba: [] })).toBe(1);
  });

  it("scores a degenerate preview as unusable rather than dividing by zero", () => {
    expect(squareTrayFill({ style: "gauge", width: 0, height: 0, rgba: [] })).toBe(0);
  });

  it("recommends only the near-square styles", () => {
    expect(squareTrayStyles(previews)).toEqual(["circular", "gauge"]);
  });

  it("nudges a Plasma user off a wide style", () => {
    const hint = squareTrayHint("kde", "battery", previews);
    expect(hint?.alternatives).toEqual(["circular", "gauge"]);
    // The message quotes this, so it has to be the real figure, not "a third"
    // hardcoded for whichever style prompted the hint first.
    expect(hint?.fill).toBeCloseTo(1 / 3, 5);
    expect(squareTrayHint("kde", "minimal", previews)?.fill).toBeCloseTo(0.5, 5);
  });

  it("says nothing once the user is already on a squarer style", () => {
    expect(squareTrayHint("kde", "circular", previews)).toBeNull();
    expect(squareTrayHint("kde", "gauge", previews)).toBeNull();
  });

  it("says nothing on desktops where the constraint does not apply", () => {
    expect(squareTrayHint("gnome", "battery", previews)).toBeNull();
    expect(squareTrayHint("other", "battery", previews)).toBeNull();
  });

  it("says nothing when the previews do not include the current style", () => {
    // Previews are rendered per style and a failed render is omitted, so the
    // current style can genuinely be missing; that is not a reason to nag.
    expect(squareTrayHint("kde", "battery", [preview("circular", 26)])).toBeNull();
  });

  it("says nothing when there is no better style to offer", () => {
    expect(squareTrayHint("kde", "battery", [preview("battery", 66)])).toBeNull();
  });
});
