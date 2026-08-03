// DOM wiring for the first-run setup wizard (issue #11): welcome → source →
// then one of two branches (issue #71) → pick icon style + interval → done.
// Polling claude.ai goes consent → session (import via #10 or paste) →
// validate (spinner + friendly errors); reading from Claude Code goes to the
// status-line step instead, since it needs neither consent nor a credential.
//
// The consent step is a hard gate, not a notice: until its checkbox is ticked
// the Continue button stays disabled, and the backend refuses every claude.ai
// call anyway (see `src-tauri/src/consent.rs`). Kept separate from `main.ts` for the same
// reason the Settings panel's own render/logic split is: this is a whole
// self-contained flow with its own element ids, and factoring it out keeps
// `main.ts` from having to know its internals — `main.ts` only calls
// `createWizard(...)` once and wires two entry points (auto-open on first
// run, and the Settings panel's "Run setup again" button) into it.
//
// Reuses existing commands wherever a step is just an existing Settings
// screen driven from a different place: `listBrowserSessions` /
// `importBrowserSession` for the session step's import path (issue #10),
// `setIconStyle` / `setRefreshInterval` for the customize step (both already
// apply live and persist), and `submitSessionKey` for the "paste a key"
// validate step (the same validated, rollback-on-rejection command the
// popover and Settings fields use). Only the completion marker
// (`wizardComplete`) and first-run detection are wizard-specific — see
// `src-tauri/src/wizard.rs`.

import { openUrl } from "@tauri-apps/plugin-opener";

import { describeImportSummary } from "./browser-import";
import { renderBrowserList } from "./browser-import-render";
import { createIconStylePicker } from "./icon-style-picker";
import { describeError } from "./ipc";
import type { UsageBackend } from "./ipc";
import { renderSelectOptions } from "./settings-render";
import { createStatuslineSetup } from "./statusline-setup";
import {
  DOCS_URL,
  TOS_BODY,
  TOS_CONSENT_LABEL,
  TOS_CONSENT_REQUIRED_HINT,
  TOS_HEADLINE,
  TOS_MITIGATION,
} from "./tos-notice";
import { REFRESH_INTERVAL_OPTIONS } from "./types";
import type { Browser, IconStyle, RefreshInterval, UsageSource } from "./types";
import {
  TOS_DECLINE_ALTERNATIVE,
  USAGE_SOURCE_OPTIONS,
  usageSourceHint,
  wizardDoneSummary,
} from "./usage-source";
import {
  type WizardStep,
  describeWizardValidation,
  nextStep,
  previousStep,
  stepIndicatorLabel,
  wizardCustomizeDefaults,
} from "./wizard-view-model";
import { requireElement } from "./dom";

/** Callbacks so the wizard's "customize" step, which drives the very same
 * live-apply-and-persist commands the Settings panel does, keeps the
 * caller's own settings echo (and the Settings panel's form) in sync instead
 * of drifting until the panel is next reopened. */
export interface WizardCallbacks {
  onIconStyleChange(style: IconStyle): void;
  onRefreshIntervalChange(interval: RefreshInterval): void;
  /** The consent step moved the ToS acknowledgement, so the Settings panel's
   * own checkbox and paused/active hint stay in step with it. */
  onTosAcknowledgedChange(acknowledged: boolean): void;
  /** The source step decides which branch the wizard takes *and* which
   * sections Settings dims, so the panel has to hear about it too. */
  onUsageSourceChange(source: UsageSource): void;
  /** Called every time the wizard closes, finished or cancelled, so the
   * caller can refresh anything the wizard may have changed (session status,
   * browser list). */
  onClose(): void;
}

export interface Wizard {
  /** Open the wizard from the beginning. Used both for an automatic
   * first-run open and for the Settings panel's "Run setup again". */
  open(): void;
  /** Open the wizard automatically if `wizardShouldRun()` says this is a
   * first run (settings.json did not exist before this launch). */
  maybeAutoOpen(): void;
}

