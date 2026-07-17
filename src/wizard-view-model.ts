// Pure view-model helpers for the first-run setup wizard (issue #11): welcome
// → session (import or paste) → validate → pick icon style + interval →
// done. No DOM, no Tauri, fully unit-testable — mirrors the split
// `settings-view-model.ts` and `browser-import.ts` use. The DOM wiring lives
// in `wizard.ts`.

import type { AppSettings, IconStyle, RefreshInterval, WizardSessionResult } from "./types";

/** The wizard's steps, in the order the user walks through them. */
export type WizardStep = "welcome" | "session" | "validate" | "customize" | "done";

export const WIZARD_STEPS: readonly WizardStep[] = ["welcome", "session", "validate", "customize", "done"];

/** 1-based position of `step` among `WIZARD_STEPS`, for a "Step 2 of 5"
 * indicator. */
export function stepNumber(step: WizardStep): number {
  return WIZARD_STEPS.indexOf(step) + 1;
}

/** Human copy for the step indicator. */
export function stepIndicatorLabel(step: WizardStep): string {
  return `Step ${stepNumber(step)} of ${WIZARD_STEPS.length}`;
}

/** Human confirmation for the validate step's outcome. Mirrors
 * `browser-import.ts::describeImportSummary`: a key claude.ai has confirmed
 * reads differently from one stored but not yet verified (claude.ai was
 * unreachable — the scheduler verifies it on the next poll). */
export function describeWizardValidation(result: WizardSessionResult): string {
  return result.validated
    ? "Your session is connected and verified with claude.ai."
    : "Your session is saved. claude.ai couldn't be reached to confirm it just now — it will " +
        "be verified on the next refresh.";
}

/** What the customize step's icon-style / refresh-interval selects should
 * show when the wizard opens: the caller's *actual* current settings, not
 * the step's hard-coded HTML defaults (Battery / Every minute). Matters most
 * when the wizard is reopened via Settings' "Run setup again" rather than on
 * first run, where those defaults and the real settings coincide anyway. */
export function wizardCustomizeDefaults(
  settings: Pick<AppSettings, "icon_style" | "refresh_interval">,
): { iconStyle: IconStyle; refreshInterval: RefreshInterval } {
  return { iconStyle: settings.icon_style, refreshInterval: settings.refresh_interval };
}
