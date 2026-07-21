// The frontend's only I/O boundary: current state on demand, live updates
// via the `usage-state` event, and the session-key command. The frontend
// owns no polling — every value here either comes from Rust or is a
// client-side recomputation of a value Rust already sent (see pacing.ts /
// format.ts).
//
// Outside a Tauri shell (`npm run dev` without `tauri dev`) `isTauri()` is
// false and `createBackend` returns a demo backend instead, so the UI can be
// developed and screenshotted in a plain browser.

import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { DEMO_ALLOWANCE_WITH_COST_STATE, DEMO_COST_STATE, DEMO_STATE } from "./demo";
import { DEFAULT_SETTINGS } from "./settings-view-model";
import type {
  AppSettings,
  Browser,
  DetectedBrowser,
  IconPreview,
  IconStyle,
  ImportSummary,
  MeterState,
  PopoverLayout,
  RefreshInterval,
  SessionCommandError,
  SessionStatus,
  SessionSubmission,
  UsageMode,
} from "./types";

const USAGE_STATE_EVENT = "usage-state";
/** Mirrors `commands::SETTINGS_CHANGED_EVENT`: broadcast whenever settings a
 * different window renders change. The popover subscribes so a model-visibility
 * toggle made in the Settings window re-filters its cards live, now that the
 * two live in separate windows and no longer share one `settings` object. */
export const SETTINGS_CHANGED_EVENT = "settings-changed";

