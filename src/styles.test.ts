// Read from disk, not imported: vitest does not run the CSS pipeline, so
// `import "./styles.css?raw"` resolves to an empty string and the assertion
// below would pass vacuously.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { expect, it } from "vitest";

const CSS = readFileSync(fileURLToPath(new URL("./styles.css", import.meta.url)), "utf8");

it("neutralises the hidden attribute globally", () => {
  // `[hidden]` is user-agent origin and loses to any author rule that sets
  // `display` on the same element, so a hidden flex container stays on screen
  // showing its unfilled slots — the 0.1.4 demo-endpoint banner. `!important`
  // is what makes one global rule enough, whatever a class's specificity.
  const rules = CSS.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/([^{}]+)\{([^{}]*)\}/g);
  const hidden = [...rules].filter(([, selector]) => selector.trim() === "[hidden]");
  expect(hidden).toHaveLength(1);
  expect(hidden[0][2]).toMatch(/display:\s*none\s*!important/);
});
