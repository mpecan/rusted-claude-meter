import { describe, expect, it } from "vitest";

import { SETTINGS_WINDOW_LABEL, resolveView } from "./view-routing";

describe("resolveView", () => {
  it("routes the settings window label to the settings view", () => {
    expect(resolveView(SETTINGS_WINDOW_LABEL)).toBe("settings");
    expect(resolveView("settings")).toBe("settings");
  });

  it("routes the main window label to the popover view", () => {
    expect(resolveView("main")).toBe("popover");
  });

  it("falls back to the popover view for any unknown label", () => {
    // Outside a Tauri shell the label can't be read and defaults to a
    // non-settings value; that must land on the popover, never settings.
    expect(resolveView("")).toBe("popover");
    expect(resolveView("whatever")).toBe("popover");
  });
});
