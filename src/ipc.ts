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

import { DEMO_STATE } from "./demo";
import { DEFAULT_SETTINGS } from "./settings-view-model";
import type {
  AppSettings,
  Browser,
  DetectedBrowser,
  IconStyle,
  ImportSummary,
  MeterState,
  RefreshInterval,
  SessionCommandError,
  SessionStatus,
  WizardSessionResult,
} from "./types";

const USAGE_STATE_EVENT = "usage-state";
/** Mirrors `tray::OPEN_SETTINGS_EVENT`: the tray's "Settings…" menu item is
 * the primary way to reach Settings on Linux, where the tray delivers no
 * click events for a popover-style affordance. */
export const OPEN_SETTINGS_EVENT = "open-settings";

export interface UsageBackend {
  /** The state as of right now, for the initial render before the first
   * `usage-state` event arrives. */
  initialState(): Promise<MeterState>;
  /** Subscribe to every subsequent state broadcast. Returns an unsubscribe
   * function. */
  onStateChange(callback: (state: MeterState) => void): () => void;
  /** Subscribe to the tray's "open Settings" request. Returns an
   * unsubscribe function. */
  onOpenSettings(callback: () => void): () => void;
  /** Parse and store a pasted session key. Rejects with a
   * `SessionCommandError`-shaped value on failure. */
  submitSessionKey(input: string): Promise<void>;
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
  /** Toggle monochrome/template tray artwork. */
  setMonochrome(monochrome: boolean): Promise<void>;
  /** Update the notification thresholds; resolves with the clamped values. */
  setThresholds(warning: number, critical: number): Promise<AppSettings>;
  /** Toggle the extra "limit reset" notification. */
  setNotifyOnReset(enabled: boolean): Promise<AppSettings>;
  /** Whether the setup wizard (issue #11) should open automatically on this
   * launch — `settings.json` did not exist before this launch loaded it. */
  wizardShouldRun(): Promise<boolean>;
  /** Parse, store and validate a pasted session key for the wizard's
   * "session" step, the same rollback-on-rejection way browser import
   * validates an imported one. Rejects with a `WizardSessionError`-shaped
   * value on failure. */
  wizardSubmitSessionKey(input: string): Promise<WizardSessionResult>;
  /** Mark the wizard complete by writing settings to disk even if nothing
   * changed, so "absence of settings" stops being true on the next launch. */
  wizardComplete(): Promise<void>;
  /** Whether this Linux session is GNOME, which hides the tray unless the
   * AppIndicator extension is installed. Always `false` off Linux. */
  isGnomeDesktop(): Promise<boolean>;
}

class TauriBackend implements UsageBackend {
  initialState(): Promise<MeterState> {
    return invoke<MeterState>("usage_state");
  }

  onStateChange(callback: (state: MeterState) => void): () => void {
    return subscribe(USAGE_STATE_EVENT, callback);
  }

  onOpenSettings(callback: () => void): () => void {
    return subscribe(OPEN_SETTINGS_EVENT, callback);
  }

  submitSessionKey(input: string): Promise<void> {
    return invoke<void>("set_session_key", { input });
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

  setMonochrome(monochrome: boolean): Promise<void> {
    return invoke<void>("set_monochrome", { monochrome });
  }

  setThresholds(warning: number, critical: number): Promise<AppSettings> {
    return invoke<AppSettings>("set_thresholds", { warning, critical });
  }

  setNotifyOnReset(enabled: boolean): Promise<AppSettings> {
    return invoke<AppSettings>("set_notify_on_reset", { enabled });
  }

  wizardShouldRun(): Promise<boolean> {
    return invoke<boolean>("wizard_should_run");
  }

  wizardSubmitSessionKey(input: string): Promise<WizardSessionResult> {
    return invoke<WizardSessionResult>("wizard_submit_session_key", { input });
  }

  wizardComplete(): Promise<void> {
    return invoke<void>("wizard_complete");
  }

  isGnomeDesktop(): Promise<boolean> {
    return invoke<boolean>("is_gnome_desktop");
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
class DemoBackend implements UsageBackend {
  private settings: AppSettings = { ...DEFAULT_SETTINGS };
  private sessionPresent = false;
  private wizardCompleted = false;

  initialState(): Promise<MeterState> {
    return Promise.resolve(DEMO_STATE);
  }

  onStateChange(): () => void {
    return () => {};
  }

  onOpenSettings(): () => void {
    return () => {};
  }

  submitSessionKey(): Promise<void> {
    this.sessionPresent = true;
    return Promise.resolve();
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
        id: "firefox",
        name: "Firefox",
        family: "firefox",
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

  wizardShouldRun(): Promise<boolean> {
    return Promise.resolve(!this.wizardCompleted);
  }

  wizardSubmitSessionKey(): Promise<WizardSessionResult> {
    this.sessionPresent = true;
    return Promise.resolve({ validated: true });
  }

  wizardComplete(): Promise<void> {
    this.wizardCompleted = true;
    return Promise.resolve();
  }

  isGnomeDesktop(): Promise<boolean> {
    return Promise.resolve(false);
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
