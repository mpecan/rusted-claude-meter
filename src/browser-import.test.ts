import { describe, expect, it } from "vitest";

import { describeImportSummary, partitionImportTargets } from "./browser-import";
import type { Browser, DetectedBrowser } from "./types";

function detected(id: Browser): DetectedBrowser {
  return { id, name: id, family: "chromium", permission_hint: null, settings_deep_link: null };
}

describe("describeImportSummary", () => {
  it("confirms a validated import", () => {
    expect(describeImportSummary({ browser: "Google Chrome", validated: true })).toBe(
      "Imported and verified the session from Google Chrome.",
    );
  });

  it("flags an unverified import as pending the next refresh", () => {
    const message = describeImportSummary({ browser: "Firefox", validated: false });
    expect(message).toContain("Firefox");
    expect(message).toContain("verified on the next refresh");
  });
});

describe("partitionImportTargets", () => {
  it("promotes the most-used browsers in popularity order and collapses the rest", () => {
    // Given in backend detection order (Chrome, Brave, Arc, Firefox, Safari).
    const { primary, more } = partitionImportTargets([
      detected("chrome"),
      detected("brave"),
      detected("arc"),
      detected("firefox"),
      detected("safari"),
    ]);
    // Primary follows PRIMARY_IMPORT_BROWSERS order (chrome, safari, firefox, edge).
    expect(primary.map((b) => b.id)).toEqual(["chrome", "safari", "firefox"]);
    // The tail keeps detection order.
    expect(more.map((b) => b.id)).toEqual(["brave", "arc"]);
  });

  it("only includes detected browsers (no phantom primaries)", () => {
    const { primary, more } = partitionImportTargets([detected("arc"), detected("brave")]);
    expect(primary).toEqual([]);
    expect(more.map((b) => b.id)).toEqual(["arc", "brave"]);
  });
});
