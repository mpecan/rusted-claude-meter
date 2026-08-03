import { describe, expect, it } from "vitest";

import {
  describeWizardValidation,
  nextStep,
  previousStep,
  stepIndicatorLabel,
  stepNumber,
  wizardCustomizeDefaults,
  wizardSteps,
} from "./wizard-view-model";

describe("wizardSteps", () => {
  it("puts the source choice second on both paths, before anything it decides", () => {
    // The point of issue #71: the Terms-of-Service question applies to only
    // one of the answers, so the user meets the alternative while deciding
    // rather than after declining and going looking in Settings.
    for (const source of ["claude_ai", "claude_code_statusline"] as const) {
      expect(wizardSteps(source)[1]).toBe("source");
    }
  });

  it("keeps welcome first, so the choice is not the first thing on screen", () => {
    for (const source of ["claude_ai", "claude_code_statusline"] as const) {
      expect(wizardSteps(source)[0]).toBe("welcome");
    }
  });

  it("asks for consent before a credential on the claude.ai path", () => {
    // Load-bearing ordering, not cosmetic: the app must not ask for a
    // claude.ai credential before the user has accepted the risk of using it.
    const steps = wizardSteps("claude_ai");
    expect(steps.indexOf("consent")).toBeLessThan(steps.indexOf("session"));
    expect(steps.indexOf("source")).toBeLessThan(steps.indexOf("consent"));
  });

  it("asks the Claude Code path for neither consent nor a session key", () => {
    // Both would be a lie on this source: it originates no claude.ai request,
    // and the backend refuses a key with `WrongSource` anyway.
    const steps = wizardSteps("claude_code_statusline");
    expect(steps).not.toContain("consent");
    expect(steps).not.toContain("session");
    expect(steps).not.toContain("validate");
  });

  it("gives the Claude Code path its own setup step the other never has", () => {
    expect(wizardSteps("claude_code_statusline")).toContain("statusline");
    expect(wizardSteps("claude_ai")).not.toContain("statusline");
  });

  it("ends both paths on customize then done", () => {
    for (const source of ["claude_ai", "claude_code_statusline"] as const) {
      expect(wizardSteps(source).slice(-2)).toEqual(["customize", "done"]);
    }
  });
});

describe("stepNumber", () => {
  it("numbers every step of a path 1-based, in order", () => {
    for (const source of ["claude_ai", "claude_code_statusline"] as const) {
      const steps = wizardSteps(source);
      expect(steps.map((step) => stepNumber(step, source))).toEqual(
        steps.map((_, index) => index + 1),
      );
    }
  });

  it("reports 0 for a step this source never reaches", () => {
    expect(stepNumber("consent", "claude_code_statusline")).toBe(0);
  });
});

describe("stepIndicatorLabel", () => {
  it("counts against the path actually being walked, not every step that exists", () => {
    // The whole reason the count is per-source: promising 7 steps to someone
    // who will only ever see 5 is worse than not counting at all.
    expect(stepIndicatorLabel("welcome", "claude_ai")).toBe("Step 1 of 7");
    expect(stepIndicatorLabel("welcome", "claude_code_statusline")).toBe("Step 1 of 5");
  });

  it("describes the done step as the last one on either path", () => {
    expect(stepIndicatorLabel("done", "claude_ai")).toBe("Step 7 of 7");
    expect(stepIndicatorLabel("done", "claude_code_statusline")).toBe("Step 5 of 5");
  });
});

describe("nextStep / previousStep", () => {
  it("routes the source step to consent on claude.ai and to statusline on Claude Code", () => {
    // The branch itself, which is the change: one control, two paths.
    expect(nextStep("source", "claude_ai")).toBe("consent");
    expect(nextStep("source", "claude_code_statusline")).toBe("statusline");
  });

  it("walks a whole path forwards and back again", () => {
    for (const source of ["claude_ai", "claude_code_statusline"] as const) {
      const steps = wizardSteps(source);
      for (const [index, step] of steps.entries()) {
        expect(nextStep(step, source)).toBe(steps[index + 1]);
        expect(previousStep(step, source)).toBe(steps[index - 1]);
      }
    }
  });

  it("has nowhere to go past the ends", () => {
    expect(nextStep("done", "claude_ai")).toBeUndefined();
    expect(previousStep("welcome", "claude_ai")).toBeUndefined();
  });

  it("has nowhere to go from a step this source never reaches", () => {
    // Rather than silently landing the user on the other branch.
    expect(nextStep("consent", "claude_code_statusline")).toBeUndefined();
    expect(previousStep("consent", "claude_code_statusline")).toBeUndefined();
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
