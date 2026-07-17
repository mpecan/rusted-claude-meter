import { describe, expect, it } from "vitest";

import { describeError } from "./ipc";
import type { SessionCommandError } from "./types";

describe("describeError", () => {
  it("reads the message off a SessionCommandError-shaped rejection", () => {
    const rejection: SessionCommandError = { kind: "Validation", message: "session key looks malformed" };
    expect(describeError(rejection)).toBe("session key looks malformed");
  });

  it("reads the message off a plain Error", () => {
    expect(describeError(new Error("network unreachable"))).toBe("network unreachable");
  });

  it("stringifies an arbitrary non-Error rejection", () => {
    expect(describeError("boom")).toBe("boom");
    expect(describeError(undefined)).toBe("undefined");
  });
});
