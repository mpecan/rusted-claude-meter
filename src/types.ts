// Frontend mirror of the JSON shapes Rust emits over the `usage-state` event
// and the `usage_state` command (see src-tauri/src/scheduler/core.rs and
// crates/meter-core/src/snapshot.rs). Field names and enum spellings must
// match serde's output exactly: plain field names (no rename) and
// `rename_all = "snake_case"` on every enum.

/** Mirrors `meter_core::LimitWindow`. */
export type LimitWindow = "five_hour" | "seven_day";

/** Mirrors `meter_core::UsageWindow`. `resets_at` is an RFC 3339 string. */
export interface UsageWindow {
  utilization: number;
  resets_at: string;
  window: LimitWindow;
}

/** Mirrors `meter_core::ScopedLimit`. */
export interface ScopedLimit {
  display_name: string;
  model_id: string | null;
  usage: UsageWindow;
  is_active: boolean;
}

/** Mirrors `meter_core::UsageSnapshot`. `fetched_at` is an RFC 3339 string. */
export interface UsageSnapshot {
  five_hour: UsageWindow | null;
  seven_day: UsageWindow | null;
  scoped: ScopedLimit[];
  fetched_at: string;
}

/** Mirrors `scheduler::core::Staleness`. */
export type Staleness = "missing" | "fresh" | "stale";

/** Mirrors `scheduler::core::Phase`. */
export type Phase = "polling" | "degraded" | "awaiting_session" | "session_expired";

/** Mirrors `scheduler::core::MeterState`: the single source of truth pushed
 * over the `usage-state` event and returned by the `usage_state` command. */
export interface MeterState {
  snapshot: UsageSnapshot | null;
  staleness: Staleness;
  phase: Phase;
}

/** Mirrors `commands::SessionCommandError`'s `{ tag = "kind", content =
 * "message" }` serde representation. */
export interface SessionCommandError {
  kind: "Validation" | "Store";
  message: string;
}

/** Mirrors `commands::SessionStatus`. */
export type SessionStatus = "present" | "absent";

/** Mirrors `meter_core::Browser`'s snake_case serde ids (issue #10). */
export type Browser =
  | "chrome"
  | "chromium"
  | "brave"
  | "edge"
  | "vivaldi"
  | "opera"
  | "opera_gx"
  | "arc"
  | "firefox"
  | "librewolf"
  | "zen"
  | "safari";

/** Mirrors `meter_core::BrowserFamily`. */
export type BrowserFamily = "chromium" | "firefox" | "safari";

/** Mirrors `browser_import::DetectedBrowser`: an import source with the
 * permission story it implies on this platform. */
export interface DetectedBrowser {
  id: Browser;
  name: string;
  family: BrowserFamily;
  /** Copy warning about the permission prompt to expect, or null. */
  permission_hint: string | null;
  /** A settings deep link (Full Disk Access on macOS for Safari), or null. */
  settings_deep_link: string | null;
}

/** Mirrors `browser_import::ImportSummary`. */
export interface ImportSummary {
  browser: string;
  /** Whether claude.ai confirmed the key. `false` means it's stored but will
   * be verified on the next poll (claude.ai was unreachable). */
  validated: boolean;
}

/** Mirrors `browser_import::BrowserImportError`'s `{ tag = "kind", content =
 * "message" }` serde representation. Shares `describeError`'s handling with
 * `SessionCommandError` since both are `{ kind, message }`. */
export interface BrowserImportError {
  kind: "Unsupported" | "CookieStore" | "NoSession" | "Invalid" | "Rejected" | "Store";
  message: string;
}

/** Mirrors `scheduler::core::RefreshInterval`. */
export type RefreshInterval = "one_minute" | "five_minutes" | "ten_minutes";

/** Mirrors `meter_render::IconStyle` (issue #9's six tray styles). */
export type IconStyle = "battery" | "circular" | "minimal" | "segments" | "dual_bar" | "gauge";

/** Mirrors `settings::AppSettings`. `shown_scoped_models` is opt-in and
 * empty by default: a scoped model reported in a snapshot is not shown in
 * the popover or the Linux tray menu until its `display_name` is added
 * here (see `src-tauri/src/settings.rs` and `tray/model.rs::menu_model`). */
export interface AppSettings {
  shown_scoped_models: string[];
  refresh_interval: RefreshInterval;
  /** Utilization percentage (0-100) at which a notification is a warning. */
  warning_threshold: number;
  /** Utilization percentage (0-100) at which a notification is critical. */
  critical_threshold: number;
  /** Whether a window resetting fires its own "limit reset" notification
   * (issue #7). Threshold-crossing notifications are always on. */
  notify_on_reset: boolean;
  icon_style: IconStyle;
  monochrome: boolean;
}
