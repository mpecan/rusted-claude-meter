// Entry point for the popover UI (issue #5) and the Settings panel (issue
// #6). Wires the IPC backend (real Tauri commands/events, or the demo
// backend outside a Tauri shell) to the pure view-models and the DOM
// renderers, and runs the client-side countdown tick. The frontend owns no
// polling: every usage number comes from the `usage-state` event; only the
// countdown text and the settings form's local echo are recomputed locally.

import { openUrl } from "@tauri-apps/plugin-opener";

import { describeImportSummary } from "./browser-import";
import { renderBrowserList } from "./browser-import-render";
import { createBackend, describeError } from "./ipc";
import { applyBanner, renderCards, tickCountdowns } from "./render";
import { renderModelToggles } from "./settings-render";
import { DEFAULT_SETTINGS, scopedModelNames, toggleModel } from "./settings-view-model";
import type { AppSettings, Browser, IconStyle, MeterState, RefreshInterval } from "./types";
import { buildViewModel } from "./view-model";

/** How often the reset countdowns re-render. A minute-granularity display
 * only needs to tick once a minute, but a second is cheap and keeps
 * "resets soon" transitions snappy. */
const COUNTDOWN_TICK_MS = 1000;

function requireElement<T extends HTMLElement>(id: string): T {
  const el = document.getElementById(id);
  if (!el) {
    throw new Error(`missing #${id} in index.html`);
  }
  return el as T;
}

function main(): void {
  const statusLineEl = requireElement<HTMLElement>("status-line");
  const cardsEl = requireElement<HTMLElement>("cards");
  const emptyStateEl = requireElement<HTMLElement>("empty-state");
  const sessionForm = requireElement<HTMLFormElement>("session-form");
  const sessionInput = requireElement<HTMLInputElement>("session-input");
  const sessionError = requireElement<HTMLElement>("session-error");
  const refreshButton = requireElement<HTMLButtonElement>("refresh-button");

  const settingsButton = requireElement<HTMLButtonElement>("settings-button");
  const settingsPanel = requireElement<HTMLElement>("settings-panel");
  const closeSettingsButton = requireElement<HTMLButtonElement>("close-settings-button");
  const modelTogglesEl = requireElement<HTMLElement>("model-toggles");
  const refreshIntervalSelect = requireElement<HTMLSelectElement>("refresh-interval-select");
  const warningInput = requireElement<HTMLInputElement>("warning-threshold");
  const warningValue = requireElement<HTMLElement>("warning-threshold-value");
  const criticalInput = requireElement<HTMLInputElement>("critical-threshold");
  const criticalValue = requireElement<HTMLElement>("critical-threshold-value");
  const iconStyleSelect = requireElement<HTMLSelectElement>("icon-style-select");
  const monochromeToggle = requireElement<HTMLInputElement>("monochrome-toggle");
  const settingsSessionStatus = requireElement<HTMLElement>("settings-session-status");
  const settingsSessionForm = requireElement<HTMLFormElement>("settings-session-form");
  const settingsSessionInput = requireElement<HTMLInputElement>("settings-session-input");
  const settingsSessionError = requireElement<HTMLElement>("settings-session-error");
  const clearSessionButton = requireElement<HTMLButtonElement>("clear-session-button");
  const browserImportList = requireElement<HTMLElement>("browser-import-list");
  const browserImportStatus = requireElement<HTMLElement>("browser-import-status");
  const browserImportError = requireElement<HTMLElement>("browser-import-error");

  const backend = createBackend();

  let settings: AppSettings = DEFAULT_SETTINGS;
  let latestState: MeterState | null = null;

  function shownScopedModels(): Set<string> {
    return new Set(settings.shown_scoped_models);
  }

  function render(state: MeterState): void {
    latestState = state;
    const viewModel = buildViewModel(state, new Date(), shownScopedModels());
    applyBanner(statusLineEl, viewModel.bannerKind, viewModel.statusLine);
    renderCards(cardsEl, viewModel.cards);
    emptyStateEl.hidden = viewModel.cards.length > 0 || viewModel.showSessionForm;
    sessionForm.hidden = !viewModel.showSessionForm;
    if (!viewModel.showSessionForm) {
      sessionError.hidden = true;
    }
    renderModelToggles(
      modelTogglesEl,
      scopedModelNames(state.snapshot),
      shownScopedModels(),
      handleModelToggle,
    );
  }

  function applySettingsToForm(): void {
    refreshIntervalSelect.value = settings.refresh_interval;
    warningInput.value = String(settings.warning_threshold);
    warningValue.textContent = `${settings.warning_threshold}%`;
    criticalInput.value = String(settings.critical_threshold);
    criticalValue.textContent = `${settings.critical_threshold}%`;
    iconStyleSelect.value = settings.icon_style;
    monochromeToggle.checked = settings.monochrome;
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

  function openSettingsPanel(): void {
    settingsPanel.hidden = false;
    refreshSessionStatus();
    loadBrowserList();
  }

  function handleModelToggle(name: string, enabled: boolean): void {
    const next = toggleModel(settings.shown_scoped_models, name, enabled);
    settings = { ...settings, shown_scoped_models: next };
    backend.setShownScopedModels(next).catch((error: unknown) => {
      console.error("failed to persist model visibility", error);
    });
    if (latestState) {
      render(latestState);
    }
  }

  backend
    .getSettings()
    .then((loaded) => {
      settings = loaded;
      applySettingsToForm();
      return backend.initialState();
    })
    .then(render)
    .catch((error: unknown) => {
      // The initial pull failed (should not happen once Tauri is up, but a
      // very early webview load could race the managed state); the next
      // `usage-state` broadcast still recovers the view.
      console.error("failed to load initial usage state", error);
    });
  backend.onStateChange(render);
  backend.onOpenSettings(openSettingsPanel);

  sessionForm.addEventListener("submit", (event) => {
    event.preventDefault();
    const value = sessionInput.value.trim();
    if (!value) {
      return;
    }
    sessionError.hidden = true;
    backend
      .submitSessionKey(value)
      .then(() => {
        sessionInput.value = "";
      })
      .catch((error: unknown) => {
        sessionError.textContent = describeError(error);
        sessionError.hidden = false;
      });
  });

  refreshButton.addEventListener("click", () => {
    backend.refreshUsage().catch((error: unknown) => {
      console.error("manual refresh failed", error);
    });
  });

  settingsButton.addEventListener("click", openSettingsPanel);

  closeSettingsButton.addEventListener("click", () => {
    settingsPanel.hidden = true;
  });

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

  iconStyleSelect.addEventListener("change", () => {
    const style = iconStyleSelect.value as IconStyle;
    settings = { ...settings, icon_style: style };
    backend.setIconStyle(style).catch((error: unknown) => {
      console.error("failed to persist icon style", error);
    });
  });

  monochromeToggle.addEventListener("change", () => {
    settings = { ...settings, monochrome: monochromeToggle.checked };
    backend.setMonochrome(monochromeToggle.checked).catch((error: unknown) => {
      console.error("failed to persist monochrome setting", error);
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
      .then(() => {
        settingsSessionInput.value = "";
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

  window.setInterval(() => {
    tickCountdowns(cardsEl, new Date());
  }, COUNTDOWN_TICK_MS);
}

window.addEventListener("DOMContentLoaded", main);
