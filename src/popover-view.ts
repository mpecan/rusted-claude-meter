// The popover window's UI (the `main` window). Wires the IPC backend to the
// pure view-model and DOM renderers, runs the client-side countdown tick, and
// hands off to the dedicated Settings window when the Settings button is
// pressed. It owns no polling: every usage number comes from the `usage-state`
// event, and its card filter (which scoped models to show) is kept live across
// windows by the `settings-changed` broadcast, since Settings now lives in a
// separate window and no longer shares this view's `settings` object.

import { buildCostViewModel, effectiveUsageMode } from "./cost-view-model";
import { describeError } from "./ipc";
import type { UsageBackend } from "./ipc";
import { applyBanner, buildCostCard, renderCards, renderCostCards, tickCountdowns } from "./render";
import { DEFAULT_SETTINGS } from "./settings-view-model";
import type { AppSettings, MeterState, UsageMode } from "./types";
import { buildViewModel } from "./view-model";
import { describeWizardValidation } from "./wizard-view-model";

/** How often the reset countdowns re-render. A minute-granularity display
 * only needs to tick once a minute, but a second is cheap and keeps
 * "resets soon" transitions snappy. */
const COUNTDOWN_TICK_MS = 1000;

/** localStorage key recording that the one-time "showing the cost view" hint
 * has actually been seen by the user, so it appears once rather than every
 * broadcast. Set only while the popover is visible (see `markCostHintSeen`),
 * never merely because a render happened — the webview renders on launch while
 * the popover is still hidden, so consuming the flag at render time would burn
 * the hint into a DOM nobody can see. */
const COST_HINT_KEY = "rcm-cost-hint-shown";

/** The one-time cost-view hint copy, shown when Auto (not a pinned Cost)
 * resolved to the cost view. */
const COST_HINT_TEXT = "Showing the cost view — change in Settings.";

/** Whether the one-time cost hint is still pending: only for Auto-resolved cost
 * (a pinned Cost never shows it) and only until it has been marked seen. Reads
 * the flag but never sets it — decoupled from rendering so it stays sticky
 * across the background broadcasts that re-render the hidden popover, and is
 * consumed only once the user actually sees it. Any localStorage failure
 * (private mode, disabled storage) simply suppresses the hint. */
function costHintPending(mode: UsageMode): boolean {
  if (mode !== "auto") {
    return false;
  }
  try {
    return window.localStorage.getItem(COST_HINT_KEY) !== "1";
  } catch {
    return false;
  }
}

/** Persist that the cost hint has now been seen, so it never shows again. Any
 * localStorage failure just means the hint may show again on the next visible
 * cost render — acceptable degradation, never an error. */
function markCostHintSeen(): void {
  try {
    window.localStorage.setItem(COST_HINT_KEY, "1");
  } catch {
    // Storage unavailable: nothing to persist.
  }
}

/** Whether the popover is actually on screen. macOS reveals the NSPopover by
 * focusing the `main` window, so either signal means the user can see the view;
 * on launch the webview renders while hidden and unfocused, so neither is true
 * and the hint stays pending until the popover is first opened. */
function popoverIsVisible(): boolean {
  return document.visibilityState === "visible" || document.hasFocus();
}

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
  // True while a still-pending cost hint is painted in the current view but the
  // popover has not yet been confirmed visible, so it must be marked seen the
  // moment the popover is opened (see `markHintSeenIfVisible`).
  let hintShownInView = false;

  function shownScopedModels(): Set<string> {
    return new Set(settings.shown_scoped_models);
  }

  /** Mark the cost hint seen once the popover is actually on screen while it is
   * being shown. Called both right after a hint render (handles the
   * already-open case) and from the focus/visibility listeners (handles the
   * popover being opened later). Gated on `hintShownInView` so a focus event
   * never burns the flag for an account that isn't showing the hint. */
  function markHintSeenIfVisible(): void {
    if (hintShownInView && popoverIsVisible()) {
      markCostHintSeen();
      hintShownInView = false;
    }
  }

  function render(state: MeterState): void {
    latestState = state;
    const viewModel = buildViewModel(state, new Date(), shownScopedModels(), {
      showResetTime: settings.show_reset_time,
      warning: settings.warning_threshold,
      critical: settings.critical_threshold,
      weeklyPaceDays: settings.weekly_pace_days,
      paceFirst: settings.pace_first_display,
      paceTrackingEnabled: settings.pace_tracking_enabled,
    });
    applyBanner(statusLineEl, viewModel.bannerKind, viewModel.statusLine);

    const snapshot = state.snapshot;
    const spend = snapshot?.spend ?? null;
    // Built once here — both the cost view and the allowance-mode summary card
    // render it from the same spend with the same thresholds.
    const costViewModel = spend
      ? buildCostViewModel(spend, settings.warning_threshold, settings.critical_threshold)
      : null;
    const mode = effectiveUsageMode(settings.usage_mode, snapshot);
    let hasContent: boolean;
    if (mode === "cost") {
      // Token/cost account (Enterprise auto-detected, or the user pinned Cost).
      // Branch on the effective mode first — exactly as the tray does — so a
      // pinned Cost renders the spend view even when the account carries no
      // spend object, never falling back to the allowance percentage cards the
      // user pinned away from.
      if (costViewModel) {
        const pending = costHintPending(settings.usage_mode);
        renderCostCards(cardsEl, costViewModel, pending ? COST_HINT_TEXT : null);
        // Sticky: keep re-rendering the hint on every broadcast until the
        // popover is actually seen, then consume the flag (immediately if it is
        // already open, otherwise on the next focus/visibility change).
        hintShownInView = pending;
        markHintSeenIfVisible();
        hasContent = true;
      } else {
        // Pinned Cost with no spend data: show nothing (the empty state),
        // mirroring the tray's empty gauge and blank menu rather than leaking
        // the allowance meters.
        cardsEl.dataset.layout = "cost";
        cardsEl.replaceChildren();
        hintShownInView = false;
        hasContent = false;
      }
    } else {
      renderCards(cardsEl, viewModel.cards, settings.popover_layout);
      hasContent = viewModel.cards.length > 0;
      hintShownInView = false;
      // Allowance view: surface a cost-summary card alongside the limit cards
      // when the account also reports spend.
      if (costViewModel) {
        cardsEl.append(buildCostCard(costViewModel));
        hasContent = true;
      }
    }
    emptyStateEl.hidden = hasContent || viewModel.showSessionForm;
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
  // The cost hint is consumed only once the popover is actually shown: macOS
  // reveals the NSPopover by focusing the `main` window, and either signal
  // confirms the user can see the hint that a launch-time render may have
  // painted while the popover was still hidden.
  document.addEventListener("visibilitychange", markHintSeenIfVisible);
  window.addEventListener("focus", markHintSeenIfVisible);
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
