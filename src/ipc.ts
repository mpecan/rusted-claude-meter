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
import type { MeterState, SessionCommandError } from "./types";

const USAGE_STATE_EVENT = "usage-state";

export interface UsageBackend {
  /** The state as of right now, for the initial render before the first
   * `usage-state` event arrives. */
  initialState(): Promise<MeterState>;
  /** Subscribe to every subsequent state broadcast. Returns an unsubscribe
   * function. */
  onStateChange(callback: (state: MeterState) => void): () => void;
  /** Parse and store a pasted session key. Rejects with a
   * `SessionCommandError`-shaped value on failure. */
  submitSessionKey(input: string): Promise<void>;
  /** Ask for a refresh now (TTL-guarded on the Rust side). */
  refreshUsage(): Promise<void>;
}

class TauriBackend implements UsageBackend {
  initialState(): Promise<MeterState> {
    return invoke<MeterState>("usage_state");
  }

  onStateChange(callback: (state: MeterState) => void): () => void {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    listen<MeterState>(USAGE_STATE_EVENT, (event) => callback(event.payload)).then((fn) => {
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

  submitSessionKey(input: string): Promise<void> {
    return invoke<void>("set_session_key", { input });
  }

  refreshUsage(): Promise<void> {
    return invoke<void>("refresh_usage");
  }
}

/** In-memory backend serving the demo fixture, for development outside
 * Tauri. `submitSessionKey` never fails so the CTA form path is also
 * reachable without a real backend. */
class DemoBackend implements UsageBackend {
  initialState(): Promise<MeterState> {
    return Promise.resolve(DEMO_STATE);
  }

  onStateChange(): () => void {
    return () => {};
  }

  submitSessionKey(): Promise<void> {
    return Promise.resolve();
  }

  refreshUsage(): Promise<void> {
    return Promise.resolve();
  }
}

export function createBackend(): UsageBackend {
  return isTauri() ? new TauriBackend() : new DemoBackend();
}

/** Best-effort extraction of a human message from an `invoke` rejection.
 * Tauri rejects command errors with the `Serialize`d `Err` value, so a
 * failed `set_session_key` call rejects with a `SessionCommandError`-shaped
 * object rather than an `Error`. */
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