export interface UsageBackend {
  /** The state as of right now, for the initial render before the first
   * `usage-state` event arrives. */
  initialState(): Promise<MeterState>;
  /** Subscribe to every subsequent state broadcast. Returns an unsubscribe
   * function. */
  onStateChange(callback: (state: MeterState) => void): () => void;
  /** Open (or focus) the dedicated Settings window — the popover's Settings
   * button. Resolves once the request is delivered; the window itself renders
   * asynchronously. */
  openSettingsWindow(): Promise<void>;
  /** Subscribe to the `settings-changed` broadcast (the full [`AppSettings`]),
   * so a window can re-render when another window changes a shared setting.
   * Returns an unsubscribe function. */
  onSettingsChanged(callback: (settings: AppSettings) => void): () => void;
  /** Parse, store and validate a pasted session key against claude.ai, with
   * rollback on rejection — the same guarantee browser import gives an
   * imported key. Resolves with whether claude.ai confirmed it (`validated:
   * false` means stored but unverified because claude.ai was unreachable).
   * Rejects with a `SessionCommandError`-shaped value on failure. Shared by
   * the popover field, the Settings field and the wizard's paste step. */
  submitSessionKey(input: string): Promise<SessionSubmission>;
  /** Ask for a refresh now (TTL-guarded on the Rust side). */
  refreshUsage(): Promise<void>;
  /** Whether a session key is currently stored, without exposing it. */
  sessionStatus(): Promise<SessionStatus>;
  /** Remove the stored session key. */
  clearSessionKey(): Promise<void>;
  /** List the browsers a claude.ai session can be imported from on this
   * platform, with the permission story each implies. */
  listBrowserSessions(): Promise<DetectedBrowser[]>;
  /** Import the claude.ai session from a browser: read it, store it, and
   * validate it. Rejects with a `BrowserImportError`-shaped value on failure. */
  importBrowserSession(browser: Browser): Promise<ImportSummary>;
  /** The persisted settings, for the Settings panel's initial render. */
  getSettings(): Promise<AppSettings>;
  /** Replace the opt-in set of scoped-model display names to show. */
  setShownScopedModels(models: string[]): Promise<void>;
  /** Change the polling cadence. */
  setRefreshInterval(interval: RefreshInterval): Promise<void>;
  /** Change the tray icon style. */
  setIconStyle(style: IconStyle): Promise<void>;
  /** Rendered previews of every icon style for the visual picker. */
  iconStylePreviews(): Promise<IconPreview[]>;
  /** Toggle monochrome/template tray artwork. */
  setMonochrome(monochrome: boolean): Promise<void>;
  /** Update the notification thresholds; resolves with the clamped values. */
  setThresholds(warning: number, critical: number): Promise<AppSettings>;
  /** Toggle the extra "limit reset" notification. */
  setNotifyOnReset(enabled: boolean): Promise<AppSettings>;
  /** Fire a one-off test notification right now, bypassing the scheduler and
   * dedup tracker, so the user can confirm banners reach them. Resolves with
   * whether the OS accepted the send (`false` = likely suppressed: Focus mode,
   * denied authorization, or no notification daemon). */
  sendTestNotification(): Promise<boolean>;
  /** Toggle whether cards show the exact reset wall-clock time (PR #26). */
  setShowResetTime(enabled: boolean): Promise<AppSettings>;
  /** Switch the popover layout (redesign 1a rows / 1c cards). */
  setPopoverLayout(layout: PopoverLayout): Promise<AppSettings>;
  /** Change how many days of the week the weekly quota is paced over (5/6/7,
   * issue #16); resolves with the clamped settings. */
  setWeeklyPaceDays(days: number): Promise<AppSettings>;
  /** Toggle pace-first display mode — lead with the pace ratio instead of the
   * raw quota percentage (issue #16). Resolves with the resulting settings. */
  setPaceFirstDisplay(enabled: boolean): Promise<AppSettings>;
  /** Master switch for the whole pace-tracking feature (issue #16). */
  setPaceTrackingEnabled(enabled: boolean): Promise<AppSettings>;
  /** Switch the usage view mode (Auto / Allowance / Cost). Resolves with the
   * resulting settings. */
  setUsageMode(mode: UsageMode): Promise<AppSettings>;
  /** Toggle debug logging of raw API responses to a local file. Resolves with
   * the resulting settings. */
  setDebugLogging(enabled: boolean): Promise<AppSettings>;
  /** The absolute path of the API-response log for display, or `null` when no
   * log location could be resolved. */
  debugLogPath(): Promise<string | null>;
  /** Reveal the API-response log in the OS file manager (or its folder when
   * nothing has been logged yet). */
  revealDebugLog(): Promise<void>;
  /** Resize the popover to the given content height (macOS binds the NSPopover
   * to it; a no-op elsewhere). Width is fixed. */
  setPopoverHeight(height: number): Promise<void>;
  /** Whether the setup wizard (issue #11) should open automatically on this
   * launch — `settings.json` did not exist before this launch loaded it. */
  wizardShouldRun(): Promise<boolean>;
  /** Record that the first-run wizard has been auto-opened this process, so a
   * rebuild of the destroy-on-close Settings window does not re-trigger it.
   * Independent of `wizardComplete` — it must fire even when the user skips. */
  wizardMarkOffered(): Promise<void>;
  /** Mark the wizard complete by writing settings to disk even if nothing
   * changed, so "absence of settings" stops being true on the next launch. */
  wizardComplete(): Promise<void>;
  /** Whether this Linux session is GNOME, which hides the tray unless the
   * AppIndicator extension is installed. Always `false` off Linux. */
  isGnomeDesktop(): Promise<boolean>;
  /** Whether launch-at-login is currently registered with the OS (issue
   * #12). Queried fresh every call — never cached — because the
   * registration can be flipped from outside the app (System Settings on
   * macOS, a user editing the XDG autostart entry on Linux). */
  autostartStatus(): Promise<boolean>;
  /** Enable or disable launch-at-login, and resolve with the resulting
   * registration state so the toggle can reconcile itself against whatever
   * actually happened rather than just echoing `enabled` back. */
  setAutostart(enabled: boolean): Promise<boolean>;
}

class TauriBackend implements UsageBackend {
  initialState(): Promise<MeterState> {
    return invoke<MeterState>("usage_state");
  }

  onStateChange(callback: (state: MeterState) => void): () => void {
    return subscribe(USAGE_STATE_EVENT, callback);
  }

  openSettingsWindow(): Promise<void> {
    return invoke<void>("open_settings_window");
  }

  onSettingsChanged(callback: (settings: AppSettings) => void): () => void {
    return subscribe(SETTINGS_CHANGED_EVENT, callback);
  }

  submitSessionKey(input: string): Promise<SessionSubmission> {
    return invoke<SessionSubmission>("set_session_key", { input });
  }

  refreshUsage(): Promise<void> {
    return invoke<void>("refresh_usage");
  }

  sessionStatus(): Promise<SessionStatus> {
    return invoke<SessionStatus>("session_status");
  }

  clearSessionKey(): Promise<void> {
    return invoke<void>("clear_session_key");
  }

  listBrowserSessions(): Promise<DetectedBrowser[]> {
    return invoke<DetectedBrowser[]>("list_browser_sessions");
  }

  importBrowserSession(browser: Browser): Promise<ImportSummary> {
    return invoke<ImportSummary>("import_browser_session", { browser });
  }

