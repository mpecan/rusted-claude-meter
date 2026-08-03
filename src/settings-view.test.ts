/** @vitest-environment jsdom */
// The Settings panel driven the way a user drives it — real `index.html`, real
// listeners, and the fake `UsageBackend` that `createBackend()` hands out
// outside a Tauri shell.
//
// `settings-view-model.test.ts` already pins which way every source-dependent
// flag points; what it cannot see is whether `applyUsageSourceToForm` puts
// them on the right elements. Getting `claudeAiSectionsHidden` onto the wrong
// section — or forgetting a control in `claudeAiOnlyControls` — leaves a live
// Terms-of-Service consent box in front of someone whose source cannot use it,
// and that is only observable by changing the select and looking.

import { beforeEach, expect, it } from "vitest";

import { requireElement } from "./dom";
import { flushAsync, mountIndexHtml } from "./dom-harness";
import { createBackend } from "./ipc";
import { initSettingsView } from "./settings-view";
import type { UsageSource } from "./types";

/** Pick a source the way the user does — the panel branches on the select's
 * `change` event, not on anything the test could assign directly. */
async function chooseSource(source: UsageSource): Promise<void> {
  const select = requireElement<HTMLSelectElement>("usage-source-select");
  select.value = source;
  select.dispatchEvent(new Event("change"));
  await flushAsync();
}

/** The two sections that exist only to serve claude.ai traffic. */
function claudeAiSections(): HTMLElement[] {
  return [
    requireElement<HTMLElement>("settings-tos-section"),
    requireElement<HTMLElement>("settings-session-section"),
  ];
}

/** Every control the user could act on inside those sections. Queried off the
 * live markup rather than restating `claudeAiOnlyControls`, so a control added
 * to either section and forgotten in that list fails here. */
function claudeAiControls(): HTMLInputElement[] {
  return claudeAiSections().flatMap((section) => [
    ...section.querySelectorAll<HTMLInputElement>("input, button"),
  ]);
}

beforeEach(async () => {
  mountIndexHtml();
  initSettingsView(createBackend());
  await flushAsync();
});

it("shows the Terms-of-Service and Session sections on the claude.ai source", async () => {
  await chooseSource("claude_ai");
  for (const section of claudeAiSections()) {
    expect(section.hidden).toBe(false);
  }
  expect(requireElement<HTMLInputElement>("settings-tos-consent").disabled).toBe(false);
});

it("hides both sections and disables every control in them on the Claude Code source", async () => {
  await chooseSource("claude_code_statusline");
  for (const section of claudeAiSections()) {
    expect(section.hidden).toBe(true);
  }
  // The consent checkbox by name, because it is the one whose being live would
  // actually matter — ticking it opens the gate that lets claude.ai be polled.
  expect(requireElement<HTMLInputElement>("settings-tos-consent").disabled).toBe(true);
  const live = claudeAiControls().filter((control) => !control.disabled);
  expect(live.map((control) => control.id || control.type)).toEqual([]);
});

it("brings them back, still enabled, when the user switches back", async () => {
  // The stored acknowledgement is deliberately untouched by the round trip, so
  // this must not come back as a fresh, un-answered question.
  await chooseSource("claude_code_statusline");
  await chooseSource("claude_ai");
  for (const section of claudeAiSections()) {
    expect(section.hidden).toBe(false);
  }
  expect(claudeAiControls().filter((control) => control.disabled)).toEqual([]);
});

it("shows the status-line setup block on exactly the source that hides the others", async () => {
  // The flags disagree on purpose; this is the pairing that would break if
  // someone applied one boolean to all of them.
  await chooseSource("claude_code_statusline");
  expect(requireElement<HTMLElement>("statusline-setup").hidden).toBe(false);
  expect(requireElement<HTMLElement>("settings-tos-section").hidden).toBe(true);

  await chooseSource("claude_ai");
  expect(requireElement<HTMLElement>("statusline-setup").hidden).toBe(true);
  expect(requireElement<HTMLElement>("settings-tos-section").hidden).toBe(false);
});
