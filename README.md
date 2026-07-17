# Rusted Claude Meter

A cross-platform (macOS + Linux) tray app showing your Claude plan usage — a Tauri v2 port of [ClaudeMeter](https://github.com/eddmann/ClaudeMeter).

It polls `claude.ai/api/organizations/{org}/usage` with your browser session key and renders a colour-coded gauge in the menu bar / system tray, with per-window usage cards (5-hour session, 7-day week) and **model-scoped limits** read from the API's `limits` array — each entry names its own model (`scope.model.display_name`), so a newly released model needs no code change.

## Status

Scaffold. The workspace layout, domain model, API decoding contract and quality gates are in place; the implementation is tracked in [the issues](https://github.com/mpecan/rusted-claude-meter/issues).

## Architecture

| Crate | Responsibility |
|---|---|
| `crates/meter-core` | Platform-neutral domain: usage windows, scoped limits, status thresholds, pacing risk, session-key parsing. No I/O. |
| `crates/meter-api` | claude.ai API client: browser-header spoofing, response decoding, mapping into domain types. |
| `src-tauri` | Application shell: tray, windows, scheduler, notifications, secure storage, settings. |
| `src/` | Webview frontend (popover cards, settings, wizard) — vanilla TypeScript + Vite. |

Interaction model is platform-idiomatic: a popover-style window anchored to the menu bar item on macOS; a tray menu with live usage lines on Linux (StatusNotifierItem delivers no click events, only menus).

## Development

Prerequisites: Rust (pinned via `rust-toolchain.toml`), Node 24+, [`just`](https://github.com/casey/just).

On Linux additionally: `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev libgtk-3-dev`. GNOME needs the [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/) to show tray icons at all.

```sh
just setup   # npm install, frontend build, git hooks
just dev     # run the app with hot reload
just check   # everything CI runs: fmt, clippy -D warnings, tests, file sizes
```

## Quality bar

- `cargo clippy --workspace --all-targets -- -D warnings` with `pedantic` + `nursery` enabled and `unwrap_used` / `expect_used` / `panic` / `todo` **denied** (tests may opt out locally).
- `unsafe_code` is forbidden workspace-wide.
- Source files stay under 500 lines (soft) / 700 (hard) — `scripts/check-file-sizes.sh`.
- Every behaviour lands with tests; API contracts are pinned by fixtures in `crates/meter-api/tests/fixtures/`.

## License

MIT
