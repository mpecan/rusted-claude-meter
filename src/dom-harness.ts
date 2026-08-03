// Test-only, and imported by nothing the app ships: vite never bundles this
// module. It lives in `src/` so `tsc --noEmit` type-checks it alongside the
// code it drives.
//
// Its whole job is that the DOM tests assert against the markup that actually
// ships. A hand-written fixture would pass forever after `index.html` was
// edited, which is the failure the tests exist to catch — so the markup is
// read off disk, verbatim, exactly as `styles.test.ts` reads the stylesheet
// and for the same reason: vitest does not run vite's HTML pipeline, so there
// is nothing to `import`.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

/** This module's own directory.
 *
 * Resolved through `node:path` rather than the `new URL(relative,
 * import.meta.url)` idiom `styles.test.ts` uses, because under jsdom the
 * global `URL` is the document's: it discards a `file:` base and resolves
 * against `http://localhost:3000/`, so the same expression that works in the
 * node environment silently points at the wrong place here. */
const HERE = dirname(fileURLToPath(import.meta.url));

/** The shipped `index.html`, verbatim. */
export const INDEX_HTML = readFileSync(join(HERE, "..", "index.html"), "utf8");

/** `index.html` parsed into a document of its own, leaving the one the test
 * runs in alone. The markup cross-check queries this; the view tests install
 * it (see [`mountIndexHtml`]). */
export function parseIndexHtml(): Document {
  return new DOMParser().parseFromString(INDEX_HTML, "text/html");
}

/** Install the shipped markup into the running document.
 *
 * Call it from `beforeEach`, not once per file: jsdom gives each *file* a
 * fresh document, and every view resolves its element handles once at
 * construction and wires listeners onto them. Two tests sharing one document
 * would leave the second driving a form the first had already typed into,
 * with both sets of listeners still attached. */
export function mountIndexHtml(): void {
  document.body.innerHTML = parseIndexHtml().body.innerHTML;
}

/** Let every already-resolved promise chain finish.
 *
 * The views start backend calls at construction and on each step change and
 * hand the results to `.then`, so nothing they fetched is on screen until
 * those microtasks drain. A macrotask boundary drains all of them however
 * deep the chain is, which awaiting one promise does not. */
export function flushAsync(): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(() => resolve(), 0);
  });
}
