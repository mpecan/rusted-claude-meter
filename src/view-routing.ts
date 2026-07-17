// A single vite bundle serves both windows. Which view a window renders is
// decided purely from its Tauri window label, so the routing is a pure,
// testable function separate from the DOM wiring in `main.ts` (mirroring the
// project's render/logic split elsewhere).

/** The two views the bundle can render. */
export type AppView = "popover" | "settings";

/** The Settings window's label — must match
 * `src-tauri/src/settings_window.rs::SETTINGS_WINDOW_LABEL`. */
export const SETTINGS_WINDOW_LABEL = "settings";

/** Resolve which view a window with the given label should render. Only the
 * dedicated Settings window renders the settings view; every other label
 * (the `main` popover, and any fallback when the label can't be read outside
 * a Tauri shell) renders the popover. */
export function resolveView(label: string): AppView {
  return label === SETTINGS_WINDOW_LABEL ? "settings" : "popover";
}
