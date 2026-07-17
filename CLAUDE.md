Rusted Claude Meter is a Tauri v2 tray app (macOS + Linux) showing Claude plan usage — a port of the SwiftUI ClaudeMeter.

Layout: `crates/meter-core` = pure domain (no I/O); `crates/meter-api` = claude.ai HTTP client + response mapping; `src-tauri` = app shell (tray, scheduler, storage, notifications); `src/` = vanilla TS frontend.

Run `just check` before considering any change done — it is exactly what CI enforces: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` (pedantic + nursery on; `unwrap_used`/`expect_used`/`panic`/`todo` are deny), `cargo test --workspace`, and the 500/700-line file-size gate. Test modules may `#![allow(clippy::unwrap_used)]`.

The frontend must be built once (`npm run build`) before any cargo command touching `src-tauri` — `tauri-build` requires `dist/` to exist.

Model-scoped limits contract: the API's `limits` array is the source of truth for per-model caps; entries are keyed by `scope.model.display_name` (API-supplied, e.g. "Sonnet", "Fable"), headline kinds (`five_hour`, `seven_day`) are excluded from the scoped pass, and incomplete entries are skipped, never errors. Flat fields are the headline fallback. Fixture: `crates/meter-api/tests/fixtures/usage_response.json`.

Never log or serialize the session key in the clear: `SessionKey` redacts in `Debug`/`Display` and the Cookie header is marked sensitive.

Linux tray reality: StatusNotifierItem gives no click events or tooltips — the tray menu is the primary Linux surface; the popover-style window is macOS-only behaviour.
