/** @vitest-environment jsdom */
// The setup wizard driven the way a user drives it — real `index.html`, real
// listeners, and the fake `UsageBackend` that `createBackend()` hands out
// outside a Tauri shell.
//
// `wizard-view-model.test.ts` already pins which steps each source walks; what
// it cannot see is whether `wizard.ts` wires that order to the buttons, or
// whether the markup still has the elements it resolves. Both are only
// observable by pressing things, which is what this does.
//
// The traces below are exact on purpose. Changing the step order or the
// indicator wording *should* fail here — read such a failure as "the flow
// changed", and the order it changed against lives in `wizard-view-model.ts`.

import type { MockInstance } from "vitest";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { requireChild, requireElement } from "./dom";
import { flushAsync, mountIndexHtml } from "./dom-harness";
import { createBackend } from "./ipc";
import type { UsageSource } from "./types";
import { wizardDoneSummary } from "./usage-source";
import { createWizard } from "./wizard";
import type { Wizard, WizardCallbacks } from "./wizard";

/** Every wizard callback as a spy — the wizard reports its changes to the
 * Settings panel through these, and a walk that fires none of them would have
 * left the panel stale. */
function spyCallbacks(): WizardCallbacks & Record<keyof WizardCallbacks, MockInstance> {
  return {
    onIconStyleChange: vi.fn(),
    onRefreshIntervalChange: vi.fn(),
    onTosAcknowledgedChange: vi.fn(),
    onUsageSourceChange: vi.fn(),
    onClose: vi.fn(),
  };
}

/** The wizard step on screen, by its short name. Exactly one is ever visible;
 * asserting that here means a broken `goToStep` shows up as a failure rather
 * than as a trace that happens to read plausibly. */
function visibleStep(): string {
  const shown = [...document.querySelectorAll<HTMLElement>(".wizard-step")].filter(
    (step) => !step.hidden,
  );
  expect(shown.map((step) => step.id)).toHaveLength(1);
  return shown[0].id.replace("wizard-step-", "");
}

/** One line of the trace issue #77 asks the flow to print. */
function frame(): string {
  return `${visibleStep()}: ${requireElement("wizard-step-indicator").textContent ?? ""}`;
}

function click(id: string): void {
  requireElement<HTMLButtonElement>(id).click();
}

/** Pick a source the way the user does — the wizard branches on the select's
 * `change` event, not on anything the test could set directly. */
function chooseSource(source: UsageSource): void {
  const select = requireElement<HTMLSelectElement>("wizard-usage-source-select");
  select.value = source;
  select.dispatchEvent(new Event("change"));
}

function setConsent(accepted: boolean): void {
  const box = requireElement<HTMLInputElement>("wizard-tos-consent");
  box.checked = accepted;
  box.dispatchEvent(new Event("change"));
}

/** Open the wizard and press "Get started", recording each frame. The two
 * path walks share this prefix so they differ only where the paths do. */
async function openToSourceStep(wizard: Wizard, trace: string[]): Promise<void> {
  wizard.open();
  // `open()` renders synchronously and then reconciles against stored
  // settings; both frames must have settled before the first one is read.
  await flushAsync();
  trace.push(frame());
  click("wizard-start-button");
  trace.push(frame());
}

/** Reach the consent step, which only exists on the claude.ai path. */
async function walkToConsent(wizard: Wizard): Promise<void> {
  await openToSourceStep(wizard, []);
  chooseSource("claude_ai");
  click("wizard-source-continue-button");
  await flushAsync();
}

let consoleError: MockInstance<typeof console.error>;

beforeEach(() => {
  mountIndexHtml();
  // Every fake backend method resolves, so anything logged here is a real
  // wiring fault. Captured rather than silenced-and-counted so a failure
  // prints what was logged.
  consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  vi.restoreAllMocks();
});

it("walks the Claude Code path in five steps and never asks for consent or a session key", async () => {
  const backend = createBackend();
  const callbacks = spyCallbacks();
  const wizard = createWizard(backend, callbacks);
  const trace: string[] = [];

  await openToSourceStep(wizard, trace);
  chooseSource("claude_code_statusline");
  trace.push(frame());
  click("wizard-source-continue-button");
  trace.push(frame());
  await flushAsync();
  click("wizard-statusline-continue-button");
  trace.push(frame());
  click("wizard-customize-continue-button");
  trace.push(frame());
  await flushAsync();

  expect(trace).toEqual([
    "welcome: Step 1 of 7",
    "source: Step 2 of 7",
    // The moment the source changes the path shortens, for whatever step is
    // on screen — the indicator must never promise steps it will not show.
    "source: Step 2 of 5",
    "statusline: Step 3 of 5",
    "customize: Step 4 of 5",
    "done: Step 5 of 5",
  ]);
  expect(callbacks.onUsageSourceChange).toHaveBeenCalledWith("claude_code_statusline");
  // Showing the status-line step is what fetches this machine's command.
  const block = requireElement<HTMLElement>("wizard-statusline-setup");
  expect(requireChild(block, "command").textContent).toBe(await backend.statuslineCommand());
  expect(requireElement("wizard-done-summary").textContent).toBe(
    wizardDoneSummary("claude_code_statusline"),
  );
  // The point of the branch: no credential was asked for, and no
  // Terms-of-Service risk was accepted on the way to a working meter.
  expect(await backend.sessionStatus()).toBe("absent");
  expect((await backend.getSettings()).tos_acknowledged).toBe(false);

  click("wizard-finish-button");
  await flushAsync();

  expect(requireElement("wizard-panel").hidden).toBe(true);
  expect(await backend.wizardShouldRun()).toBe(false);
  expect(callbacks.onClose).toHaveBeenCalledTimes(1);
  expect(consoleError.mock.calls).toEqual([]);
});

