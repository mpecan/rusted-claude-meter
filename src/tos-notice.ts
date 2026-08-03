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

/** What Settings shows under the checkbox once it is off — the state the app
 * is actually in, not a nag. */
export const TOS_PAUSED_HINT =
  "Tracking is paused. The app is making no requests to claude.ai at all.";

/** What Settings shows under the checkbox while it is on. */
export const TOS_ACTIVE_HINT =
  "Tracking is on. Untick this at any time to stop all claude.ai requests immediately.";

/** What Settings shows when the question does not bear on the chosen source
 * at all (issue #71).
 *
 * A third state rather than a fork of the two above, because the two above
 * are both *wrong* here and in opposite directions: "Tracking is paused"
 * describes a dead meter to someone whose meter is working, and "Tracking is
 * on" would read as consent granted by someone who never gave it. The section
 * is dimmed rather than hidden, so it also has to say why it is still on
 * screen and when it will matter again. */
export const TOS_NOT_APPLICABLE_HINT =
  "Doesn't apply while you're reading from Claude Code — the app makes no claude.ai requests at all on that source. This question comes back if you switch to polling claude.ai.";

/** The hint under the consent row, keyed to the current state. Exported as a
 * function so the Settings/wizard call sites cannot disagree about which
 * string goes with which state.
 *
 * `applies` is `usage-source.ts::tosAppliesTo` — passed in rather than read
 * here so this module keeps knowing nothing about sources, and so there stays
 * exactly one spelling of that predicate. */
export function tosStateHint(acknowledged: boolean, applies: boolean): string {
  if (!applies) {
    return TOS_NOT_APPLICABLE_HINT;
  }
  return acknowledged ? TOS_ACTIVE_HINT : TOS_PAUSED_HINT;
}
