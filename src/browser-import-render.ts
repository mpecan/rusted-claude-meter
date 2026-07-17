// DOM rendering for the "import from a browser" list (issue #10). Kept
// separate from `main.ts` (event wiring) and `browser-import.ts` (pure
// helpers) for the same reasons `render.ts` / `settings-render.ts` are split.

import type { Browser, DetectedBrowser } from "./types";

/** Rebuild the detected-browser list from scratch. `onImport` fires when the
 * user asks to import from a browser; `onOpenSettings` opens a permission
 * settings pane (the macOS Full Disk Access deep link for Safari). */
export function renderBrowserList(
  container: HTMLElement,
  browsers: readonly DetectedBrowser[],
  onImport: (browser: Browser) => void,
  onOpenSettings: (url: string) => void,
): void {
  if (browsers.length === 0) {
    const empty = document.createElement("p");
    empty.className = "browser-import-empty settings-hint";
    empty.textContent = "No supported browsers on this system.";
    container.replaceChildren(empty);
    return;
  }
  container.replaceChildren(
    ...browsers.map((browser) => buildRow(browser, onImport, onOpenSettings)),
  );
}

function buildRow(
  browser: DetectedBrowser,
  onImport: (browser: Browser) => void,
  onOpenSettings: (url: string) => void,
): HTMLElement {
  const row = document.createElement("div");
  row.className = "browser-import-row";

  const header = document.createElement("div");
  header.className = "browser-import-header";

  const name = document.createElement("span");
  name.className = "browser-import-name";
  name.textContent = browser.name;

  const importButton = document.createElement("button");
  importButton.type = "button";
  importButton.className = "ghost-button";
  importButton.textContent = "Import";
  importButton.addEventListener("click", () => onImport(browser.id));

  header.append(name, importButton);
  row.append(header);

  if (browser.permission_hint) {
    const hint = document.createElement("p");
    hint.className = "browser-import-hint settings-hint";
    hint.textContent = browser.permission_hint;
    row.append(hint);
  }

  const deepLink = browser.settings_deep_link;
  if (deepLink) {
    const settingsButton = document.createElement("button");
    settingsButton.type = "button";
    settingsButton.className = "ghost-button browser-import-settings";
    settingsButton.textContent = "Open Settings";
    settingsButton.addEventListener("click", () => onOpenSettings(deepLink));
    row.append(settingsButton);
  }

  return row;
}
