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

/** Mirrors `meter_core::Money`. A money amount in minor units, tagged with its
 * currency and decimal-place count — the exact shape claude.ai returns
 * (`{ minor: 35, currency: "EUR", exponent: 2 }` = €0.35). Kept in minor units
 * so no rounding creeps in; `€0.35` formatting is a frontend concern
 * (`formatMoney`). */
export interface Money {
  minor: number;
  currency: string;
  exponent: number;
}

/** Mirrors `meter_core::Spend`. Token/cost-based usage ("usage credits" /
 * paid overage) for the current period, present for accounts whose usage
 * response carries no allowance limits (Enterprise) and alongside allowance
 * limits when an account has opted into paid overage. `limit` is the spend
 * budget the gauge measures against and `cap` the hard ceiling (often equal);
 * either may be `null`. */
export interface Spend {
  used: Money | null;
  limit: Money | null;
  cap: Money | null;
  enabled: boolean;
}

/** Mirrors `meter_core::UsageSnapshot`. `fetched_at` is an RFC 3339 string.
 * `spend` carries token/cost usage when the account reports it, else `null`
 * (serde `Box<Spend>` is transparent, so it decodes as a plain object). */
export interface UsageSnapshot {
  five_hour: UsageWindow | null;
  seven_day: UsageWindow | null;
  scoped: ScopedLimit[];
  spend: Spend | null;
  fetched_at: string;
}

/** Mirrors `meter_core::UsageMode`. `auto` follows the account (allowance when
 * it reports limits, cost otherwise); `allowance`/`cost` pin the view. */
export type UsageMode = "auto" | "allowance" | "cost";

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
 * "message" }` serde representation. `Rejected` means the key parsed but
 * claude.ai refused it — the previously stored key (if any) was restored. */
export interface SessionCommandError {
  kind: "Validation" | "Rejected" | "Store";
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

/** A `<select>` option's value/label pair. */
export interface SelectOption<T extends string> {
  value: T;
  label: string;
}

/** One rendered icon-style preview from `icon_style_previews`: straight-alpha
 * RGBA (`width * height * 4` bytes) the picker paints into a `<canvas>`, so
 * the style buttons show the actual tray artwork. */
export interface IconPreview {
  style: IconStyle;
  width: number;
  height: number;
  rgba: number[];
}

/** The tray icon style choices, in display order. Single source of truth for
 * both the Settings panel's `#icon-style-select` and the wizard's customize
 * step's `#wizard-icon-style-select` — see `settings-render.ts::renderSelectOptions`,
 * which both `main.ts` and `wizard.ts` use to populate their `<select>`. */
export const ICON_STYLE_OPTIONS: readonly SelectOption<IconStyle>[] = [
  { value: "battery", label: "Battery" },
  { value: "circular", label: "Circular" },
  { value: "minimal", label: "Minimal" },
  { value: "segments", label: "Segments" },
  { value: "dual_bar", label: "Dual Bar" },
  { value: "gauge", label: "Gauge" },
];

/** The refresh interval choices, in display order. Single source of truth
 * for both `#refresh-interval-select` and `#wizard-refresh-interval-select`. */
export const REFRESH_INTERVAL_OPTIONS: readonly SelectOption<RefreshInterval>[] = [
  { value: "one_minute", label: "Every minute" },
  { value: "five_minutes", label: "Every 5 minutes" },
  { value: "ten_minutes", label: "Every 10 minutes" },
];

/** Mirrors `commands::SessionSubmission`: the outcome of a validated
 * session-key submission, shared by the popover field, the Settings field
 * and the wizard's paste step (issues #1/#11). */
export interface SessionSubmission {
  validated: boolean;
}

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
  /** Whether each card appends the exact reset wall-clock time next to the
   * relative countdown (ClaudeMeter PR #26). On by default. */
  show_reset_time: boolean;
  /** Which popover layout the frontend renders (redesign 1a/1c). */
  popover_layout: PopoverLayout;
  /** How many days of the week the weekly quota is expected to be paced over
   * (issue #16's working-week option) — 5, 6 or 7. Clamped to `5..=7` on the
   * Rust side; applied to the weekly and scoped weekly cards. Defaults to 7
   * (the full week). */
  weekly_pace_days: number;
  /** Whether the tray/popover lead with the pace ratio instead of the raw
   * quota percentage (upstream's `DisplayModePicker`). Off by default. */
  pace_first_display: boolean;
  /** Master switch for the whole pace-tracking feature (issue #16). When off,
   * the popover drops projections / pace line / verdict badge and the tray
   * shows no pace ratio or badge. On by default. */
  pace_tracking_enabled: boolean;
  /** Which usage view to render: `auto` follows the account (allowance vs
   * cost), `allowance`/`cost` pin it. Defaults to `auto`. */
  usage_mode: UsageMode;
  /** Whether raw claude.ai API responses are appended to a local log file for
   * troubleshooting (how the token/cost `spend` shape was captured, and the way
   * to verify it against account types not yet observed). Off by default; the
   * log holds only usage data, never the session key. */
  debug_logging: boolean;
}

/** Mirrors `meter_shell::settings::PopoverLayout` — the two popover layouts
 * (redesign 1a compact rows / 1c status cards). */
export type PopoverLayout = "rows" | "cards";
