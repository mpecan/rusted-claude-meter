/** The Settings block that gets the bridge into Claude Code's status line.
 *
 * Its own module rather than more of `settings-view.ts` for the file-size
 * gate, and because it is self-contained: it owns its elements, fetches the
 * command once, and exposes nothing but "show yourself or don't".
 *
 * Two routes, deliberately in this order. `/statusline` is the easy one — the
 * agent behind it reads `~/.claudemeter/statusline-command.txt` (written by
 * `statusline::setup`) and edits `~/.claude/settings.json`, so the user never
 * handles the path. The raw command is the fallback for anyone who would
 * rather edit the file themselves, or whose Claude Code is too old to have
 * the command.
 */

import type { UsageBackend } from "./ipc";
import { STATUSLINE_SLASH_COMMAND } from "./usage-source";
import { requireElement } from "./dom";

/** What the block exposes to the settings view. */
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

export function createStatuslineSetup(backend: UsageBackend): StatuslineSetup {
  const container = requireElement<HTMLElement>("statusline-setup");
  const slashCommandEl = requireElement<HTMLElement>("statusline-slash-command");
  const commandEl = requireElement<HTMLElement>("statusline-command");

  slashCommandEl.textContent = STATUSLINE_SLASH_COMMAND;

  wireCopy(
    requireElement<HTMLButtonElement>("copy-statusline-slash-command"),
    requireElement<HTMLElement>("copy-statusline-slash-status"),
    () => STATUSLINE_SLASH_COMMAND,
  );
  wireCopy(
    requireElement<HTMLButtonElement>("copy-statusline-command"),
    requireElement<HTMLElement>("copy-statusline-status"),
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
