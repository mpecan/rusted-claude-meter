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
