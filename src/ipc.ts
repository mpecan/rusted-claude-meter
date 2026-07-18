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
  IconPreview,
  IconStyle,
  ImportSummary,
  MeterState,
  RefreshInterval,
  SessionCommandError,
  SessionStatus,
  SessionSubmission,
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
class DemoBackend implements UsageBackend {
  private settings: AppSettings = { ...DEFAULT_SETTINGS };
  private sessionPresent = false;
  private wizardCompleted = false;
  private autostartEnabled = false;

  initialState(): Promise<MeterState> {
    return Promise.resolve(DEMO_STATE);
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
