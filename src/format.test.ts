import { describe, expect, it } from "vitest";

import {
  describeRemaining,
  formatAge,
  formatCountdown,
  formatHitTime,
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

describe("describeRemaining", () => {
  // Exact port of upstream UsageLimit.resetDescription (minus its "in "
  // prefix); rounds up so it never understates how long is left.
  it("guards non-positive with 'now'", () => {
    expect(describeRemaining(0)).toBe("now");
    expect(describeRemaining(-5)).toBe("now");
  });

  it("rounds sub-hour up to whole minutes, floored at 1", () => {
    expect(describeRemaining(59)).toBe("1 minute");
    expect(describeRemaining(60)).toBe("1 minute");
    expect(describeRemaining(61)).toBe("2 minutes");
    expect(describeRemaining(3599)).toBe("60 minutes");
  });

  it("rounds sub-day up to whole hours at the hour boundary", () => {
    expect(describeRemaining(3600)).toBe("1 hour");
    expect(describeRemaining(3601)).toBe("2 hours");
    // One second shy of a day still reads in hours (matches upstream).
    expect(describeRemaining(86_399)).toBe("24 hours");
  });

  it("splits a day or more into days and residual hours", () => {
    expect(describeRemaining(86_400)).toBe("1 day");
    expect(describeRemaining(86_401)).toBe("1 day 1 hour");
    expect(describeRemaining(2 * 86_400 + 3 * 3600)).toBe("2 days 3 hours");
    expect(describeRemaining(2 * 86_400)).toBe("2 days");
  });
});

describe("formatHitTime", () => {
  // Locale/timezone are the runner's, so assert structure and the same-day vs
  // different-day split, not an exact string (mirrors formatResetClock above).
  const now = new Date("2026-07-17T12:00:00Z");

  it("shows a bare clock time (no month or year) when the hit lands today", () => {
    const text = formatHitTime(now, now);
    expect(text).toMatch(/\d/);
    expect(text).toContain(":");
    expect(text).not.toMatch(/2026/);
  });

  it("adds a date (but still no year) when the hit lands on another day", () => {
    const later = new Date(now.getTime() + 30 * 86_400 * 1000);
    const dated = formatHitTime(later, now);
    expect(dated).not.toMatch(/2026/);
    // Strictly more information than a same-day clock time.
    expect(dated.length).toBeGreaterThan(formatHitTime(now, now).length);
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
