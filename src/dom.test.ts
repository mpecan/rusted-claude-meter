/** @vitest-environment jsdom */
// The two lookup helpers every view resolves its handles through. Small, but
// two other tests lean on their exact behaviour: the markup cross-check
// (`dom-contract.test.ts`) assumes a missing id is a *throw* rather than a
// silent null, and the double-mount test (`statusline-setup.test.ts`) assumes
// a role lookup is scoped to the container it was handed.

import { beforeEach, expect, it } from "vitest";

import { requireChild, requireElement } from "./dom";

beforeEach(() => {
  document.body.innerHTML = "";
});

it("names the id index.html is missing", () => {
  // Diagnosability is the reason this throws instead of returning null: a
  // renamed id has to fail at construction saying which id, not surface an
  // hour later as a button whose listener was never wired.
  expect(() => requireElement("nowhere-near-real")).toThrow("missing #nowhere-near-real");
});

it("names both the role and the container that lacks it", () => {
  const container = document.createElement("div");
  container.id = "some-block";
  document.body.append(container);

  // The container matters in the message: the same role legitimately exists
  // in several blocks, so "missing [data-role=command]" alone would not say
  // which mount is broken.
  expect(() => requireChild(container, "command")).toThrow(
    'missing [data-role="command"] inside #some-block',
  );
});

it("scopes a role lookup to the container it was given", () => {
  // The mechanism that lets the status-line block be mounted twice at once.
  // `getElementById` cannot do this — a duplicated id is invalid HTML and
  // would hand both mounts the first one's elements.
  document.body.innerHTML = `
    <div id="first"><span data-role="command">first command</span></div>
    <div id="second"><span data-role="command">second command</span></div>
  `;
  const first = requireElement<HTMLElement>("first");
  const second = requireElement<HTMLElement>("second");

  expect(requireChild(first, "command").textContent).toBe("first command");
  expect(requireChild(second, "command").textContent).toBe("second command");
});