/** Wire up every element of the wizard panel and return the two entry
 * points `main.ts` needs. All wizard-internal state (current step, the
 * pasted-vs-imported result being shown on the validate step) lives in this
 * closure. */
export function createWizard(backend: UsageBackend, callbacks: WizardCallbacks): Wizard {
  const panel = requireElement<HTMLElement>("wizard-panel");
  const stepIndicator = requireElement<HTMLElement>("wizard-step-indicator");

  const steps: Record<WizardStep, HTMLElement> = {
    welcome: requireElement("wizard-step-welcome"),
    source: requireElement("wizard-step-source"),
    consent: requireElement("wizard-step-consent"),
    session: requireElement("wizard-step-session"),
    validate: requireElement("wizard-step-validate"),
    statusline: requireElement("wizard-step-statusline"),
    customize: requireElement("wizard-step-customize"),
    done: requireElement("wizard-step-done"),
  };

  /** Which branch the wizard is on. Held here rather than re-read from the
   * backend on every transition: the source step writes it the moment it
   * changes, and every path decision below reads this one value. */
  let source: UsageSource = "claude_ai";

  /** The step on screen. Tracked because the step *indicator* depends on the
   * source as well as the step — switching source changes how many steps
   * there are — so it has to be re-rendered when the source moves, for
   * whatever step happens to be showing. */
  let currentStep: WizardStep = "welcome";

  const skipButton = requireElement<HTMLButtonElement>("wizard-skip-button");
  const startButton = requireElement<HTMLButtonElement>("wizard-start-button");

  const usageSourceSelect = requireElement<HTMLSelectElement>("wizard-usage-source-select");
  const usageSourceHintEl = requireElement<HTMLElement>("wizard-usage-source-hint");
  const sourceBackButton = requireElement<HTMLButtonElement>("wizard-source-back-button");
  const sourceContinueButton = requireElement<HTMLButtonElement>("wizard-source-continue-button");

  const statuslineSetup = createStatuslineSetup(backend, "wizard-statusline-setup");
  const statuslineBackButton = requireElement<HTMLButtonElement>("wizard-statusline-back-button");
  const statuslineContinueButton = requireElement<HTMLButtonElement>(
    "wizard-statusline-continue-button",
  );

  const tosHeadline = requireElement<HTMLElement>("wizard-tos-headline");
  const tosBody = requireElement<HTMLElement>("wizard-tos-body");
  const tosMitigation = requireElement<HTMLElement>("wizard-tos-mitigation");
  const tosDocsLink = requireElement<HTMLButtonElement>("wizard-tos-docs-link");
  const tosConsent = requireElement<HTMLInputElement>("wizard-tos-consent");
  const tosConsentLabel = requireElement<HTMLElement>("wizard-tos-consent-label");
  const tosAlternative = requireElement<HTMLElement>("wizard-tos-alternative");
  const consentBackButton = requireElement<HTMLButtonElement>("wizard-consent-back-button");
  const consentContinueButton = requireElement<HTMLButtonElement>(
    "wizard-consent-continue-button",
  );
  const consentBlockedReason = requireElement<HTMLElement>("wizard-consent-blocked-reason");

  const browserImportList = requireElement<HTMLElement>("wizard-browser-import-list");
  const sessionForm = requireElement<HTMLFormElement>("wizard-session-form");
  const sessionInput = requireElement<HTMLInputElement>("wizard-session-input");
  const sessionError = requireElement<HTMLElement>("wizard-session-error");
  const sessionBackButton = requireElement<HTMLButtonElement>("wizard-session-back-button");
  const sessionCancelButton = requireElement<HTMLButtonElement>("wizard-session-cancel-button");

  const validateStatus = requireElement<HTMLElement>("wizard-validate-status");
  const validateError = requireElement<HTMLElement>("wizard-validate-error");
  const validateRetryButton = requireElement<HTMLButtonElement>("wizard-validate-retry-button");
  const validateContinueButton = requireElement<HTMLButtonElement>("wizard-validate-continue-button");

  const iconStyleContainer = requireElement<HTMLElement>("wizard-icon-style-picker");
  const refreshIntervalSelect = requireElement<HTMLSelectElement>("wizard-refresh-interval-select");
  const customizeContinueButton = requireElement<HTMLButtonElement>("wizard-customize-continue-button");

  const doneSummary = requireElement<HTMLElement>("wizard-done-summary");
  const gnomeHint = requireElement<HTMLElement>("wizard-gnome-hint");
  const finishButton = requireElement<HTMLButtonElement>("wizard-finish-button");

  renderSelectOptions(refreshIntervalSelect, REFRESH_INTERVAL_OPTIONS);
  renderSelectOptions(usageSourceSelect, USAGE_SOURCE_OPTIONS);

  // Copy comes from `tos-notice.ts` so the wizard and Settings state the same
  // risk in the same words — see that module's header.
  tosHeadline.textContent = TOS_HEADLINE;
  tosBody.textContent = TOS_BODY.join(" ");
  tosMitigation.textContent = TOS_MITIGATION;
  tosConsentLabel.textContent = TOS_CONSENT_LABEL;
  // The way out of this step for anyone unwilling to tick the box.
  tosAlternative.textContent = TOS_DECLINE_ALTERNATIVE;
  // …and, for anyone who is willing, what the inert Continue is waiting for.
  consentBlockedReason.textContent = TOS_CONSENT_REQUIRED_HINT;

  const iconStylePicker = createIconStylePicker(iconStyleContainer, "battery", (style) => {
    backend.setIconStyle(style).catch((error: unknown) => {
      console.error("failed to persist icon style", error);
    });
    callbacks.onIconStyleChange(style);
  });
  backend
    .iconStylePreviews()
    .then((previews) => iconStylePicker.setPreviews(previews))
    .catch((error: unknown) => {
      console.error("failed to load icon style previews", error);
    });

  function goToStep(step: WizardStep): void {
    currentStep = step;
    for (const [name, el] of Object.entries(steps)) {
      el.hidden = name !== step;
    }
    stepIndicator.textContent = stepIndicatorLabel(step, source);
    if (step === "session") {
      loadBrowsers();
    } else if (step === "statusline") {
      // Showing the block is what fetches this machine's command.
      statuslineSetup.setVisible(true);
    } else if (step === "done") {
      doneSummary.textContent = wizardDoneSummary(source);
      backend
        .linuxDesktop()
        .then((desktop) => {
          gnomeHint.hidden = desktop !== "gnome";
        })
        .catch((error: unknown) => {
          console.error("failed to detect desktop session", error);
        });
    }
  }

  /** Keep Continue in step with the checkbox, and persist the answer as it is
   * given rather than on Continue — so a user who ticks the box and closes the
   * wizard has still consented, and one who unticks it has still withdrawn.
   *
   * The checkbox is the only way past this step: there is no "skip" and no
   * "decide later", because every step after it involves contacting claude.ai.
   *
   * The reason Continue is inert moves with the flag (issue #78), as a visible
   * paragraph the button points `aria-describedby` at rather than a `title`: a
   * disabled button is not focusable, so anything hung off the button itself is
   * announced to nobody, and a tooltip is invisible to keyboard and touch. Both
   * the paragraph and the reference go the moment the box is ticked — a
   * description of a state the control is no longer in is worse than none. */
  function syncConsent(): void {
    const blocked = !tosConsent.checked;
    consentContinueButton.disabled = blocked;
    consentBlockedReason.hidden = !blocked;
    if (blocked) {
      // The element's own id, never a re-spelled literal, so the reference
      // cannot dangle — which is a failure ARIA reports to no one.
      consentContinueButton.setAttribute("aria-describedby", consentBlockedReason.id);
    } else {
      consentContinueButton.removeAttribute("aria-describedby");
    }
  }

  /** Move one step along the current source's path. Every Back/Continue goes
   * through these rather than naming its neighbour, so which step follows
   * which is `wizard-view-model.ts`'s business alone — and the two branches
   * cannot grow a transition the path does not have. */
  function advance(from: WizardStep): void {
    const to = nextStep(from, source);
    if (to) {
      goToStep(to);
    }
  }

  function back(from: WizardStep): void {
    const to = previousStep(from, source);
    if (to) {
      goToStep(to);
    }
  }

  /** Reflect the chosen source: the hint beside the picker, and — because the
   * choice decides how many steps there are — the indicator for whichever
   * step is on screen. `currentStep`, not `"source"`: this also runs from
   * `loadCurrentSettings` while the user is still on `welcome`, and hard-coding
   * the source step there would label the welcome screen "Step 2". */
  function syncSource(): void {
    usageSourceSelect.value = source;
    usageSourceHintEl.textContent = usageSourceHint(source);
    stepIndicator.textContent = stepIndicatorLabel(currentStep, source);
  }

  function loadBrowsers(): void {
    backend
      .listBrowserSessions()
      .then((browsers) => {
        renderBrowserList(browserImportList, browsers, handleBrowserImport, handleOpenSettingsPane);
      })
      .catch((error: unknown) => {
        console.error("failed to list importable browsers", error);
      });
  }

  function showValidating(): void {
    goToStep("validate");
    validateStatus.textContent = "Checking your session with claude.ai…";
    validateError.hidden = true;
    validateRetryButton.hidden = true;
    validateContinueButton.hidden = true;
  }

  function showValidateSuccess(message: string): void {
    validateStatus.textContent = message;
    validateContinueButton.hidden = false;
  }

  function showValidateFailure(message: string): void {
    validateStatus.textContent = "";
    validateError.textContent = message;
    validateError.hidden = false;
    validateRetryButton.hidden = false;
  }

  function handleBrowserImport(browser: Browser): void {
    showValidating();
    backend
      .importBrowserSession(browser)
      .then((summary) => showValidateSuccess(describeImportSummary(summary)))
      .catch((error: unknown) => showValidateFailure(describeError(error)));
  }

  function handleOpenSettingsPane(url: string): void {
    openUrl(url).catch((error: unknown) => {
      console.error("failed to open settings pane", error);
    });
  }

  sessionForm.addEventListener("submit", (event) => {
    event.preventDefault();
    const value = sessionInput.value.trim();
    if (!value) {
      return;
    }
    sessionError.hidden = true;
    showValidating();
    backend
      .submitSessionKey(value)
      .then((result) => {
        sessionInput.value = "";
        showValidateSuccess(describeWizardValidation(result));
      })
      .catch((error: unknown) => {
        // Land back on the session step with the message, rather than the
        // spinner step, so the user can immediately correct the input.
        goToStep("session");
        sessionError.textContent = describeError(error);
        sessionError.hidden = false;
      });
  });

  tosConsent.addEventListener("change", () => {
    syncConsent();
    backend.setTosAcknowledged(tosConsent.checked).catch((error: unknown) => {
      console.error("failed to persist ToS acknowledgement", error);
    });
    callbacks.onTosAcknowledgedChange(tosConsent.checked);
  });
  tosDocsLink.addEventListener("click", () => handleOpenSettingsPane(DOCS_URL));
  consentBackButton.addEventListener("click", () => back("consent"));
  consentContinueButton.addEventListener("click", () => advance("consent"));

  usageSourceSelect.addEventListener("change", () => {
    source = usageSourceSelect.value as UsageSource;
    syncSource();
    backend.setUsageSource(source).catch((error: unknown) => {
      console.error("failed to persist the usage source", error);
    });
    callbacks.onUsageSourceChange(source);
  });
  sourceBackButton.addEventListener("click", () => back("source"));
  sourceContinueButton.addEventListener("click", () => advance("source"));

  statuslineBackButton.addEventListener("click", () => back("statusline"));
  statuslineContinueButton.addEventListener("click", () => advance("statusline"));

  sessionBackButton.addEventListener("click", () => back("session"));
  sessionCancelButton.addEventListener("click", close);
  skipButton.addEventListener("click", close);
  startButton.addEventListener("click", () => advance("welcome"));

  // Retry re-enters the step that failed, which is not "the previous one" on
  // any path — the validate step is reached from `session` by submitting it.
  validateRetryButton.addEventListener("click", () => goToStep("session"));
  validateContinueButton.addEventListener("click", () => advance("validate"));

  refreshIntervalSelect.addEventListener("change", () => {
    const interval = refreshIntervalSelect.value as RefreshInterval;
    backend.setRefreshInterval(interval).catch((error: unknown) => {
      console.error("failed to persist refresh interval", error);
    });
    callbacks.onRefreshIntervalChange(interval);
  });

  customizeContinueButton.addEventListener("click", () => advance("customize"));

  finishButton.addEventListener("click", () => {
    backend
      .wizardComplete()
      .catch((error: unknown) => {
        // Best-effort: closing the wizard should never get stuck on this.
        console.error("failed to mark setup complete", error);
      })
      .finally(close);
  });

  function close(): void {
    panel.hidden = true;
    callbacks.onClose();
  }

  /** Preselect every control the wizard shares with Settings — icon style,
   * refresh interval, the consent box and the usage source — from the
   * caller's actual current settings, so reopening the wizard (Settings' "Run
   * setup again") shows the app's real configuration rather than the steps'
   * hard-coded HTML defaults (Battery / Every minute) — mirrors `main.ts`'s
   * `applySettingsToForm()`. Best-effort: if this fails the controls just keep
   * whatever they last showed. */
  function loadCurrentSettings(): void {
    backend
      .getSettings()
      .then((settings) => {
        const defaults = wizardCustomizeDefaults(settings);
        iconStylePicker.setSelected(defaults.iconStyle);
        refreshIntervalSelect.value = defaults.refreshInterval;
        // Reflect previously given answers, so "Run setup again" does not
        // present someone who already consented with an unticked box, or
        // offer a source they did not choose.
        tosConsent.checked = settings.tos_acknowledged;
        syncConsent();
        source = settings.usage_source;
        syncSource();
      })
      .catch((error: unknown) => {
        console.error("failed to load current settings for the wizard", error);
      });
  }

  function open(): void {
    panel.hidden = false;
    sessionInput.value = "";
    sessionError.hidden = true;
    // Closed until `loadCurrentSettings` says otherwise: the box must never
    // be pre-ticked on a first run.
    tosConsent.checked = false;
    syncConsent();
    // The path itself depends on the stored source, so assume the default
    // until `loadCurrentSettings` says otherwise — a first run has no stored
    // settings and claude.ai is what `AppSettings::default` picks.
    source = "claude_ai";
    // Before `syncSource`, which renders the indicator for whatever step is
    // showing: reopening the wizard must not label the welcome screen with
    // the step the *last* run happened to end on.
    goToStep("welcome");
    syncSource();
    loadCurrentSettings();
  }

  function maybeAutoOpen(): void {
    backend
      .wizardShouldRun()
      .then((shouldRun) => {
        if (shouldRun) {
          // Consume the first-run signal *before* showing the wizard: the
          // Settings window is destroyed on close and rebuilt (re-running this)
          // on the next open, so without recording "already offered" the wizard
          // would auto-open again on every reopen this session — even after the
          // user finished or skipped it. Best-effort; showing it once matters
          // more than the record, so a failure here never blocks the open.
          backend.wizardMarkOffered().catch((error: unknown) => {
            console.error("failed to record first-run wizard as offered", error);
          });
          open();
        }
      })
      .catch((error: unknown) => {
        console.error("failed to determine first-run status", error);
      });
  }

  return { open, maybeAutoOpen };
}
