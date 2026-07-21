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
  type UsageMode,
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

/** Wire a `.segmented` radiogroup to a settings field: on each option click,
 * parse the clicked `data-value`, skip if it already matches, otherwise
 * optimistically reflect the selection and persist it (logging any persist
 * failure). Shared by every segmented control so the boilerplate lives once. */
function bindSegmented<T>(
  container: HTMLElement,
  isCurrent: (value: T) => boolean,
  parse: (raw: string) => T,
  onChange: (value: T) => Promise<unknown>,
  label: string,
): void {
  for (const option of container.querySelectorAll<HTMLButtonElement>(".segmented-option")) {
    option.addEventListener("click", () => {
      const raw = option.dataset.value ?? "";
      const value = parse(raw);
      if (isCurrent(value)) {
        return;
      }
      setSegmentedValue(container, raw);
      onChange(value).catch((error: unknown) => {
        console.error(`failed to persist ${label}`, error);
      });
    });
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
  const notifyOnResetToggle = requireElement<HTMLInputElement>("notify-on-reset-toggle");
  const testNotificationButton = requireElement<HTMLButtonElement>("send-test-notification-button");
  const testNotificationStatus = requireElement<HTMLElement>("test-notification-status");
  const usageModeToggle = requireElement<HTMLElement>("usage-mode-toggle");
  const popoverLayoutToggle = requireElement<HTMLElement>("popover-layout-toggle");
  const paceTrackingToggle = requireElement<HTMLInputElement>("pace-tracking-toggle");
  const paceConfig = requireElement<HTMLElement>("pace-config");
  const displayModeToggle = requireElement<HTMLElement>("display-mode-toggle");
  const weeklyPaceDaysToggle = requireElement<HTMLElement>("weekly-pace-days-toggle");
  const autostartToggle = requireElement<HTMLInputElement>("autostart-toggle");
  const autostartError = requireElement<HTMLElement>("autostart-error");
  const debugLoggingToggle = requireElement<HTMLInputElement>("debug-logging-toggle");
  const debugLogLocation = requireElement<HTMLElement>("debug-log-location");
  const debugLogPathEl = requireElement<HTMLElement>("debug-log-path");
  const revealDebugLogButton = requireElement<HTMLButtonElement>("reveal-debug-log-button");
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
    notifyOnResetToggle.checked = settings.notify_on_reset;
    debugLoggingToggle.checked = settings.debug_logging;
    setSegmentedValue(usageModeToggle, settings.usage_mode);
    setSegmentedValue(popoverLayoutToggle, settings.popover_layout);
    paceTrackingToggle.checked = settings.pace_tracking_enabled;
    paceConfig.hidden = !settings.pace_tracking_enabled;
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

  /** Show the resolved API-response log path (Debug section). Called on load;
   * the path is fixed for the process, so a failure or a `null` (no log dir
   * resolvable) simply leaves the location row hidden. */
  function loadDebugLogPath(): void {
    backend
      .debugLogPath()
      .then((path) => {
        if (path) {
          debugLogPathEl.textContent = path;
          debugLogLocation.hidden = false;
        }
      })
      .catch((error: unknown) => {
        console.error("failed to read debug log path", error);
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
  loadDebugLogPath();

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

  notifyOnResetToggle.addEventListener("change", () => {
    settings = { ...settings, notify_on_reset: notifyOnResetToggle.checked };
    backend.setNotifyOnReset(notifyOnResetToggle.checked).catch((error: unknown) => {
      console.error("failed to persist notify-on-reset setting", error);
    });
  });

  testNotificationButton.addEventListener("click", () => {
    testNotificationStatus.hidden = true;
    // Guard against a double-fire while the send is in flight.
    testNotificationButton.disabled = true;
    backend
      .sendTestNotification()
      .then((delivered) => {
        testNotificationStatus.textContent = delivered
          ? "Sent — check your notifications. If nothing appeared, look at Focus/Do Not Disturb and this app's notification permission."
          : "Couldn't deliver a notification — check this app's notification permission and that notifications aren't disabled.";
        testNotificationStatus.hidden = false;
      })
      .catch((error: unknown) => {
        console.error("failed to send test notification", error);
        testNotificationStatus.textContent = describeError(error);
        testNotificationStatus.hidden = false;
      })
      .finally(() => {
        testNotificationButton.disabled = false;
      });
  });

  debugLoggingToggle.addEventListener("change", () => {
    const enabled = debugLoggingToggle.checked;
    settings = { ...settings, debug_logging: enabled };
    backend.setDebugLogging(enabled).catch((error: unknown) => {
      console.error("failed to persist debug-logging setting", error);
    });
  });

  revealDebugLogButton.addEventListener("click", () => {
    backend.revealDebugLog().catch((error: unknown) => {
      console.error("failed to reveal debug log", error);
    });
  });

  paceTrackingToggle.addEventListener("change", () => {
    const enabled = paceTrackingToggle.checked;
    settings = { ...settings, pace_tracking_enabled: enabled };
    // Hide the sub-controls (display mode + weekly basis) when the whole
    // feature is off — they only matter while pace tracking is enabled.
    paceConfig.hidden = !enabled;
    backend.setPaceTrackingEnabled(enabled).catch((error: unknown) => {
      console.error("failed to persist pace-tracking setting", error);
    });
  });

  bindSegmented<UsageMode>(
    usageModeToggle,
    (mode) => settings.usage_mode === mode,
    (raw) => raw as UsageMode,
    (mode) => {
      settings = { ...settings, usage_mode: mode };
      return backend.setUsageMode(mode);
    },
    "usage mode",
  );

  bindSegmented<PopoverLayout>(
    popoverLayoutToggle,
    (layout) => settings.popover_layout === layout,
    (raw) => raw as PopoverLayout,
    (layout) => {
      settings = { ...settings, popover_layout: layout };
      return backend.setPopoverLayout(layout);
    },
    "popover layout",
  );

  bindSegmented<boolean>(
    displayModeToggle,
    (paceFirst) => settings.pace_first_display === paceFirst,
    (raw) => raw === "pace",
    (paceFirst) => {
      settings = { ...settings, pace_first_display: paceFirst };
      return backend.setPaceFirstDisplay(paceFirst);
    },
    "display mode",
  );

  bindSegmented<number>(
    weeklyPaceDaysToggle,
    (days) => settings.weekly_pace_days === days,
    (raw) => Number(raw),
    (days) => {
      settings = { ...settings, weekly_pace_days: days };
      return backend.setWeeklyPaceDays(days);
    },
    "weekly pace basis",
  );

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
