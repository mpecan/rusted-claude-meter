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
    expect(WIZARD_STEPS.map(stepNumber)).toEqual([1, 2, 3, 4, 5]);
  });
});

describe("stepIndicatorLabel", () => {
  it("describes the welcome step as step 1", () => {
    expect(stepIndicatorLabel("welcome")).toBe("Step 1 of 5");
  });

  it("describes the done step as the last step", () => {
    expect(stepIndicatorLabel("done")).toBe("Step 5 of 5");
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
