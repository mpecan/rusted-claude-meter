import { describe, expect, it } from "vitest";

import {
  DOCS_URL,
  TOS_ACTIVE_HINT,
  TOS_BODY,
  TOS_CONSENT_LABEL,
  TOS_HEADLINE,
  TOS_MITIGATION,
  TOS_NOT_APPLICABLE_HINT,
  TOS_PAUSED_HINT,
  tosStateHint,
} from "./tos-notice";

describe("the ToS warning copy", () => {
  it("names the terms it is about", () => {
    // The headline has to be specific enough to act on. "Please be careful"
    // would pass a length check and tell the user nothing.
    expect(TOS_HEADLINE).toMatch(/Terms of Service/);
    expect(TOS_HEADLINE).toMatch(/Anthropic/);
  });

  it("states the conclusion rather than only raising a possibility", () => {
    // Guards the decision recorded in the module header: the plain reading of
    // §3 covers this app, so the copy says "likely a breach" and not "might
    // conceivably". If someone softens it, this fails and they have to think
    // about why.
    expect(TOS_HEADLINE).toMatch(/likely a breach/i);
  });

  it("explains the mechanism, the clause, and the stake", () => {
    const body = TOS_BODY.join(" ");
    expect(body).toMatch(/session cookie/);
    expect(body).toMatch(/automated or non-human means/);
    expect(body).toMatch(/without prior notice/);
    expect(body).toMatch(/your/i);
  });

  it("says what the app does not do, so the risk can be weighed honestly", () => {
    expect(TOS_MITIGATION).toMatch(/never leaves this machine/);
    expect(TOS_MITIGATION).toMatch(/no inference/);
  });

  it("phrases consent as the user's own statement", () => {
    expect(TOS_CONSENT_LABEL).toMatch(/^I understand/);
    expect(TOS_CONSENT_LABEL).toMatch(/risk/);
  });

  it("links to the full write-up", () => {
    expect(DOCS_URL).toMatch(/^https:\/\/github\.com\//);
    expect(DOCS_URL).toMatch(/terms-of-service\.md$/);
  });

  it("describes the state the app is actually in", () => {
    expect(tosStateHint(false, true)).toBe(TOS_PAUSED_HINT);
    expect(tosStateHint(true, true)).toBe(TOS_ACTIVE_HINT);
    // The off state must claim no traffic, because that is what the gate
    // actually enforces (`consent.rs` / `transport.rs`) — the copy and the
    // behaviour are one promise.
    expect(TOS_PAUSED_HINT).toMatch(/no requests/i);
    expect(TOS_ACTIVE_HINT).toMatch(/immediately/);
  });

  it("says the question does not apply, rather than that tracking is paused", () => {
    // The bug issue #71 names: on the Claude Code source the meter is working
    // and no consent was given, so both of the other two hints are wrong —
    // one describes a dead meter, the other implies an answer never given.
    expect(tosStateHint(false, false)).toBe(TOS_NOT_APPLICABLE_HINT);
    expect(tosStateHint(true, false)).toBe(TOS_NOT_APPLICABLE_HINT);
    expect(TOS_NOT_APPLICABLE_HINT).not.toMatch(/paused/i);
  });

  it("tells the user when the question will matter again, since the row stays on screen", () => {
    expect(TOS_NOT_APPLICABLE_HINT).toMatch(/switch/i);
    expect(TOS_NOT_APPLICABLE_HINT).toMatch(/claude\.ai/);
  });
});
