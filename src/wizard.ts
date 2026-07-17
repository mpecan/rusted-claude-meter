// DOM wiring for the first-run setup wizard (issue #11): welcome → session
// (import via #10 or paste) → validate (spinner + friendly errors) → pick
// icon style + interval → done. Kept separate from `main.ts` for the same
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
// apply live and persist). Only the "paste a key" validate step
// (`wizardSubmitSessionKey`) and completion marker (`wizardComplete`) are
// wizard-specific — see `src-tauri/src/wizard.rs`.

import { openUrl } from "@tauri-apps/plugin-opener";

import { describeImportSummary } from "./browser-import";
import { renderBrowserList } from "./browser-import-render";
import { describeError } from "./ipc";
import type { UsageBackend } from "./ipc";
import { renderSelectOptions } from "./settings-render";
import { ICON_STYLE_OPTIONS, REFRESH_INTERVAL_OPTIONS } from "./types";
import type { Browser, IconStyle, RefreshInterval } from "./types";
import {
  type WizardStep,
  describeWizardValidation,
  stepIndicatorLabel,
  wizardCustomizeDefaults,
} from "./wizard-view-model";

/** Callbacks so the wizard's "customize" step, which drives the very same
 * live-apply-and-persist commands the Settings panel does, keeps the
 * caller's own settings echo (and the Settings panel's form) in sync instead
 * of drifting until the panel is next reopened. */
export interface WizardCallbacks {
  onIconStyleChange(style: IconStyle): void;
  onRefreshIntervalChange(interval: RefreshInterval): void;
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

function requireElement<T extends HTMLElement>(id: string): T {
  const el = document.getElementById(id);
  if (!el) {
    throw new Error(`missing #${id} in index.html`);
  }
  return el as T;
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
    session: requireElement("wizard-step-session"),
    validate: requireElement("wizard-step-validate"),
    customize: requireElement("wizard-step-customize"),
    done: requireElement("wizard-step-done"),
  };

  const skipButton = requireElement<HTMLButtonElement>("wizard-skip-button");
  const startButton = requireElement<HTMLButtonElement>("wizard-start-button");

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

  const iconStyleSelect = requireElement<HTMLSelectElement>("wizard-icon-style-select");
  const refreshIntervalSelect = requireElement<HTMLSelectElement>("wizard-refresh-interval-select");
  const customizeContinueButton = requireElement<HTMLButtonElement>("wizard-customize-continue-button");

  const gnomeHint = requireElement<HTMLElement>("wizard-gnome-hint");
  const finishButton = requireElement<HTMLButtonElement>("wizard-finish-button");

  renderSelectOptions(iconStyleSelect, ICON_STYLE_OPTIONS);
  renderSelectOptions(refreshIntervalSelect, REFRESH_INTERVAL_OPTIONS);

  function goToStep(step: WizardStep): void {
    for (const [name, el] of Object.entries(steps)) {
      el.hidden = name !== step;
    }
    stepIndicator.textContent = stepIndicatorLabel(step);
    if (step === "session") {
      loadBrowsers();
    } else if (step === "done") {
      backend
        .isGnomeDesktop()
        .then((isGnome) => {
          gnomeHint.hidden = !isGnome;
        })
        .catch((error: unknown) => {
          console.error("failed to detect desktop session", error);
        });
    }
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
      .wizardSubmitSessionKey(value)
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

  sessionBackButton.addEventListener("click", () => goToStep("welcome"));
  sessionCancelButton.addEventListener("click", close);
  skipButton.addEventListener("click", close);
  startButton.addEventListener("click", () => goToStep("session"));

  validateRetryButton.addEventListener("click", () => goToStep("session"));
  validateContinueButton.addEventListener("click", () => goToStep("customize"));

  iconStyleSelect.addEventListener("change", () => {
    const style = iconStyleSelect.value as IconStyle;
    backend.setIconStyle(style).catch((error: unknown) => {
      console.error("failed to persist icon style", error);
    });
    callbacks.onIconStyleChange(style);
  });

  refreshIntervalSelect.addEventListener("change", () => {
    const interval = refreshIntervalSelect.value as RefreshInterval;
    backend.setRefreshInterval(interval).catch((error: unknown) => {
      console.error("failed to persist refresh interval", error);
    });
    callbacks.onRefreshIntervalChange(interval);
  });

  customizeContinueButton.addEventListener("click", () => goToStep("done"));

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

  /** Preselect the customize step's icon-style / refresh-interval selects
   * from the caller's actual current settings, so reopening the wizard
   * (Settings' "Run setup again") shows the app's real configuration rather
   * than the customize step's hard-coded HTML defaults (Battery / Every
   * minute) — mirrors `main.ts`'s `applySettingsToForm()`. Best-effort: if
   * this fails the selects just keep whatever they last showed. */
  function loadCustomizeDefaults(): void {
    backend
      .getSettings()
      .then((settings) => {
        const defaults = wizardCustomizeDefaults(settings);
        iconStyleSelect.value = defaults.iconStyle;
        refreshIntervalSelect.value = defaults.refreshInterval;
      })
      .catch((error: unknown) => {
        console.error("failed to load current settings for the customize step", error);
      });
  }

  function open(): void {
    panel.hidden = false;
    sessionInput.value = "";
    sessionError.hidden = true;
    loadCustomizeDefaults();
    goToStep("welcome");
  }

  function maybeAutoOpen(): void {
    backend
      .wizardShouldRun()
      .then((shouldRun) => {
        if (shouldRun) {
          open();
        }
      })
      .catch((error: unknown) => {
        console.error("failed to determine first-run status", error);
      });
  }

  return { open, maybeAutoOpen };
}
