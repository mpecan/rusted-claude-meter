import { describe, expect, it } from "vitest";

import {
  WIZARD_STEPS,
  describeWizardValidation,
  stepIndicatorLabel,
  stepNumber,
  wizardCustomizeDefaults,
} from "./wizard-view-model";

describe("stepNumber", () => {
  it("numbers every step 1-based, in order", () => {
    expect(WIZARD_STEPS.map(stepNumber)).toEqual([1, 2, 3, 4, 5, 6]);
  });
});

describe("stepIndicatorLabel", () => {
  it("describes the welcome step as step 1", () => {
    expect(stepIndicatorLabel("welcome")).toBe("Step 1 of 6");
  });

  it("describes the done step as the last step", () => {
    expect(stepIndicatorLabel("done")).toBe("Step 6 of 6");
  });
});

describe("the consent step's position", () => {
  it("comes before the session step", () => {
    // Load-bearing ordering, not cosmetic: the app must not ask for a
    // claude.ai credential before the user has accepted the risk of using it.
    expect(WIZARD_STEPS.indexOf("consent")).toBeLessThan(WIZARD_STEPS.indexOf("session"));
  });

  it("comes after the welcome step, so the warning is not the first thing on screen", () => {
    expect(WIZARD_STEPS.indexOf("consent")).toBeGreaterThan(WIZARD_STEPS.indexOf("welcome"));
  });
});

describe("describeWizardValidation", () => {
  it("confirms a validated session", () => {
    expect(describeWizardValidation({ validated: true })).toContain("verified with claude.ai");
  });

  it("flags an unverified session as pending the next refresh", () => {
    const message = describeWizardValidation({ validated: false });
    expect(message).toContain("saved");
    expect(message).toContain("next refresh");
  });
});

describe("wizardCustomizeDefaults", () => {
  it("echoes the caller's actual current settings, not hard-coded defaults", () => {
    expect(
      wizardCustomizeDefaults({ icon_style: "gauge", refresh_interval: "ten_minutes" }),
    ).toEqual({ iconStyle: "gauge", refreshInterval: "ten_minutes" });
  });

  it("reflects a different combination too, so it isn't just passing one fixed value through", () => {
    expect(
      wizardCustomizeDefaults({ icon_style: "battery", refresh_interval: "one_minute" }),
    ).toEqual({ iconStyle: "battery", refreshInterval: "one_minute" });
  });
});
