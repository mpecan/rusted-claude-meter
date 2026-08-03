// A text contract over the shipped `index.html`, not a DOM test: this suite
// asserts what the markup *says* before any script runs. Issue #77 owns the
// DOM tier, and adding a test environment for it here would collide with that
// work — so the file is read from disk and matched, the same idiom
// `styles.test.ts` uses for the stylesheet.
//
// What it guards is issue #78's accessibility half: the consent step's
// Continue button ships `disabled`, so the very first thing a screen-reader
// user meets is a control that is inert for a reason nothing states. The
// reason is a paragraph in the reading order that the button points at — and
// an `aria-describedby` naming an id that does not exist fails silently, with
// no console warning and no visible symptom, which is exactly the kind of
// mistake worth pinning.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const HTML = readFileSync(fileURLToPath(new URL("../index.html", import.meta.url)), "utf8");

const REASON_ID = "wizard-consent-blocked-reason";

describe("the consent step's disabled Continue", () => {
  it("ships described by the reason it is disabled", () => {
    // Both attributes on the same tag: `disabled` without the description is
    // the bug, and the description without `disabled` would mean the initial
    // markup and `syncConsent`'s initial state disagreed.
    const tag = /<button[^>]*id="wizard-consent-continue-button"[^>]*>/.exec(HTML);
    expect(tag).not.toBeNull();
    expect(tag?.[0]).toMatch(/\bdisabled\b/);
    expect(tag?.[0]).toMatch(new RegExp(`aria-describedby="${REASON_ID}"`));
  });

  it("points aria-describedby at an element that actually exists in the consent step", () => {
    // Exactly once, and inside the step it describes: a dangling reference is
    // announced as nothing at all, and a duplicate id makes which paragraph
    // gets read undefined.
    const declarations = [...HTML.matchAll(new RegExp(`id="${REASON_ID}"`, "g"))];
    expect(declarations).toHaveLength(1);
    const consentStep = /<div id="wizard-step-consent"[\s\S]*?\n {12}<\/div>/.exec(HTML);
    expect(consentStep).not.toBeNull();
    expect(consentStep?.[0]).toContain(`id="${REASON_ID}"`);
  });
});
