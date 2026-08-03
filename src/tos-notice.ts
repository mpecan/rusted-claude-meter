// The Terms-of-Service warning copy, in one place.
//
// It is shown twice — as a blocking step in the first-run wizard, and as the
// consent row in Settings — and the two must say exactly the same thing. A
// user who accepts the risk in the wizard and later re-reads it in Settings
// should not find a softer or a scarier version of what they agreed to, so
// the strings live here rather than being written out in `index.html` twice.
//
// Substance, not decoration: `docs/terms-of-service.md` holds the full
// analysis with the clauses quoted, and `DOCS_URL` links to it.

/** Where "Read the full explanation" goes. Points at the repo rather than a
 * bundled file so it stays reachable from the wizard, from Settings, and from
 * a browser someone lands in later. */
export const DOCS_URL =
  "https://github.com/mpecan/rusted-claude-meter/blob/main/docs/terms-of-service.md";

/** The headline. Deliberately states the conclusion rather than hedging into
 * "may potentially" — the plain reading of Consumer Terms §3 covers what this
 * app does, and a warning nobody takes seriously protects nobody. */
export const TOS_HEADLINE =
  "Using this app is likely a breach of Anthropic's Terms of Service.";

/** The body: what the app does, why that is the problem, and what is at
 * stake. Kept to three sentences — past that, people stop reading, which
 * defeats the point of the gate. */
export const TOS_BODY = [
  "There is no official API for Claude usage, so this app polls an internal claude.ai endpoint on a timer using your web session cookie, sending browser-shaped headers so it isn't turned away as an automated client.",
  "Anthropic's Consumer Terms §3 prohibit accessing the service “through automated or non-human means, whether through a bot, script, or otherwise” without an API key, and prohibit harvesting data from it.",
  "Anthropic enforces without prior notice, and the account at risk is yours.",
] as const;

/** The reassurance, which is true and belongs next to the warning: the risk is
 * real but bounded, and a user weighing it deserves both halves. */
export const TOS_MITIGATION =
  "Your session key never leaves this machine except to claude.ai, and the app runs no inference — it only reads counters. Every known enforcement action in 2026 targeted tools running inference on subscription credentials.";

/** The consent checkbox's label. Phrased as the user's own statement so that
 * ticking it is an act, not a dismissal. */
export const TOS_CONSENT_LABEL =
  "I understand the risk to my Claude account and want to track my usage anyway.";

/** Why the wizard's consent step has an inert Continue button (issue #78).
 *
 * A disabled button conveys *that* it is unavailable and never *why*; worse, a
 * disabled button is not focusable, so a reason attached to the button itself
 * (a `title`, or a description only reachable by tabbing to it) is announced to
 * nobody. This is therefore a visible paragraph in the reading order, shown and
 * referenced by `aria-describedby` only while it is true.
 *
 * Distinct from `usage-source.ts::TOS_DECLINE_ALTERNATIVE`, which is the way
 * *out* for someone unwilling to tick the box: this one names what unblocks the
 * step for someone who is willing but cannot see what the button wants. */
export const TOS_CONSENT_REQUIRED_HINT =
  "Continue stays unavailable until you tick the box above — there is no way on from here without it.";

/** What Settings shows under the checkbox once it is off — the state the app
 * is actually in, not a nag. */
export const TOS_PAUSED_HINT =
  "Tracking is paused. The app is making no requests to claude.ai at all.";

/** What Settings shows under the checkbox while it is on. */
export const TOS_ACTIVE_HINT =
  "Tracking is on. Untick this at any time to stop all claude.ai requests immediately.";

/** The hint under the consent row, keyed to the current state. Exported as a
 * function so the Settings/wizard call sites cannot disagree about which
 * string goes with which state.
 *
 * Two states, not three: the row is only ever on screen on the claude.ai
 * source, so "the question does not apply to your source" is a hint with
 * nowhere to render. It used to have a third — the section was dimmed rather
 * than hidden, and had to explain why it was still there. Hiding it outright
 * makes that explanation the answer to a question nobody is now looking at. */
export function tosStateHint(acknowledged: boolean): string {
  return acknowledged ? TOS_ACTIVE_HINT : TOS_PAUSED_HINT;
}
