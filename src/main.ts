// Entry point for both windows. A single vite bundle is loaded by the popover
// (`main`) window and the dedicated Settings (`settings`) window; this router
// reads the current window's Tauri label and reveals + wires exactly one view.
// The label→view decision is the pure `resolveView` (see `view-routing.ts`);
// each view's DOM wiring lives in `popover-view.ts` / `settings-view.ts`.

import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { createBackend } from "./ipc";
import { initPopoverView } from "./popover-view";
import { initSettingsView } from "./settings-view";
import { resolveView } from "./view-routing";

/** The current window's label, or "main" outside a Tauri shell (`npm run dev`
 * in a plain browser), so the demo always renders the popover. Outside Tauri a
 * `?window=<label>` query override lets either surface be previewed in a plain
 * browser (dev/design only — `isTauri()` is always true in the real app). */
function currentWindowLabel(): string {
  if (!isTauri()) {
    return new URLSearchParams(window.location.search).get("window") ?? "main";
  }
  try {
    return getCurrentWindow().label;
  } catch (error) {
    console.error("failed to read the current window label", error);
    return "main";
  }
}

function reveal(viewId: string): void {
  const el = document.getElementById(viewId);
  if (!el) {
    throw new Error(`missing #${viewId} in index.html`);
  }
  el.hidden = false;
}

function main(): void {
  const backend = createBackend();
  if (resolveView(currentWindowLabel()) === "settings") {
    reveal("settings-view");
    initSettingsView(backend);
  } else {
    reveal("popover-view");
    initPopoverView(backend);
  }
}

window.addEventListener("DOMContentLoaded", main);