  getSettings(): Promise<AppSettings> {
    return invoke<AppSettings>("get_settings");
  }

  setShownScopedModels(models: string[]): Promise<void> {
    return invoke<void>("set_shown_scoped_models", { models });
  }

  setRefreshInterval(interval: RefreshInterval): Promise<void> {
    return invoke<void>("set_refresh_interval", { interval });
  }

  setIconStyle(style: IconStyle): Promise<void> {
    return invoke<void>("set_icon_style", { style });
  }

  iconStylePreviews(): Promise<IconPreview[]> {
    return invoke<IconPreview[]>("icon_style_previews");
  }

  setMonochrome(monochrome: boolean): Promise<void> {
    return invoke<void>("set_monochrome", { monochrome });
  }

  setThresholds(warning: number, critical: number): Promise<AppSettings> {
    return invoke<AppSettings>("set_thresholds", { warning, critical });
  }

  setNotifyOnReset(enabled: boolean): Promise<AppSettings> {
    return invoke<AppSettings>("set_notify_on_reset", { enabled });
  }

  sendTestNotification(): Promise<boolean> {
    return invoke<boolean>("send_test_notification");
  }

  setShowResetTime(enabled: boolean): Promise<AppSettings> {
    return invoke<AppSettings>("set_show_reset_time", { enabled });
  }

  setPopoverLayout(layout: PopoverLayout): Promise<AppSettings> {
    return invoke<AppSettings>("set_popover_layout", { layout });
  }

  setWeeklyPaceDays(days: number): Promise<AppSettings> {
    return invoke<AppSettings>("set_weekly_pace_days", { days });
  }

  setPaceFirstDisplay(enabled: boolean): Promise<AppSettings> {
    return invoke<AppSettings>("set_pace_first_display", { enabled });
  }

  setPaceTrackingEnabled(enabled: boolean): Promise<AppSettings> {
    return invoke<AppSettings>("set_pace_tracking_enabled", { enabled });
  }

  setUsageMode(mode: UsageMode): Promise<AppSettings> {
    return invoke<AppSettings>("set_usage_mode", { mode });
  }

  setDebugLogging(enabled: boolean): Promise<AppSettings> {
    return invoke<AppSettings>("set_debug_logging", { enabled });
  }

  debugLogPath(): Promise<string | null> {
    return invoke<string | null>("debug_log_path");
  }

  revealDebugLog(): Promise<void> {
    return invoke<void>("reveal_debug_log");
  }

  setPopoverHeight(height: number): Promise<void> {
    return invoke<void>("set_popover_height", { height });
  }

  wizardShouldRun(): Promise<boolean> {
    return invoke<boolean>("wizard_should_run");
  }

  wizardMarkOffered(): Promise<void> {
    return invoke<void>("wizard_mark_offered");
  }

  wizardComplete(): Promise<void> {
    return invoke<void>("wizard_complete");
  }

  isGnomeDesktop(): Promise<boolean> {
    return invoke<boolean>("is_gnome_desktop");
  }

  autostartStatus(): Promise<boolean> {
    return invoke<boolean>("autostart_status");
  }

  setAutostart(enabled: boolean): Promise<boolean> {
    return invoke<boolean>("set_autostart", { enabled });
  }
}

/** Subscribe to a Tauri event, tolerating an unsubscribe requested before
 * `listen`'s promise resolves (mirrors the previous inline `onStateChange`
 * implementation, now shared by every event this module subscribes to). */
function subscribe<T>(event: string, callback: (payload: T) => void): () => void {
  let unlisten: (() => void) | undefined;
  let cancelled = false;
  listen<T>(event, (e) => callback(e.payload)).then((fn) => {
    if (cancelled) {
      fn();
    } else {
      unlisten = fn;
    }
  });
  return () => {
    cancelled = true;
    unlisten?.();
  };
}

/** In-memory backend serving the demo fixture, for development outside
 * Tauri. `submitSessionKey` never fails so the CTA form path is also
 * reachable without a real backend. Settings start from
 * `DEFAULT_SETTINGS` — opted out of every scoped model, same as a real
 * fresh install — so the opt-in toggle is exercisable in `npm run dev` too. */
/** The `?mode=` preview override, when it names a real usage mode. */
function demoModeOverride(): UsageMode | null {
  const mode = new URLSearchParams(window.location.search).get("mode");
  return mode === "auto" || mode === "allowance" || mode === "cost" ? mode : null;
}

