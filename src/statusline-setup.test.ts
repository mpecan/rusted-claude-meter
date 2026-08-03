/** @vitest-environment jsdom */
// The status-line block is the one component this app mounts twice at once —
// once in Settings, once in the wizard's status-line step — which is why it
// resolves its children by `data-role` within a container it is handed rather
// than by global id.
//
// That is invisible to any pure test: with `getElementById` both mounts
// compile, both type-check, and both quietly drive the *first* container's
// elements, so one block sits empty and the other shows a command that may
// not be the one its own backend reported. Only rendering both at once shows
// it, which is what this does — against the real `index.html`, so the two
// container ids and every role are the shipped ones.

import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { requireChild, requireElement } from "./dom";
import { flushAsync, mountIndexHtml } from "./dom-harness";
import { createBackend } from "./ipc";
import type { UsageBackend } from "./ipc";
import { createStatuslineSetup } from "./statusline-setup";
import {
  STATUSLINE_SETUP_INTRO,
  STATUSLINE_SETUP_MANUAL,
  STATUSLINE_SLASH_COMMAND,
} from "./usage-source";

/** The two containers `index.html` carries, in the order a user meets them. */
const SETTINGS_BLOCK = "statusline-setup";
const WIZARD_BLOCK = "wizard-statusline-setup";

/** The fake backend, with only the one method this block calls replaced —
 * cheaper and less brittle than a hand-written stand-in for the whole
 * forty-method interface. */
function backendCommanding(command: string): UsageBackend {
  const backend = createBackend();
  backend.statuslineCommand = (): Promise<string> => Promise.resolve(command);
  return backend;
}

function roleIn(containerId: string, role: string): HTMLElement {
  return requireChild(requireElement<HTMLElement>(containerId), role);
}

/** jsdom has no clipboard, and the copy buttons are half of what this block
 * does. `defineProperty` because `navigator.clipboard` is a getter. */
function stubClipboard(writeText: (text: string) => Promise<void>): void {
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
}

beforeEach(() => {
  mountIndexHtml();
});

afterEach(() => {
  vi.restoreAllMocks();
});

it("gives each mounted block its own elements when Settings and the wizard render at once", async () => {
  const settingsCommand = "settings-block-command";
  const wizardCommand = "wizard-block-command";
  const settings = createStatuslineSetup(backendCommanding(settingsCommand), SETTINGS_BLOCK);
  const wizard = createStatuslineSetup(backendCommanding(wizardCommand), WIZARD_BLOCK);

  settings.setVisible(true);
  wizard.setVisible(true);
  await flushAsync();

  // Distinct commands rather than one shared string on purpose: identical
  // text would pass even if both instances were writing to the same element.
  expect(roleIn(SETTINGS_BLOCK, "command").textContent).toBe(settingsCommand);
  expect(roleIn(WIZARD_BLOCK, "command").textContent).toBe(wizardCommand);
  expect(requireElement("statusline-setup").hidden).toBe(false);
  expect(requireElement("wizard-statusline-setup").hidden).toBe(false);
});

it("renders the same copy in both blocks", () => {
  createStatuslineSetup(backendCommanding("a"), SETTINGS_BLOCK);
  createStatuslineSetup(backendCommanding("b"), WIZARD_BLOCK);

  // Forking the block to show a subset in the wizard would be two versions of
  // copy `usage-source.ts` exists to keep singular — so both mounts must show
  // both routes, and every role must resolve inside each subtree.
  for (const block of [SETTINGS_BLOCK, WIZARD_BLOCK]) {
    expect(roleIn(block, "intro").textContent).toBe(STATUSLINE_SETUP_INTRO);
    expect(roleIn(block, "manual").textContent).toBe(STATUSLINE_SETUP_MANUAL);
    expect(roleIn(block, "slash-command").textContent).toBe(STATUSLINE_SLASH_COMMAND);
  }
});

it("reports a copy only in the block whose button was pressed", async () => {
  const copied: string[] = [];
  stubClipboard((text) => {
    copied.push(text);
    return Promise.resolve();
  });
  const settings = createStatuslineSetup(backendCommanding("settings-block-command"), SETTINGS_BLOCK);
  createStatuslineSetup(backendCommanding("wizard-block-command"), WIZARD_BLOCK);
  settings.setVisible(true);
  await flushAsync();

  (roleIn(SETTINGS_BLOCK, "copy-command") as HTMLButtonElement).click();
  await flushAsync();

  expect(copied).toEqual(["settings-block-command"]);
  expect(roleIn(SETTINGS_BLOCK, "copy-status").textContent).toBe("Copied.");
  // The other mount's status must stay silent — a shared element would have
  // it reporting a copy nobody asked it for.
  expect(roleIn(WIZARD_BLOCK, "copy-status").textContent).toBe("");
});

it("explains itself when the clipboard refuses", async () => {
  const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
  stubClipboard(() => Promise.reject(new Error("denied")));
  createStatuslineSetup(backendCommanding("settings-block-command"), SETTINGS_BLOCK);

  (roleIn(SETTINGS_BLOCK, "copy-slash-command") as HTMLButtonElement).click();
  await flushAsync();

  // A denied clipboard is a real outcome (no user gesture, a locked-down
  // WebView), and silence would read as a button that does nothing.
  expect(roleIn(SETTINGS_BLOCK, "copy-slash-status").textContent).toBe(
    "Couldn't copy — select the text and copy it.",
  );
  expect(consoleError).toHaveBeenCalled();
});

it("fetches the command once, however often the block is shown", async () => {
  const backend = backendCommanding("settings-block-command");
  const fetches = vi.spyOn(backend, "statuslineCommand");
  const settings = createStatuslineSetup(backend, SETTINGS_BLOCK);

  // The Settings block is shown and hidden every time the usage source is
  // switched; the command is the running executable's path and cannot change
  // while the app is open, so once is the contract.
  settings.setVisible(true);
  await flushAsync();
  settings.setVisible(false);
  settings.setVisible(true);
  await flushAsync();

  expect(fetches).toHaveBeenCalledTimes(1);
  expect(roleIn(SETTINGS_BLOCK, "command").textContent).toBe("settings-block-command");
});