it("walks the claude.ai path in seven steps, with consent before the session key", async () => {
  const backend = createBackend();
  const callbacks = spyCallbacks();
  const wizard = createWizard(backend, callbacks);
  const trace: string[] = [];

  await openToSourceStep(wizard, trace);
  chooseSource("claude_ai");
  trace.push(frame());
  click("wizard-source-continue-button");
  trace.push(frame());
  setConsent(true);
  await flushAsync();
  click("wizard-consent-continue-button");
  trace.push(frame());
  await flushAsync();
  requireElement<HTMLInputElement>("wizard-session-input").value = "sk-ant-not-a-real-key";
  requireElement<HTMLFormElement>("wizard-session-form").dispatchEvent(
    new Event("submit", { cancelable: true }),
  );
  trace.push(frame());
  await flushAsync();
  click("wizard-validate-continue-button");
  trace.push(frame());
  click("wizard-customize-continue-button");
  trace.push(frame());
  await flushAsync();

  expect(trace).toEqual([
    "welcome: Step 1 of 7",
    "source: Step 2 of 7",
    "source: Step 2 of 7",
    // Consent is reached *before* a credential is ever requested — the whole
    // reason the source step was moved ahead of it (issue #71).
    "consent: Step 3 of 7",
    "session: Step 4 of 7",
    "validate: Step 5 of 7",
    "customize: Step 6 of 7",
    "done: Step 7 of 7",
  ]);
  expect(requireElement("wizard-validate-status").textContent).toContain("connected and verified");
  expect(await backend.sessionStatus()).toBe("present");
  expect(requireElement("wizard-done-summary").textContent).toBe(wizardDoneSummary("claude_ai"));
  expect(consoleError.mock.calls).toEqual([]);
});

it("keeps Continue disabled on the consent step until the risk is accepted", async () => {
  const wizard = createWizard(createBackend(), spyCallbacks());
  await walkToConsent(wizard);
  const continueButton = requireElement<HTMLButtonElement>("wizard-consent-continue-button");

  // The gate the whole Terms-of-Service design rests on: the checkbox is the
  // only way past this step, and every step after it contacts claude.ai.
  expect(continueButton.disabled).toBe(true);
  setConsent(true);
  expect(continueButton.disabled).toBe(false);
  setConsent(false);
  expect(continueButton.disabled).toBe(true);
});

it("persists the consent answer the moment it is given, not on Continue", async () => {
  const backend = createBackend();
  const callbacks = spyCallbacks();
  const wizard = createWizard(backend, callbacks);
  await walkToConsent(wizard);

  setConsent(true);
  await flushAsync();

  // A user who ticks the box and closes the wizard has still consented — and
  // one who unticks it has still withdrawn, without pressing anything else.
  expect((await backend.getSettings()).tos_acknowledged).toBe(true);
  expect(callbacks.onTosAcknowledgedChange).toHaveBeenLastCalledWith(true);

  setConsent(false);
  await flushAsync();

  expect((await backend.getSettings()).tos_acknowledged).toBe(false);
  expect(callbacks.onTosAcknowledgedChange).toHaveBeenLastCalledWith(false);
});

it("reopens on the source and consent the user last chose", async () => {
  const backend = createBackend();
  await backend.setUsageSource("claude_code_statusline");
  await backend.setTosAcknowledged(true);
  const wizard = createWizard(backend, spyCallbacks());

  wizard.open();
  await flushAsync();

  // "Step 1 of 5", not "Step 2 of …": reconciling with stored settings
  // re-renders the indicator for whatever step is *showing*, so the welcome
  // screen keeps its own number while already counting the shorter path.
  expect(frame()).toBe("welcome: Step 1 of 5");
  expect(requireElement<HTMLInputElement>("wizard-tos-consent").checked).toBe(true);
  expect(consoleError.mock.calls).toEqual([]);
});