/** Dev/design preview overrides read from the URL (browser only). When a
 * `?layout=` or `?mode=` is present we also opt into the demo's scoped models
 * so the preview shows a fuller set of meters. */
function demoSettingOverrides(): Partial<AppSettings> {
  const overrides: Partial<AppSettings> = {};
  const layout = new URLSearchParams(window.location.search).get("layout");
  if (layout === "rows" || layout === "cards") {
    overrides.popover_layout = layout;
  }
  const mode = demoModeOverride();
  if (mode !== null) {
    overrides.usage_mode = mode;
  }
  if (Object.keys(overrides).length > 0) {
    overrides.shown_scoped_models = ["Fable", "Sonnet"];
  }
  return overrides;
}

/** The demo snapshot for the current preview: a token/cost account for
 * `?mode=cost`, an allowance account carrying spend (so the cost-summary card
 * shows) for `?mode=allowance`, otherwise the plain allowance fixture. */
function demoState(): MeterState {
  switch (demoModeOverride()) {
    case "cost":
      return DEMO_COST_STATE;
    case "allowance":
      return DEMO_ALLOWANCE_WITH_COST_STATE;
    default:
      return DEMO_STATE;
  }
}

class DemoBackend implements UsageBackend {
  // Dev/design preview: outside Tauri a `?layout=rows|cards` query seeds the
  // popover layout and a `?mode=auto|allowance|cost` query seeds the usage
  // view (and its matching demo snapshot) so every direction can be captured
  // in a plain browser.
  private settings: AppSettings = { ...DEFAULT_SETTINGS, ...demoSettingOverrides() };
  private sessionPresent = false;
  private wizardCompleted = false;
  private autostartEnabled = false;

  initialState(): Promise<MeterState> {
    return Promise.resolve(demoState());
  }

  onStateChange(): () => void {
    return () => {};
  }

  openSettingsWindow(): Promise<void> {
    // No second window outside a Tauri shell; the demo renders the popover
    // only. A no-op keeps the Settings button harmless in `npm run dev`.
    return Promise.resolve();
  }

  onSettingsChanged(): () => void {
    return () => {};
  }

  submitSessionKey(): Promise<SessionSubmission> {
    this.sessionPresent = true;
    return Promise.resolve({ validated: true });
  }

  refreshUsage(): Promise<void> {
    return Promise.resolve();
  }

  sessionStatus(): Promise<SessionStatus> {
    return Promise.resolve(this.sessionPresent ? "present" : "absent");
  }

  clearSessionKey(): Promise<void> {
    this.sessionPresent = false;
    return Promise.resolve();
  }

  listBrowserSessions(): Promise<DetectedBrowser[]> {
    return Promise.resolve([
      {
        id: "chrome",
        name: "Google Chrome",
        family: "chromium",
        permission_hint:
          "macOS will ask to unlock the login Keychain so the cookie can be decrypted.",
        settings_deep_link: null,
      },
      {
        id: "safari",
        name: "Safari",
        family: "safari",
        permission_hint: "Safari cookies need Full Disk Access to read.",
        settings_deep_link: "x-apple.systempreferences:com.apple.preference.security",
      },
      {
        id: "firefox",
        name: "Firefox",
        family: "firefox",
        permission_hint: null,
        settings_deep_link: null,
      },
      {
        id: "brave",
        name: "Brave",
        family: "chromium",
        permission_hint: null,
        settings_deep_link: null,
      },
      {
        id: "arc",
        name: "Arc",
        family: "chromium",
        permission_hint: null,
        settings_deep_link: null,
      },
    ]);
  }

  importBrowserSession(browser: Browser): Promise<ImportSummary> {
    this.sessionPresent = true;
    return Promise.resolve({ browser, validated: true });
  }

  getSettings(): Promise<AppSettings> {
    return Promise.resolve({ ...this.settings, shown_scoped_models: [...this.settings.shown_scoped_models] });
  }

  setShownScopedModels(models: string[]): Promise<void> {
    this.settings = { ...this.settings, shown_scoped_models: [...models] };
    return Promise.resolve();
  }

  setRefreshInterval(interval: RefreshInterval): Promise<void> {
    this.settings = { ...this.settings, refresh_interval: interval };
    return Promise.resolve();
  }

  setIconStyle(style: IconStyle): Promise<void> {
    this.settings = { ...this.settings, icon_style: style };
    return Promise.resolve();
  }

  iconStylePreviews(): Promise<IconPreview[]> {
    // No renderer outside the Tauri shell; the picker falls back to
    // label-only buttons when previews are empty.
    return Promise.resolve([]);
  }

