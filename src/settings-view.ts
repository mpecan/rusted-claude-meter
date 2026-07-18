// The Settings window's UI (the `settings` window). Hosts every user-
// configurable setting — per-model visibility, refresh interval, notification
// thresholds, tray icon, launch-at-login, and session management — plus the
// first-run setup wizard, which auto-opens here on first launch (when the
// Settings window opens itself) and reopens from "Run setup again".
//
// Kept separate from `popover-view.ts` so `main.ts` only routes on the window
// label; the pure, DOM-free logic stays in `settings-view-model.ts`.

import { openUrl } from "@tauri-apps/plugin-opener";

import { describeImportSummary } from "./browser-import";
import { renderBrowserList } from "./browser-import-render";
import { createIconStylePicker } from "./icon-style-picker";
import { describeError } from "./ipc";
import type { UsageBackend } from "./ipc";
import { renderModelToggles, renderSelectOptions } from "./settings-render";
import { DEFAULT_SETTINGS, scopedModelNames, toggleModel } from "./settings-view-model";
import {
  REFRESH_INTERVAL_OPTIONS,
  type AppSettings,
  type Browser,
  type PopoverLayout,
  type RefreshInterval,
  type UsageSnapshot,
} from "./types";
import { describeWizardValidation } from "./wizard-view-model";
import { createWizard } from "./wizard";

function requireElement<T extends HTMLElement>(id: string): T {
  const el = document.getElementById(id);
  if (!el) {
    throw new Error(`missing #${id} in index.html`);
  }
  return el as T;
}

/** Reflect the selected option of a `.segmented` radio group by `data-value`. */
function setSegmentedValue(container: HTMLElement, value: string): void {
  for (const option of container.querySelectorAll<HTMLButtonElement>(".segmented-option")) {
    const selected = option.dataset.value === value;
    option.classList.toggle("is-selected", selected);
    option.setAttribute("aria-checked", selected ? "true" : "false");
  }
}

