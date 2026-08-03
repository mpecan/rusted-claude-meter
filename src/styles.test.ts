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

/** Every top-level `selector { body }` pair, comments stripped. Naive by
 * design — it does not understand at-rule nesting, so every rule asserted on
 * below is deliberately top-level. */
function rules(): [string, string][] {
  return [...CSS.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/([^{}]+)\{([^{}]*)\}/g)].map(
    ([, selector, body]) => [selector.trim(), body],
  );
}

it("dims every disabled control and refuses the cursor on it", () => {
  // Issue #78: `disabled` had no styling at all, so the wizard's consent
  // Continue looked pressable and silently ignored the click. Keyed to the
  // element's state rather than to `.primary-button`/`.ghost-button` by name,
  // so the next control to grow a disabled state is right for free.
  const base = rules().filter(([selector]) => /(^|,)\s*button:disabled\s*(,|$)/.test(selector));
  expect(base).toHaveLength(1);
  const [selector, body] = base[0];
  expect(selector).toMatch(/input:disabled/);
  expect(selector).toMatch(/select:disabled/);
  expect(body).toMatch(/opacity:\s*var\(--inert-opacity\)/);
  expect(body).toMatch(/cursor:\s*not-allowed/);
});

it("dims disabled controls and not-applicable sections by the same token, not the same number twice", () => {
  // The two treatments say the same thing ("here, but not available to you"),
  // so they have to agree by construction. Two literal `0.55`s would agree
  // only until the first time either was tuned.
  const declarations = [...CSS.matchAll(/--inert-opacity:\s*([^;]+);/g)];
  expect(declarations).toHaveLength(1);
  const notApplicable = rules().filter(
    ([selector]) => selector === ".settings-section.not-applicable",
  );
  expect(notApplicable).toHaveLength(1);
  expect(notApplicable[0][1]).toMatch(/opacity:\s*var\(--inert-opacity\)/);
  expect(notApplicable[0][1]).not.toMatch(/opacity:\s*[\d.]/);
});

it("reaches the label wrapping a disabled checkbox, which :disabled alone cannot", () => {
  // "Launch at login" is text in the label, not in the input, so the autostart
  // toggle's visible half stays undimmed — reading as broken rather than busy
  // — unless the label is styled too. And because opacity multiplies through
  // nesting, the box inside a dimmed label has to be reset to 1 or it renders
  // at 0.55 x 0.55 ≈ 0.30.
  const label = rules().filter(([selector]) => selector === "label:has(input:disabled)");
  expect(label).toHaveLength(1);
  expect(label[0][1]).toMatch(/opacity:\s*var\(--inert-opacity\)/);
  expect(label[0][1]).toMatch(/cursor:\s*not-allowed/);
  const nested = rules().filter(
    ([selector]) => selector === "label:has(input:disabled) input:disabled",
  );
  expect(nested).toHaveLength(1);
  expect(nested[0][1]).toMatch(/opacity:\s*1/);
});

it("keeps the :has() selector out of the base disabled rule, so an engine without :has() still dims", () => {
  // An engine that does not know `:has()` (WebKitGTK < 2.42, which ships in
  // the Linux webview) invalidates the *whole* selector list it appears in.
  // Merged into the base rule, an old engine would lose the plain disabled
  // treatment as well — a much worse trade than losing the label dimming.
  const merged = rules().filter(
    ([selector]) => selector.includes(":has(") && /(^|,)\s*button:disabled\s*(,|$)/.test(selector),
  );
  expect(merged).toEqual([]);
});

it("styles selects in the wizard, not only in Settings", () => {
  // The source picker (issue #71) is the first select placed directly in a
  // wizard step rather than inside a `.settings-section` wrapper. Before this
  // selector it rendered with browser defaults — a regression invisible to
  // every other test, since nothing else reads the stylesheet.
  const rules = CSS.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/([^{}]+)\{([^{}]*)\}/g);
  const selectRules = [...rules].filter(([, selector]) => /select\s*$/.test(selector.trim()));
  expect(selectRules.some(([, selector]) => selector.includes(".wizard-step select"))).toBe(true);
});
