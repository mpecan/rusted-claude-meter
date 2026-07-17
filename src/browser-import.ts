// Pure helpers for the browser-session-import UI (issue #10). Kept DOM-free
// and backend-free so it stays trivially testable; the DOM rendering lives in
// `browser-import-render.ts` and the event/IPC wiring in `main.ts`.

import type { ImportSummary } from "./types";

/** Human confirmation for a completed import. A key claude.ai has confirmed
 * reads differently from one stored but not yet verified (claude.ai was
 * unreachable at import time — the scheduler verifies it on the next poll). */
export function describeImportSummary(summary: ImportSummary): string {
  if (summary.validated) {
    return `Imported and verified the session from ${summary.browser}.`;
  }
  return `Imported the session from ${summary.browser}. It will be verified on the next refresh.`;
}
