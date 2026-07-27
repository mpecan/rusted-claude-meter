// The `hidden` attribute is the app's only mechanism for "show this only
// when …" — the session form, the wizard, the debug-log path row, the demo
// endpoint banner. Nothing else in the test suite exercises the stylesheet
// (there is no DOM in these tests), so the one CSS invariant that whole
// mechanism rests on is pinned here by reading the files directly.
// Read from disk rather than imported: vitest does not run the CSS pipeline, so
// `import "./styles.css?raw"` resolves to an empty string and every assertion
// below would pass vacuously.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const read = (relative: string): string =>
  readFileSync(fileURLToPath(new URL(relative, import.meta.url)), "utf8");

const CSS = read("./styles.css");
const HTML = read("../index.html");

/** Declaration bodies of every rule whose selector matches `predicate`. */
function rulesMatching(css: string, predicate: (selector: string) => boolean): string[] {
  const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const bodies: string[] = [];
  for (const [, selector, body] of withoutComments.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    if (predicate(selector.trim())) {
      bodies.push(body);
    }
  }
  return bodies;
}

/** Class names on every element in `html` that carries the `hidden` attribute.
 * Deliberately regex-level: the point is to read the shipped markup, and a
 * parser dependency for five tag matches is not worth it. */
function classesOnHiddenElements(html: string): Set<string> {
  const classes = new Set<string>();
  for (const tag of html.matchAll(/<[a-zA-Z][^>]*>/g)) {
    if (!/\bhidden\b(?![-\w])/.test(tag[0])) {
      continue;
    }
    const className = /class="([^"]*)"/.exec(tag[0]);
    for (const name of className?.[1].split(/\s+/) ?? []) {
      classes.add(name);
    }
  }
  return classes;
}

describe("the hidden attribute", () => {
  it("is neutralised globally, beating any author display rule", () => {
    // The UA stylesheet's `[hidden] { display: none }` is user-agent origin and
    // loses to every author rule that sets `display` on the same element, so
    // marking a flex container hidden does not hide it. This rule restores the
    // attribute's meaning for the whole app; `!important` is what makes it
    // independent of the specificity of whatever set `display`.
    const global = rulesMatching(CSS, (selector) => selector === "[hidden]");
    expect(global).toHaveLength(1);
    expect(global[0]).toMatch(/display:\s*none\s*!important/);
  });

  it("hides every element the shipped markup starts hidden", () => {
    // Guards the bug the global rule fixes rather than the rule itself: if it
    // is ever dropped or weakened, each class that sets its own `display` needs
    // its own `[hidden]` override again, and this says which ones.
    const globallyHidden = rulesMatching(CSS, (selector) => selector === "[hidden]").some((body) =>
      /display:\s*none\s*!important/.test(body),
    );
    const leaked: string[] = [];
    for (const className of classesOnHiddenElements(HTML)) {
      const selectorMatchesClass = new RegExp(`\\.${className}(?![-\\w])`);
      const setsDisplay = rulesMatching(
        CSS,
        (selector) => selectorMatchesClass.test(selector) && !selector.includes("[hidden]"),
      ).some((body) => /(^|;|\s)display\s*:/.test(body));
      const overridden = rulesMatching(CSS, (selector) =>
        new RegExp(`\\.${className}\\[hidden\\]`).test(selector),
      ).some((body) => /display:\s*none/.test(body));
      if (setsDisplay && !overridden && !globallyHidden) {
        leaked.push(className);
      }
    }
    expect(leaked).toEqual([]);
  });

  it("finds the classes it is meant to be checking", () => {
    // Both assertions above pass vacuously if the markup scan returns nothing —
    // a renamed attribute or a rewritten Settings page would silently disarm
    // them. `.demo-endpoint-banner` is the element that shipped visible-when-
    // hidden in 0.1.4, so its presence is the canary.
    const classes = classesOnHiddenElements(HTML);
    expect(classes.has("demo-endpoint-banner")).toBe(true);
    expect(classes.size).toBeGreaterThan(3);
  });
});
