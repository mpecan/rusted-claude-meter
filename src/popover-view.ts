// The popover window's UI (the `main` window). Wires the IPC backend to the
// pure view-model and DOM renderers, runs the client-side countdown tick, and
// hands off to the dedicated Settings window when the Settings button is
// pressed. It owns no polling: every usage number comes from the `usage-state`
// event, and its card filter (which scoped models to show) is kept live across
// windows by the `settings-changed` broadcast, since Settings now lives in a
// separate window and no longer shares this view's `settings` object.

import { describeError } from "./ipc";
import type { UsageBackend } from "./ipc";
import { applyBanner, renderCards, tickCountdowns } from "./render";
import { DEFAULT_SETTINGS } from "./settings-view-model";
import type { AppSettings, MeterState } from "./types";
import { buildViewModel } from "./view-model";
import { describeWizardValidation } from "./wizard-view-model";

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

export function initPopoverView(backend: UsageBackend): void {
  const statusLineEl = requireElement<HTMLElement>("status-line");
  const cardsEl = requireElement<HTMLElement>("cards");
  const emptyStateEl = requireElement<HTMLElement>("empty-state");
  const sessionForm = requireElement<HTMLFormElement>("session-form");
  const sessionInput = requireElement<HTMLInputElement>("session-input");
  const sessionError = requireElement<HTMLElement>("session-error");
  const refreshButton = requireElement<HTMLButtonElement>("refresh-button");
  const settingsButton = requireElement<HTMLButtonElement>("settings-button");

  let settings: AppSettings = DEFAULT_SETTINGS;
  let latestState: MeterState | null = null;

  function shownScopedModels(): Set<string> {
    return new Set(settings.shown_scoped_models);
  }

  function render(state: MeterState): void {
    latestState = state;
    const viewModel = buildViewModel(
      state,
      new Date(),
      shownScopedModels(),
      settings.show_reset_time,
      settings.warning_threshold,
      settings.critical_threshold,
    );
    applyBanner(statusLineEl, viewModel.bannerKind, viewModel.statusLine);
    renderCards(cardsEl, viewModel.cards, settings.popover_layout);
    emptyStateEl.hidden = viewModel.cards.length > 0 || viewModel.showSessionForm;
    sessionForm.hidden = !viewModel.showSessionForm;
    if (!viewModel.showSessionForm) {
      sessionError.hidden = true;
    }
  }

  backend
    .getSettings()
    .then((loaded) => {
      settings = loaded;
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
  backend.onSettingsChanged((updated) => {
    // A setting changed in the Settings window (e.g. model visibility);
    // re-render the cards against the new filter without waiting for the next
    // usage broadcast.
    settings = updated;
    if (latestState) {
      render(latestState);
    }
  });

  sessionForm.addEventListener("submit", (event) => {
    event.preventDefault();
    const value = sessionInput.value.trim();
    if (!value) {
      return;
    }
    sessionError.hidden = true;
    backend
      .submitSessionKey(value)
      .then((result) => {
        sessionInput.value = "";
        if (!result.validated) {
          // Stored, but claude.ai was unreachable to confirm it — say so
          // rather than silently accepting (mirrors the wizard's copy).
          sessionError.textContent = describeWizardValidation(result);
          sessionError.hidden = false;
        }
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

  function openSettings(): void {
    backend.openSettingsWindow().catch((error: unknown) => {
      console.error("failed to open settings window", error);
    });
  }
  settingsButton.addEventListener("click", openSettings);

  // First run (settings.json didn't exist before this launch): open the
  // Settings window so its setup wizard greets the user in a real, front-most
  // window. The popover's webview loads even while its window is hidden, so
  // this fires on launch without waiting for a tray click. The wizard itself
  // lives in — and auto-opens from — the Settings window (see
  // `settings-view.ts`); this only surfaces that window.
  backend
    .wizardShouldRun()
    .then((shouldRun) => {
      if (shouldRun) {
        openSettings();
      }
    })
    .catch((error: unknown) => {
      console.error("failed to determine first-run status", error);
    });

  window.setInterval(() => {
    tickCountdowns(cardsEl, new Date());
  }, COUNTDOWN_TICK_MS);
}
