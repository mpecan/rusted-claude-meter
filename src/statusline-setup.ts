/** The block that gets the bridge into Claude Code's status line.
 *
 * Its own module rather than more of `settings-view.ts` for the file-size
 * gate, and because it is self-contained: it owns its elements, its copy, and
 * fetches the command once, exposing nothing but "show yourself or don't".
 *
 * Two routes, deliberately in this order. `/statusline` is the easy one — the
 * agent behind it reads `~/.claudemeter/statusline-command.txt` (written by
 * `statusline::setup`) and edits `~/.claude/settings.json`, so the user never
 * handles the path. The raw command is the fallback for anyone who would
 * rather edit the file themselves, or whose Claude Code is too old to have
 * the command.
 *
 * **Mounted twice** — the Settings section and the first-run wizard's
 * status-line step (issue #71) — so it resolves its elements by `data-role`
 * within a container the caller names, rather than by global id. Both routes
 * appear in both places: the wizard is exactly where someone whose Claude
 * Code predates `/statusline` needs the fallback, and forking the block to
 * show a subset would be two versions of copy the module header exists to
 * keep singular.
 */

import type { UsageBackend } from "./ipc";
import { STATUSLINE_SETUP_INTRO, STATUSLINE_SETUP_MANUAL, STATUSLINE_SLASH_COMMAND } from "./usage-source";
import { requireChild, requireElement } from "./dom";

/** What the block exposes to its host view. */
export interface StatuslineSetup {
  /** Show or hide the whole block; showing it fetches the command once. */
  setVisible(visible: boolean): void;
}

/** Wire a copy button to a source of text, reporting success or failure in
 * `status`. Shared by both routes so neither can grow its own behaviour. */
function wireCopy(button: HTMLButtonElement, status: HTMLElement, text: () => string): void {
  button.addEventListener("click", () => {
    navigator.clipboard
      .writeText(text())
      .then(() => {
        status.textContent = "Copied.";
      })
      .catch((error: unknown) => {
        console.error("failed to copy to the clipboard", error);
        status.textContent = "Couldn't copy — select the text and copy it.";
      });
  });
}

/** Wire the block inside `containerId`, whose descendants carry the
 * `data-role` names below. */
export function createStatuslineSetup(backend: UsageBackend, containerId: string): StatuslineSetup {
  const container = requireElement<HTMLElement>(containerId);
  const commandEl = requireChild<HTMLElement>(container, "command");

  requireChild<HTMLElement>(container, "intro").textContent = STATUSLINE_SETUP_INTRO;
  requireChild<HTMLElement>(container, "manual").textContent = STATUSLINE_SETUP_MANUAL;
  requireChild<HTMLElement>(container, "slash-command").textContent = STATUSLINE_SLASH_COMMAND;

  wireCopy(
    requireChild<HTMLButtonElement>(container, "copy-slash-command"),
    requireChild<HTMLElement>(container, "copy-slash-status"),
    () => STATUSLINE_SLASH_COMMAND,
  );
  wireCopy(
    requireChild<HTMLButtonElement>(container, "copy-command"),
    requireChild<HTMLElement>(container, "copy-status"),
    () => commandEl.textContent ?? "",
  );

  /** Fetched lazily and only once: it is the running executable's path, which
   * cannot change while the app is open. */
  function loadCommand(): void {
    if (commandEl.textContent) {
      return;
    }
    backend
      .statuslineCommand()
      .then((command) => {
        commandEl.textContent = command;
      })
      .catch((error: unknown) => {
        console.error("failed to build the status-line command", error);
      });
  }

  return {
    setVisible(visible: boolean): void {
      container.hidden = !visible;
      if (visible) {
        loadCommand();
      }
    },
  };
}
