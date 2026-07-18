import { describe, expect, it } from "vitest";

import {
  formatAge,
  formatCountdown,
  formatResetClock,
  roundPercent,
  shortDuration,
} from "./format";

describe("roundPercent", () => {
  it("rounds and clamps like meter_render::round_percent", () => {
    expect(roundPercent(41.5)).toBe(42);
    expect(roundPercent(130)).toBe(100);
    expect(roundPercent(-3)).toBe(0);
  });
});

describe("shortDuration", () => {
  it("covers every magnitude", () => {
    expect(shortDuration(30)).toBe("under 1m");
    expect(shortDuration(12 * 60)).toBe("12m");
    expect(shortDuration(2 * 3600 + 15 * 60)).toBe("2h 15m");
    expect(shortDuration(3 * 86_400 + 4 * 3600)).toBe("3d 4h");
    expect(shortDuration(-10)).toBe("under 1m");
  });
});

describe("formatCountdown", () => {
  const now = new Date("2026-07-17T12:00:00Z");

  it("reads 'resets in <duration>' while time remains", () => {
    const resetsAt = new Date(now.getTime() + (2 * 3600 + 15 * 60) * 1000);
    expect(formatCountdown(resetsAt, now)).toBe("resets in 2h 15m");
  });

  it("reads 'resets soon' just after the reset moment", () => {
    const resetsAt = new Date(now.getTime() - 5 * 1000);
    expect(formatCountdown(resetsAt, now)).toBe("resets soon");
  });

  it("reads '<duration> ago' well past the reset moment", () => {
    const resetsAt = new Date(now.getTime() - (2 * 86_400 + 3 * 3600) * 1000);
    expect(formatCountdown(resetsAt, now)).toBe("reset 2d 3h ago");
  });
});

describe("formatAge", () => {
  it("reads '<duration> ago'", () => {
    const now = new Date("2026-07-17T12:00:00Z");
    const fetchedAt = new Date(now.getTime() - 25 * 60 * 1000);
    expect(formatAge(fetchedAt, now)).toBe("25m ago");
  });
});

describe("formatResetClock", () => {
  // Locale/timezone are the runner's, so assert structure, not an exact
  // string: the time-only variant (5-hour card) carries a clock time but no
  // month; the date+time variant (weekly/scoped) carries a month too, and
  // never a year.
  const resetsAt = new Date("2026-07-19T11:30:00Z");

  it("time-only variant shows a time and no month or year", () => {
    const text = formatResetClock(resetsAt, true);
    expect(text).toMatch(/\d/);
    expect(text).not.toMatch(/2026/);
    // Some hour digit and a minute separator survive in every locale.
    expect(text).toContain(":");
  });

  it("date+time variant adds a date but still no year", () => {
    const timeOnly = formatResetClock(resetsAt, false);
    expect(timeOnly).not.toMatch(/2026/);
    // Strictly more information than the time-only form.
    expect(timeOnly.length).toBeGreaterThan(formatResetClock(resetsAt, true).length);
  });
});
