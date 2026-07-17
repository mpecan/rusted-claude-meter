import { describe, expect, it } from "vitest";

import { describeImportSummary } from "./browser-import";

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
