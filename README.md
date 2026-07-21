# Rusted Claude Meter

A cross-platform (macOS + Linux) tray app showing your Claude plan usage — a Tauri v2 port of [ClaudeMeter](https://github.com/eddmann/ClaudeMeter).

It polls `claude.ai/api/organizations/{org}/usage` with your browser session key and renders a colour-coded gauge in the menu bar / system tray, with per-window usage cards (5-hour session, 7-day week) and **model-scoped limits** read from the API's `limits` array — each entry names its own model (`scope.model.display_name`), so a newly released model needs no code change.

## Status

Feature-complete port, actively developed. The tray icon (six styles), the native macOS `NSPopover` (two switchable layouts), the Linux tray menu, a dedicated Settings window with a first-run wizard, threshold notifications, `usage.json` export, browser session import, launch-at-login, and packaging/release CI are all implemented. Bugs and follow-ups are tracked in [the issues](https://github.com/mpecan/rusted-claude-meter/issues).

## Architecture

| Crate | Responsibility |
|---|---|
| `crates/meter-core` | Platform-neutral domain: usage windows, scoped limits, status thresholds, pacing risk, session-key parsing. No I/O. |
| `crates/meter-api` | claude.ai API client: browser-header spoofing, response decoding, mapping into domain types. |
| `crates/meter-render` | Pure tray-icon rendering: `IconState` → parameterized SVG → RGBA pixels. No platform code. |
| `src-tauri` | Application shell: tray, windows, scheduler, notifications, secure storage, settings. |
| `src/` | Webview frontend (popover cards, settings, wizard) — vanilla TypeScript + Vite. |

Interaction model is platform-idiomatic:

- **macOS** — left-click the menu-bar icon to toggle a native `NSPopover` (via [`tauri-plugin-nspopover`](https://github.com/freethinkel/tauri-nspopover-plugin)) that hosts the webview: it drops down anchored under the status item with the arrow, slide animation and click-outside dismissal you expect. Settings open in their own dedicated window (front-most despite the accessory activation policy). Right-click serves the tray menu. The popover offers two layouts — compact **rows** or roomier **status cards** — switchable in Settings; both colour green → amber → red and raise an escalating fire glyph keyed to your configured warning/critical thresholds.
- **Linux** — StatusNotifierItem/AppIndicator delivers **no click events and no tooltip**, so the tray menu is the primary surface: a status line plus one live line per usage window (5-hour, 7-day, and each model-scoped limit) with percent and reset time, then Open / Refresh Now / Quit. Menu text updates in place — the tray icon is never recreated, so updates don't flicker. On GNOME the [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/) is required for the tray icon to appear at all; KDE Plasma shows it out of the box.

## External integrations

After every successful fetch, the app writes `~/.claudemeter/usage.json` — a public, typed export of current usage for statusline scripts and other external tools. The write is atomic (temp file + rename), so a script never observes a truncated file, and a failed write is logged but never fails the refresh itself.

The path is shared with the Swift [ClaudeMeter](https://github.com/eddmann/ClaudeMeter) app **intentionally**, so existing statusline integrations keep working unmodified when switching between the two. If both apps run at once, whichever fetches most recently wins — there is no locking or merging between them.

Schema (mirrors `ClaudeMeter`'s `UsageExportPayload`, [eddmann/ClaudeMeter#32](https://github.com/eddmann/ClaudeMeter/pull/32)):

```json
{
  "session_usage": { "utilization": 42.5, "reset_at": "2026-07-17T15:00:00Z" },
  "weekly_usage": { "utilization": 60.0, "reset_at": "2026-07-20T00:00:00Z" },
  "scoped_usage": [
    { "name": "Fable", "limit": { "utilization": 12.0, "reset_at": "2026-07-20T00:00:00Z" }, "is_active": true }
  ],
  "sonnet_usage": null,
  "last_updated": "2026-07-17T12:00:00Z"
}
```

`scoped_usage` is the general, forward-compatible form — one entry per model-scoped limit the API reports. `sonnet_usage` is a deprecated alias kept for backward compatibility: it mirrors the scoped entry named "Sonnet" (case-insensitive) when one exists, or `null` otherwise, so scripts written against the older Sonnet-only export keep working.

**Deviation from the Swift app:** in `ClaudeMeter`'s `UsageExportPayload`, `session_usage`/`weekly_usage` are non-optional — a snapshot missing a headline window either fails the fetch outright or gets a synthesized fallback reset time. This app's domain model already collapses "missing" into `None` with no data left to synthesize a fallback from, so on the rare snapshot without a headline window this export writes `session_usage`/`weekly_usage` as JSON `null` rather than omitting the field. Consumers written against the Swift app's non-optional guarantee should null-check these two fields.

## Development

Prerequisites: Rust (pinned via `rust-toolchain.toml`), Node ≥ 24 (pinned via `package.json` `engines`), [`just`](https://github.com/casey/just).

On Linux additionally: `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev libgtk-3-dev`. GNOME needs the [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/) to show tray icons at all.

```sh
just setup   # npm install, frontend build, git hooks
just dev     # run the app with hot reload
just check   # everything CI runs: fmt, clippy -D warnings, tests, file sizes,
             # cargo-deny, cargo-dupes, coverage floor, frontend typecheck + tests
```

`just check` needs a few cargo tools beyond `just setup` — see [CONTRIBUTING.md](CONTRIBUTING.md#setup).

## Packaging & releases

Releases are driven by release-please: merging its release PR cuts a GitHub
Release and `v*` tag, which builds a signed + notarized macOS DMG (signed on
macOS, then notarized + stapled on Linux via `rcodesign`), a Linux AppImage
and `.deb`, and a Homebrew cask, and uploads them to that Release — see
[`docs/packaging.md`](docs/packaging.md) for the release process, the Apple
signing/notarization secrets it needs, and the Flatpak evaluation findings.

### Antivirus / EDR false positives

Some endpoint-protection products — notably Palo Alto **Cortex XDR** (its
"Local Analysis" module) — may flag the macOS build as malware. **This is a
false positive.** The download is a Developer ID-signed, Apple-notarized build
(Apple Team ID `A98UV6DX7K`) of this open-source code, and the app only ever
talks to `claude.ai`.

Local Analysis is an on-device static-ML classifier that errs toward flagging
*unknown, low-prevalence* binaries before they ever run — a freshly-signed
release with little install history is a textbook trigger, and a later cloud
(WildFire) verdict overrides it. (Behavioural EDRs may separately flag the
opt-in "import session from a browser" step, which reads your browser's
`claude.ai` cookie — the same access pattern a credential stealer uses; see
[External integrations](#external-integrations).)

To resolve it:

- **Allow-list by code signer / Apple Team ID `A98UV6DX7K`** in your EDR —
  durable across every future signed release, unlike a per-build hash.
- **Report the false positive** to your vendor so their cloud verdict
  reclassifies the build as benign, which then overrides the local verdict for
  everyone.

Distributing through the Mac App Store would *not* fix this and isn't viable
anyway: the App Store mandates the App Sandbox, which forbids reading another
browser's cookie store — exactly the mechanism the session import depends on.

## Quality bar

- `cargo clippy --workspace --all-targets -- -D warnings` with `pedantic` + `nursery` enabled and `unwrap_used` / `expect_used` / `panic` / `todo` **denied** (tests may opt out locally).
- `unsafe_code` is forbidden workspace-wide.
- Source files stay under 500 lines (soft) / 700 (hard) — `scripts/check-file-sizes.sh`.
- Every behaviour lands with tests; API contracts are pinned by fixtures in `crates/meter-api/tests/fixtures/`.
- `cargo-deny` (`deny.toml`) gates dependency licenses, security advisories, banned crates, and dependency sources.
- `cargo-dupes` gates structural code duplication against a ratcheted ceiling.
- `cargo-llvm-cov` gates test coverage against a ratcheted floor — see the `coverage` job in `.github/workflows/ci.yml` for the PR-facing report.
- Dependabot keeps cargo, npm, and GitHub Actions dependencies current (`.github/dependabot.yml`).

## License

[MIT](LICENSE) © 2026 Matjaz Domen Pecan. All crates in the workspace inherit `license = "MIT"` from the root `Cargo.toml`.

Third-party notices:

- The tray-icon renderer bundles a subsetted **Roboto Mono** (digits and `%` only) to bake the percentage into the icons, under the SIL Open Font License 1.1 — see [`crates/meter-render/assets/RobotoMono-LICENSE.txt`](crates/meter-render/assets/RobotoMono-LICENSE.txt).
- The macOS `NSPopover` integration uses [`tauri-plugin-nspopover`](https://github.com/freethinkel/tauri-nspopover-plugin) (MIT), pinned by commit.
- Dependency licenses are gated by `cargo-deny` (`deny.toml`); the allowed set is enumerated there.
