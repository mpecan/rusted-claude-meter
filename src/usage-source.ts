/** Copy for the usage-source picker, in one place.
 *
 * Mirrors `src-tauri/src/source.rs` — that module's doc comment is the
 * authority on what each source can actually do, and these strings must not
 * promise more than it does. Kept out of `settings-view.ts` for the same
 * reason as `tos-notice.ts`: wording this load-bearing deserves tests, and a
 * second surface (the wizard) is a plausible next home for it.
 */

import type { SelectOption, UsageSource } from "./types";

/** The source choices, in display order. claude.ai first because it is the
 * default and the complete one. */
export const USAGE_SOURCE_OPTIONS: readonly SelectOption<UsageSource>[] = [
  { value: "claude_ai", label: "Poll claude.ai" },
  { value: "claude_code_statusline", label: "Read from Claude Code" },
];

/** What each source means, stated as a trade rather than a recommendation —
 * neither is simply better, and the honest difference is what the user needs
 * in order to choose. */
export function usageSourceHint(source: UsageSource): string {
  return source === "claude_code_statusline"
    ? "No claude.ai requests and no session key — so no Terms-of-Service risk. Reports only the 5-hour and 7-day windows, and only updates while Claude Code is running."
    : "The complete picture: model-scoped limits, spend, and updates on your refresh interval. Needs your session key, and the Terms-of-Service risk below.";
}

/** Whether the Terms-of-Service acknowledgement has any bearing on the
 * selected source. The statusline source originates no request, so the gate
 * neither blocks it nor is relevant to it — and saying so is better than
 * leaving a prominent warning that looks like it still applies. */
export function tosAppliesTo(source: UsageSource): boolean {
  return source !== "claude_code_statusline";
}

/** What the user types into Claude Code to have `/statusline` do the editing.
 *
 * Naming the file is the whole point: the agent behind `/statusline` can read
 * files and edit them and nothing else, so it cannot run the binary to learn
 * where the binary is. The app writes that file on every launch — see
 * `statusline::setup`.
 *
 * **The file name is mirrored in `crates/meter-files/src/statusline/setup.rs`**
 * (`SETUP_FILE`); rename them together or this points at nothing. */
export const STATUSLINE_SLASH_COMMAND =
  "/statusline add the Rusted Claude Meter usage segment exactly as described in ~/.claudemeter/statusline-command.txt";

/** Shown above the generated command. Names the constraint that forces the
 * component shape — one slot, one command — so "add this to yours" reads as
 * the design it is rather than a workaround. */
export const STATUSLINE_SETUP_INTRO =
  "Claude Code gives its status-line data to exactly one command, so this has to be added to whatever you already have rather than replacing it. The easiest way is to let Claude Code do the editing — run this in any session:";

/** Introduces the manual route, for anyone who would rather edit the file
 * themselves — or whose Claude Code predates `/statusline`. */
export const STATUSLINE_SETUP_MANUAL =
  'Or add it by hand: this command reads the input once, pipes a copy to the meter, and leaves the reading in $meter for your own line to print. It goes in "statusLine": { "type": "command", "command": … } in ~/.claude/settings.json.';

/** Said once, where a user will meet it: the source cannot report scoped
 * models, so the Model-scoped limits section has nothing to show. Better than
 * an empty list they would read as "no models found". */
export const STATUSLINE_NO_SCOPED_MODELS =
  "Model-scoped limits and the spend view need claude.ai — Claude Code does not report them.";

/** Said where the session field lives, when it cannot do anything. The field
 * is dimmed rather than removed for the same reason the Terms-of-Service
 * warning is: switching back must not feel like something vanished. */
export const STATUSLINE_NO_SESSION_KEY =
  "Not needed on this source — Claude Code reports usage without one, and the app makes no claude.ai request to validate it.";

/** Shown under the wizard's consent checkbox (issue #71).
 *
 * The step is a dead end for anyone unwilling to accept the risk: the
 * Continue button stays disabled and there is nothing else to press. Putting
 * the source choice first means the alternative is exactly one Back away, but
 * only if they know that — and the moment they are weighing the risk is the
 * moment it is worth knowing. Names the option rather than gesturing at
 * Settings, because the button that reaches it is right there. */
export const TOS_DECLINE_ALTERNATIVE =
  "Would rather not? Go back and choose \u201cRead from Claude Code\u201d instead — it needs no session key and carries no Terms-of-Service risk, but reports only the 5-hour and 7-day windows.";

/** The wizard's closing line, which differs by source because what is true at
 * that moment differs (issue #71).
 *
 * On claude.ai the meter is already working. On the status-line source it is
 * not: nothing appears until Claude Code next renders its status line, which
 * needs the command from the previous step to be in place *and* a Claude Code
 * session to run. Saying "now watching your usage" there would be a promise
 * the tray immediately fails to keep. */
export function wizardDoneSummary(source: UsageSource): string {
  return tosAppliesTo(source)
    ? "Rusted Claude Meter is now watching your usage from the tray."
    : "Rusted Claude Meter will start showing usage the next time Claude Code renders its status line — prompt it once and watch the tray. Until then it says it is waiting.";
}