export function initSettingsView(backend: UsageBackend): void {
  const modelTogglesEl = requireElement<HTMLElement>("model-toggles");
  const refreshIntervalSelect = requireElement<HTMLSelectElement>("refresh-interval-select");
  const warningInput = requireElement<HTMLInputElement>("warning-threshold");
  const warningValue = requireElement<HTMLElement>("warning-threshold-value");
  const criticalInput = requireElement<HTMLInputElement>("critical-threshold");
  const criticalValue = requireElement<HTMLElement>("critical-threshold-value");
  const iconStyleContainer = requireElement<HTMLElement>("icon-style-picker");
  const monochromeToggle = requireElement<HTMLInputElement>("monochrome-toggle");
  const showResetTimeToggle = requireElement<HTMLInputElement>("show-reset-time-toggle");
  const popoverLayoutToggle = requireElement<HTMLElement>("popover-layout-toggle");
  const displayModeToggle = requireElement<HTMLElement>("display-mode-toggle");
  const weeklyPaceDaysToggle = requireElement<HTMLElement>("weekly-pace-days-toggle");
  const autostartToggle = requireElement<HTMLInputElement>("autostart-toggle");
  const autostartError = requireElement<HTMLElement>("autostart-error");
  const settingsSessionStatus = requireElement<HTMLElement>("settings-session-status");
  const settingsSessionForm = requireElement<HTMLFormElement>("settings-session-form");
  const settingsSessionInput = requireElement<HTMLInputElement>("settings-session-input");
  const settingsSessionError = requireElement<HTMLElement>("settings-session-error");
  const clearSessionButton = requireElement<HTMLButtonElement>("clear-session-button");
  const browserImportList = requireElement<HTMLElement>("browser-import-list");
  const browserImportStatus = requireElement<HTMLElement>("browser-import-status");
  const browserImportError = requireElement<HTMLElement>("browser-import-error");
  const runSetupAgainButton = requireElement<HTMLButtonElement>("run-setup-again-button");

  renderSelectOptions(refreshIntervalSelect, REFRESH_INTERVAL_OPTIONS);

  let settings: AppSettings = DEFAULT_SETTINGS;
  let latestSnapshot: UsageSnapshot | null = null;

  const iconStylePicker = createIconStylePicker(
    iconStyleContainer,
    settings.icon_style,
    (style) => {
      settings = { ...settings, icon_style: style };
      backend.setIconStyle(style).catch((error: unknown) => {
        console.error("failed to persist icon style", error);
      });
    },
  );
  backend
    .iconStylePreviews()
    .then((previews) => iconStylePicker.setPreviews(previews))
    .catch((error: unknown) => {
      console.error("failed to load icon style previews", error);
    });

  function renderToggles(): void {
    renderModelToggles(
      modelTogglesEl,
      scopedModelNames(latestSnapshot),
      new Set(settings.shown_scoped_models),
      handleModelToggle,
    );
  }

  function applySettingsToForm(): void {
    refreshIntervalSelect.value = settings.refresh_interval;
    warningInput.value = String(settings.warning_threshold);
    warningValue.textContent = `${settings.warning_threshold}%`;
    criticalInput.value = String(settings.critical_threshold);
    criticalValue.textContent = `${settings.critical_threshold}%`;
    iconStylePicker.setSelected(settings.icon_style);
    monochromeToggle.checked = settings.monochrome;
    showResetTimeToggle.checked = settings.show_reset_time;
    setSegmentedValue(popoverLayoutToggle, settings.popover_layout);
    setSegmentedValue(displayModeToggle, settings.pace_first_display ? "pace" : "consumption");
    setSegmentedValue(weeklyPaceDaysToggle, String(settings.weekly_pace_days));
  }

  function refreshSessionStatus(): void {
    backend
      .sessionStatus()
      .then((status) => {
        settingsSessionStatus.textContent =
          status === "present" ? "A session key is stored." : "No session key is stored.";
      })
      .catch((error: unknown) => {
        console.error("failed to read session status", error);
      });
  }

  /** Read the live OS registration state and reflect it in the toggle
   * (issue #12). Called on load, since the registration can be changed
   * outside the app and a cached value would go stale. */
  function refreshAutostartStatus(): void {
    autostartError.hidden = true;
    backend
      .autostartStatus()
      .then((enabled) => {
        autostartToggle.checked = enabled;
      })
      .catch((error: unknown) => {
        // A failed read leaves the checkbox at its previous value with nothing
        // on screen to say it might not reflect the real OS registration —
        // surface it the same way as the write path below.
        console.error("failed to read autostart status", error);
        autostartError.textContent = describeError(error);
        autostartError.hidden = false;
      });
  }

  function loadBrowserList(): void {
    backend
      .listBrowserSessions()
      .then((browsers) => {
        renderBrowserList(browserImportList, browsers, handleBrowserImport, handleOpenSettingsPane);
      })
      .catch((error: unknown) => {
        console.error("failed to list importable browsers", error);
      });
  }

  function handleBrowserImport(browser: Browser): void {
    browserImportError.hidden = true;
    browserImportStatus.hidden = true;
    backend
      .importBrowserSession(browser)
      .then((summary) => {
        browserImportStatus.textContent = describeImportSummary(summary);
        browserImportStatus.hidden = false;
        refreshSessionStatus();
      })
      .catch((error: unknown) => {
        browserImportError.textContent = describeError(error);
        browserImportError.hidden = false;
      });
  }

  function handleOpenSettingsPane(url: string): void {
    openUrl(url).catch((error: unknown) => {
      console.error("failed to open settings pane", error);
    });
  }

  function handleModelToggle(name: string, enabled: boolean): void {
    const next = toggleModel(settings.shown_scoped_models, name, enabled);
    settings = { ...settings, shown_scoped_models: next };
    backend.setShownScopedModels(next).catch((error: unknown) => {
      console.error("failed to persist model visibility", error);
    });
    renderToggles();
  }

  const wizard = createWizard(backend, {
    onIconStyleChange(style) {
      settings = { ...settings, icon_style: style };
      applySettingsToForm();
    },
    onRefreshIntervalChange(interval) {
      settings = { ...settings, refresh_interval: interval };
      applySettingsToForm();
    },
    onClose() {
      // The wizard may have changed the session (imported or pasted) or the
      // browser list's permission state; refresh both so the Settings page
      // reflects it.
      refreshSessionStatus();
      loadBrowserList();
    },
  });
  runSetupAgainButton.addEventListener("click", () => wizard.open());
  wizard.maybeAutoOpen();

  backend
    .getSettings()
    .then((loaded) => {
      settings = loaded;
      applySettingsToForm();
      renderToggles();
      return backend.initialState();
    })
    .then((state) => {
      latestSnapshot = state.snapshot;
      renderToggles();
    })
    .catch((error: unknown) => {
      console.error("failed to load initial settings state", error);
    });
  backend.onStateChange((state) => {
    latestSnapshot = state.snapshot;
    renderToggles();
  });

  refreshSessionStatus();
  loadBrowserList();
  refreshAutostartStatus();

  refreshIntervalSelect.addEventListener("change", () => {
    const interval = refreshIntervalSelect.value as RefreshInterval;
    settings = { ...settings, refresh_interval: interval };
    backend.setRefreshInterval(interval).catch((error: unknown) => {
      console.error("failed to persist refresh interval", error);
    });
  });

  warningInput.addEventListener("input", () => {
    warningValue.textContent = `${warningInput.value}%`;
  });
  warningInput.addEventListener("change", () => {
    const warning = Number(warningInput.value);
    settings = { ...settings, warning_threshold: warning };
    backend.setThresholds(warning, settings.critical_threshold).catch((error: unknown) => {
      console.error("failed to persist warning threshold", error);
    });
  });

  criticalInput.addEventListener("input", () => {
    criticalValue.textContent = `${criticalInput.value}%`;
  });
  criticalInput.addEventListener("change", () => {
    const critical = Number(criticalInput.value);
    settings = { ...settings, critical_threshold: critical };
    backend.setThresholds(settings.warning_threshold, critical).catch((error: unknown) => {
      console.error("failed to persist critical threshold", error);
    });
  });

  monochromeToggle.addEventListener("change", () => {
    settings = { ...settings, monochrome: monochromeToggle.checked };
    backend.setMonochrome(monochromeToggle.checked).catch((error: unknown) => {
      console.error("failed to persist monochrome setting", error);
    });
  });

  showResetTimeToggle.addEventListener("change", () => {
    settings = { ...settings, show_reset_time: showResetTimeToggle.checked };
    backend.setShowResetTime(showResetTimeToggle.checked).catch((error: unknown) => {
      console.error("failed to persist show-reset-time setting", error);
    });
  });

  for (const option of popoverLayoutToggle.querySelectorAll<HTMLButtonElement>(".segmented-option")) {
    option.addEventListener("click", () => {
      const layout = option.dataset.value as PopoverLayout;
      if (settings.popover_layout === layout) {
        return;
      }
      settings = { ...settings, popover_layout: layout };
      setSegmentedValue(popoverLayoutToggle, layout);
      backend.setPopoverLayout(layout).catch((error: unknown) => {
        console.error("failed to persist popover layout", error);
      });
    });
  }

  for (const option of displayModeToggle.querySelectorAll<HTMLButtonElement>(".segmented-option")) {
    option.addEventListener("click", () => {
      const paceFirst = option.dataset.value === "pace";
      if (settings.pace_first_display === paceFirst) {
        return;
      }
      settings = { ...settings, pace_first_display: paceFirst };
      setSegmentedValue(displayModeToggle, paceFirst ? "pace" : "consumption");
      backend.setPaceFirstDisplay(paceFirst).catch((error: unknown) => {
        console.error("failed to persist display mode", error);
      });
    });
  }

  for (const option of weeklyPaceDaysToggle.querySelectorAll<HTMLButtonElement>(".segmented-option")) {
    option.addEventListener("click", () => {
      const days = Number(option.dataset.value);
      if (settings.weekly_pace_days === days) {
        return;
      }
      settings = { ...settings, weekly_pace_days: days };
      setSegmentedValue(weeklyPaceDaysToggle, String(days));
      backend.setWeeklyPaceDays(days).catch((error: unknown) => {
        console.error("failed to persist weekly pace basis", error);
      });
    });
  }

  autostartToggle.addEventListener("change", () => {
    const requested = autostartToggle.checked;
    autostartError.hidden = true;
    // Guard against overlapping enable/disable IPC calls against the same
    // OS-level Launch Agent / .desktop entry: without this, rapid re-toggling
    // can fire two requests whose responses arrive out of order, leaving the
    // checkbox showing whichever settled last rather than the actual final OS
    // registration.
    autostartToggle.disabled = true;
    backend
      .setAutostart(requested)
      .then((actual) => {
        // Reconcile against what the OS actually reports rather than trusting
        // `requested` — the toggle must always show ground truth.
        autostartToggle.checked = actual;
      })
      .catch((error: unknown) => {
        // Registration failed (e.g. no permission to write the Launch Agent /
        // autostart entry): revert the checkbox and surface why.
        autostartToggle.checked = !requested;
        autostartError.textContent = describeError(error);
        autostartError.hidden = false;
      })
      .finally(() => {
        autostartToggle.disabled = false;
      });
  });

  settingsSessionForm.addEventListener("submit", (event) => {
    event.preventDefault();
    const value = settingsSessionInput.value.trim();
    if (!value) {
      return;
    }
    settingsSessionError.hidden = true;
    backend
      .submitSessionKey(value)
      .then((result) => {
        settingsSessionInput.value = "";
        if (!result.validated) {
          settingsSessionError.textContent = describeWizardValidation(result);
          settingsSessionError.hidden = false;
        }
        refreshSessionStatus();
      })
      .catch((error: unknown) => {
        settingsSessionError.textContent = describeError(error);
        settingsSessionError.hidden = false;
      });
  });

  clearSessionButton.addEventListener("click", () => {
    backend
      .clearSessionKey()
      .then(() => {
        refreshSessionStatus();
      })
      .catch((error: unknown) => {
        console.error("failed to clear session key", error);
      });
  });
}
