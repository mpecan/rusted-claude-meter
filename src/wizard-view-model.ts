// Pure view-model helpers for the first-run setup wizard (issue #11): welcome
// → source → (claude.ai: consent → session → validate | Claude Code:
// statusline) → pick icon style + interval → done. No DOM, no Tauri, fully
// unit-testable — mirrors the split `settings-view-model.ts` and
// `browser-import.ts` use. The DOM wiring lives in `wizard.ts`.

import type {
  AppSettings,
  IconStyle,
  RefreshInterval,
  SessionSubmission,
  UsageSource,
} from "./types";
import { tosAppliesTo } from "./usage-source";

export type WizardStep =
  | "welcome"
  | "source"
  | "consent"
  | "session"
  | "validate"
  | "statusline"
  | "customize"
  | "done";

/** Polling claude.ai: the risk has to be accepted, then a credential given,
 * then verified.
 *
 * `consent` sits before `session` deliberately — asking for the credential is
 * the first step toward using it, so the risk is accepted first. */
const CLAUDE_AI_STEPS: readonly WizardStep[] = [
  "welcome",
  "source",
  "consent",
  "session",
  "validate",
  "customize",
  "done",
];

/** Reading from Claude Code: no consent, no credential, one thing to set up.
 *
 * Not a shortened claude.ai path with steps hidden — `statusline` is work the
 * other source never has, and the two branches genuinely differ. */
const STATUSLINE_STEPS: readonly WizardStep[] = [
  "welcome",
  "source",
  "statusline",
  "customize",
  "done",
];

/** The steps this source actually walks through (issue #71).
 *
 * `source` comes second, before `consent`, because the Terms-of-Service
 * question only applies to one of the answers: a user who would rather not
 * take that risk should meet the alternative *while* deciding, not after
 * declining and going looking in Settings. It comes after `welcome` so the
 * first thing on screen is still an introduction rather than a choice with no
 * context.
 *
 * Which branch follows is [`tosAppliesTo`], not a second reading of the same
 * fact: `consent` and `session` exist precisely because a source reaches
 * claude.ai, and that predicate is what says so. */
export function wizardSteps(source: UsageSource): readonly WizardStep[] {
  return tosAppliesTo(source) ? CLAUDE_AI_STEPS : STATUSLINE_STEPS;
}

/** 1-based position of `step` on this source's path, for a "Step 2 of 5"
 * indicator. `0` for a step this source never reaches. */
export function stepNumber(step: WizardStep, source: UsageSource): number {
  return wizardSteps(source).indexOf(step) + 1;
}

/** Human copy for the step indicator.
 *
 * Counted against the path the user is actually on, so the Claude Code branch
 * says "of 5" rather than promising steps it will never show. */
export function stepIndicatorLabel(step: WizardStep, source: UsageSource): string {
  const steps = wizardSteps(source);
  return `Step ${stepNumber(step, source)} of ${steps.length}`;
}

/** The step after `step` on this source's path, or `undefined` at the end (or
 * for a step this source never reaches).
 *
 * The wizard's Back/Continue buttons go through this rather than naming their
 * neighbours, so the order lives in one place and a branch cannot grow a
 * transition the path does not have. */
export function nextStep(step: WizardStep, source: UsageSource): WizardStep | undefined {
  const steps = wizardSteps(source);
  const index = steps.indexOf(step);
  return index < 0 ? undefined : steps[index + 1];
}

/** The step before `step` on this source's path; see [`nextStep`]. */
export function previousStep(step: WizardStep, source: UsageSource): WizardStep | undefined {
  const steps = wizardSteps(source);
  const index = steps.indexOf(step);
  return index <= 0 ? undefined : steps[index - 1];
}

/** The three DOM flags the consent step's Continue and its reason paragraph
 * carry, all of which move together (issue #78). */
export interface ConsentGateState {
  /** `disabled` on Continue. */
  readonly continueDisabled: boolean;
  /** `hidden` on the paragraph saying *why* Continue is inert. */
  readonly reasonHidden: boolean;
  /** Whether Continue should point `aria-describedby` at that paragraph. */
  readonly reasonDescribesContinue: boolean;
}

/** What the consent step should look like for a given state of the checkbox.
 *
 * A pure function of one boolean rather than three assignments inline in
 * `wizard.ts::syncConsent`, because the interesting part is that the three
 * never disagree: a visible reason under an enabled Continue contradicts
 * itself on screen, and an `aria-describedby` left pointing at it announces
 * "Continue stays unavailable…" for a button that is now pressable — worse
 * than no description at all. Fields are named for the attributes they become
 * and carry the attributes' own polarity (`hidden`, not "visible"), since
 * flipping one of those on the way to the DOM is exactly the mistake this
 * exists to make hard.
 *
 * The paragraph's id stays in `wizard.ts` — the element knows its own id, and a
 * literal re-spelled here could dangle silently. */
export function consentGateState(consentChecked: boolean): ConsentGateState {
  const blocked = !consentChecked;
  return {
    continueDisabled: blocked,
    reasonHidden: !blocked,
    reasonDescribesContinue: blocked,
  };
}

/** Human confirmation for the validate step's outcome. Mirrors
 * `browser-import.ts::describeImportSummary`: a key claude.ai has confirmed
 * reads differently from one stored but not yet verified (claude.ai was
 * unreachable — the scheduler verifies it on the next poll). */
export function describeWizardValidation(result: SessionSubmission): string {
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
