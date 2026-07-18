import { describe, expect, it } from "vitest";

import { statusFromUtilization, statusLabel } from "./status";

describe("statusFromUtilization", () => {
  it("classifies against the given thresholds", () => {
    // warning=75, critical=90.
    expect(statusFromUtilization(0, 75, 90)).toBe("safe");
    expect(statusFromUtilization(74.9, 75, 90)).toBe("safe");
    expect(statusFromUtilization(75, 75, 90)).toBe("warning");
    expect(statusFromUtilization(89.9, 75, 90)).toBe("warning");
    expect(statusFromUtilization(90, 75, 90)).toBe("critical");
    expect(statusFromUtilization(120, 75, 90)).toBe("critical");
  });

  it("follows the user's own thresholds, not fixed bands", () => {
    // A tighter warning threshold escalates sooner.
    expect(statusFromUtilization(55, 50, 80)).toBe("warning");
    expect(statusFromUtilization(85, 50, 80)).toBe("critical");
  });

  it("defaults to 75/90 when no thresholds are given", () => {
    expect(statusFromUtilization(74)).toBe("safe");
    expect(statusFromUtilization(75)).toBe("warning");
    expect(statusFromUtilization(90)).toBe("critical");
  });
});

describe("statusLabel", () => {
  it("maps each status to its pill label", () => {
    expect(statusLabel("safe")).toBe("OK");
    expect(statusLabel("warning")).toBe("HIGH");
    expect(statusLabel("critical")).toBe("CRITICAL");
  });
});
