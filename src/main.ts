// Entry point for the popover UI (issue #5). Wires the IPC backend (real
// Tauri commands/events, or the demo backend outside a Tauri shell) to the
// pure view-model and the DOM renderer, and runs the client-side countdown
// tick. The frontend owns no polling: every usage number comes from the
// `usage-state` event; only the countdown text is recomputed locally.

import { createBackend, describeError } from "./ipc";
import { applyBanner, renderCards, tickCountdowns } from "./render";
import type { MeterState } from "./types";
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

  const backend = createBackend();

  function render(state: MeterState): void {
    const viewModel = buildViewModel(state, new Date());
    applyBanner(statusLineEl, viewModel.bannerKind, viewModel.statusLine);
    renderCards(cardsEl, viewModel.cards);
    emptyStateEl.hidden = viewModel.cards.length > 0 || viewModel.showSessionForm;
    sessionForm.hidden = !viewModel.showSessionForm;
    if (!viewModel.showSessionForm) {
      sessionError.hidden = true;
    }
  }

  backend
    .initialState()
    .then(render)
    .catch((error: unknown) => {
      // The initial pull failed (should not happen once Tauri is up, but a
      // very early webview load could race the managed state); the next
      // `usage-state` broadcast still recovers the view.
      console.error("failed to load initial usage state", error);
    });
  backend.onStateChange(render);

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

  window.setInterval(() => {
    tickCountdowns(cardsEl, new Date());
  }, COUNTDOWN_TICK_MS);
}

window.addEventListener("DOMContentLoaded", main);
