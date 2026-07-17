import { describe, expect, it } from "vitest";

import { statusFromUtilization } from "./status";

describe("statusFromUtilization", () => {
  it("classifies thresholds exactly like meter_core::UsageStatus", () => {
    expect(statusFromUtilization(0)).toBe("safe");
    expect(statusFromUtilization(49.9)).toBe("safe");
    expect(statusFromUtilization(50)).toBe("warning");
    expect(statusFromUtilization(79.9)).toBe("warning");
    expect(statusFromUtilization(80)).toBe("critical");
    expect(statusFromUtilization(120)).toBe("critical");
  });
});
