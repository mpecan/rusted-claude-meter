// Pure helpers for the browser-session-import UI (issue #10). Kept DOM-free
// and backend-free so it stays trivially testable; the DOM rendering lives in
// `browser-import-render.ts` and the event/IPC wiring in `main.ts`.

import type { Browser, DetectedBrowser, ImportSummary } from "./types";

/** The most-used browsers, shown up-front in the import list in this order;
 * everything else detected is collapsed under a "More" section. Edit this
 * list to change which import targets are promoted. */
export const PRIMARY_IMPORT_BROWSERS: readonly Browser[] = [
  "chrome",
  "safari",
  "firefox",
  "edge",
];

/** Split detected import targets into the promoted `primary` ones (in
 * `PRIMARY_IMPORT_BROWSERS` order) and the `more` tail (kept in the backend's
 * detection order). Only browsers actually detected appear in either bucket. */
export function partitionImportTargets(browsers: readonly DetectedBrowser[]): {
  primary: DetectedBrowser[];
  more: DetectedBrowser[];
} {
  const primary: DetectedBrowser[] = [];
  for (const id of PRIMARY_IMPORT_BROWSERS) {
    const found = browsers.find((browser) => browser.id === id);
    if (found) {
      primary.push(found);
    }
  }
  const more = browsers.filter((browser) => !PRIMARY_IMPORT_BROWSERS.includes(browser.id));
  return { primary, more };
}

/** Human confirmation for a completed import. A key claude.ai has confirmed
 * reads differently from one stored but not yet verified (claude.ai was
 * unreachable at import time — the scheduler verifies it on the next poll). */
export function describeImportSummary(summary: ImportSummary): string {
  if (summary.validated) {
    return `Imported and verified the session from ${summary.browser}.`;
  }
  return `Imported the session from ${summary.browser}. It will be verified on the next refresh.`;
}
