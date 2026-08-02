import { describe, expect, it } from "vitest";

import {
  STATUSLINE_MIN_CLAUDE_CODE,
  STATUSLINE_NO_SCOPED_MODELS,
  STATUSLINE_NO_SESSION_KEY,
  STATUSLINE_SETUP_INTRO,
  STATUSLINE_SETUP_MANUAL,
  STATUSLINE_SLASH_COMMAND,
  USAGE_SOURCE_OPTIONS,
  tosAppliesTo,
  usageSourceHint,
} from "./usage-source";

describe("the usage-source picker", () => {
  it("offers exactly the two sources the backend implements", () => {
    expect(USAGE_SOURCE_OPTIONS.map((option) => option.value)).toEqual([
      "claude_ai",
      "claude_code_statusline",
    ]);
  });

  it("lists claude.ai first, since it is the default and the complete one", () => {
    expect(USAGE_SOURCE_OPTIONS[0]?.value).toBe("claude_ai");
  });
});

describe("the source hints", () => {
  it("names what the status-line source costs the user, not only what it saves", () => {
    // The whole point of the picker is an honest trade. A hint that sold the
    // ToS-free source without naming its two real limits would be marketing.
    const hint = usageSourceHint("claude_code_statusline");
    expect(hint).toMatch(/5-hour and 7-day/);
    expect(hint).toMatch(/only updates while Claude Code is running/);
  });

  it("says the status-line source needs no key and carries no ToS risk", () => {
    const hint = usageSourceHint("claude_code_statusline");
    expect(hint).toMatch(/no session key/i);
    expect(hint).toMatch(/Terms-of-Service/);
  });

  it("says the claude.ai source needs the key and the risk", () => {
    const hint = usageSourceHint("claude_ai");
    expect(hint).toMatch(/session key/);
    expect(hint).toMatch(/Terms-of-Service risk/);
  });

  it("promises scoped limits and spend only where they exist", () => {
    expect(usageSourceHint("claude_ai")).toMatch(/model-scoped limits/i);
    expect(usageSourceHint("claude_code_statusline")).not.toMatch(/model-scoped/i);
  });
});

describe("whether the ToS question applies", () => {
  it("applies to claude.ai and not to the status line", () => {
    // Mirrors `source::UsageSource::is_statusline` — the two must agree, or
    // the UI would dim a warning the backend is still enforcing.
    expect(tosAppliesTo("claude_ai")).toBe(true);
    expect(tosAppliesTo("claude_code_statusline")).toBe(false);
  });
});

describe("the status-line setup copy", () => {
  it("explains why the command is added to an existing one rather than replacing it", () => {
    expect(STATUSLINE_SETUP_INTRO).toMatch(/exactly one command/);
    expect(STATUSLINE_SETUP_INTRO).toMatch(/rather than replacing it/);
  });

  it("names the variable the user has to put in their own line", () => {
    // Without this the pasted command records usage but shows nothing, and
    // the user has no way to know why.
    expect(STATUSLINE_SETUP_MANUAL).toMatch(/\$meter/);
  });

  it("says which file the manual route edits", () => {
    expect(STATUSLINE_SETUP_MANUAL).toMatch(/~\/\.claude\/settings\.json/);
    expect(STATUSLINE_SETUP_MANUAL).toMatch(/statusLine/);
  });

  it("pins the Claude Code version floor", () => {
    // Below this the payload has no `rate_limits` at all, and older builds
    // treat `statusline` as an unknown argument and launch the GUI.
    expect(STATUSLINE_MIN_CLAUDE_CODE).toBe("2.1.216");
  });

  it("hands /statusline the file that names this machine's binary path", () => {
    // The agent behind /statusline can read and edit files and nothing else,
    // so it cannot run the binary to find out where the binary is. Pointing
    // it at the written file is the entire mechanism.
    expect(STATUSLINE_SLASH_COMMAND).toMatch(/^\/statusline /);
    // Mirrors `statusline::setup::SETUP_FILE` — renaming one without the
    // other points the agent at a file that is not there.
    expect(STATUSLINE_SLASH_COMMAND).toContain("~/.claudemeter/statusline-command.txt");
  });

  it("offers the easy route before the manual one", () => {
    expect(STATUSLINE_SETUP_INTRO).toMatch(/easiest/i);
    expect(STATUSLINE_SETUP_MANUAL).toMatch(/^Or add it by hand/);
  });

  it("explains why the session field is dimmed instead of just disabling it", () => {
    // The backend refuses a paste on this source (`WrongSource`), so the copy
    // has to say why rather than leave a field that looks usable.
    expect(STATUSLINE_NO_SESSION_KEY).toMatch(/Not needed/i);
    expect(STATUSLINE_NO_SESSION_KEY).toMatch(/no claude\.ai request/);
  });

  it("explains the empty scoped-models list rather than leaving it bare", () => {
    expect(STATUSLINE_NO_SCOPED_MODELS).toMatch(/claude\.ai/);
    expect(STATUSLINE_NO_SCOPED_MODELS).toMatch(/does not report them/);
  });
});