  setMonochrome(monochrome: boolean): Promise<void> {
    this.settings = { ...this.settings, monochrome };
    return Promise.resolve();
  }

  setThresholds(warning: number, critical: number): Promise<AppSettings> {
    this.settings = {
      ...this.settings,
      warning_threshold: Math.min(Math.max(warning, 0), 100),
      critical_threshold: Math.min(Math.max(critical, 0), 100),
    };
    return Promise.resolve({ ...this.settings });
  }

  setNotifyOnReset(enabled: boolean): Promise<AppSettings> {
    this.settings = { ...this.settings, notify_on_reset: enabled };
    return Promise.resolve({ ...this.settings });
  }

  sendTestNotification(): Promise<boolean> {
    // No OS notification centre outside a Tauri shell; report success so the
    // Settings button's "sent" path is exercisable in a plain browser.
    return Promise.resolve(true);
  }

  setShowResetTime(enabled: boolean): Promise<AppSettings> {
    this.settings = { ...this.settings, show_reset_time: enabled };
    return Promise.resolve({ ...this.settings });
  }

  setPopoverLayout(layout: PopoverLayout): Promise<AppSettings> {
    this.settings = { ...this.settings, popover_layout: layout };
    return Promise.resolve({ ...this.settings });
  }

  setWeeklyPaceDays(days: number): Promise<AppSettings> {
    this.settings = { ...this.settings, weekly_pace_days: Math.min(Math.max(days, 5), 7) };
    return Promise.resolve({ ...this.settings });
  }

  setPaceFirstDisplay(enabled: boolean): Promise<AppSettings> {
    this.settings = { ...this.settings, pace_first_display: enabled };
    return Promise.resolve({ ...this.settings });
  }

  setPaceTrackingEnabled(enabled: boolean): Promise<AppSettings> {
    this.settings = { ...this.settings, pace_tracking_enabled: enabled };
    return Promise.resolve({ ...this.settings });
  }

  setUsageMode(mode: UsageMode): Promise<AppSettings> {
    this.settings = { ...this.settings, usage_mode: mode };
    return Promise.resolve({ ...this.settings });
  }

  setDebugLogging(enabled: boolean): Promise<AppSettings> {
    this.settings = { ...this.settings, debug_logging: enabled };
    return Promise.resolve({ ...this.settings });
  }

  debugLogPath(): Promise<string | null> {
    // A representative path so the Settings row is exercisable in a plain
    // browser; the real path is the OS log dir resolved on the Rust side.
    return Promise.resolve("~/Library/Logs/rusted-claude-meter/api-responses.log");
  }

  revealDebugLog(): Promise<void> {
    return Promise.resolve();
  }

  setPopoverHeight(): Promise<void> {
    // No popover outside a Tauri shell; the demo renders in a plain browser.
    return Promise.resolve();
  }

  wizardShouldRun(): Promise<boolean> {
    return Promise.resolve(!this.wizardCompleted);
  }

  wizardMarkOffered(): Promise<void> {
    // Same consume-once effect the real backend has: once offered, don't
    // auto-open again this session.
    this.wizardCompleted = true;
    return Promise.resolve();
  }

  wizardComplete(): Promise<void> {
    this.wizardCompleted = true;
    return Promise.resolve();
  }

  isGnomeDesktop(): Promise<boolean> {
    return Promise.resolve(false);
  }

  autostartStatus(): Promise<boolean> {
    return Promise.resolve(this.autostartEnabled);
  }

  setAutostart(enabled: boolean): Promise<boolean> {
    this.autostartEnabled = enabled;
    return Promise.resolve(this.autostartEnabled);
  }
}

export function createBackend(): UsageBackend {
  return isTauri() ? new TauriBackend() : new DemoBackend();
}

/** Best-effort extraction of a human message from an `invoke` rejection.
 * Tauri rejects command errors with the `Serialize`d `Err` value, so a
 * failed `set_session_key` or `import_browser_session` call rejects with a
 * `{ kind, message }`-shaped object (`SessionCommandError` /
 * `BrowserImportError`) rather than an `Error`. */
export function describeError(error: unknown): string {
  if (isSessionCommandError(error)) {
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function isSessionCommandError(value: unknown): value is SessionCommandError {
  return (
    typeof value === "object" &&
    value !== null &&
    "kind" in value &&
    "message" in value &&
    typeof (value as { message: unknown }).message === "string"
  );
}
