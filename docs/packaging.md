# Packaging & release

Covers issue [#14](https://github.com/mpecan/rusted-claude-meter/issues/14):
signed/notarized macOS DMG, Linux AppImage + `.deb`, and a release-please
flow that needs no manual version bumps or artifact handling.

## Release process

Two distinct workflows, split on purpose so building never runs on a plain
tag push:

1. **`.github/workflows/release-please.yml`** (on push to `main`) maintains a
   **release PR** via
   [release-please](https://github.com/googleapis/release-please). It bumps
   the version everywhere it lives — `Cargo.toml`'s `[workspace.package]`
   (annotated with `# x-release-please-version`), `src-tauri/tauri.conf.json`
   and `package.json` — and rolls up `CHANGELOG.md` from Conventional Commits.
   Config lives in `.release-please-config.json` +
   `.release-please-manifest.json`. Merging that PR creates the GitHub Release
   and the `v*` tag.
   - It authenticates as a GitHub App (`REPOSITORY_BUTLER_APP_ID` /
     `REPOSITORY_BUTLER_PEM`), not the default `GITHUB_TOKEN`, **so the
     `release: published` event actually fires** — a release created by
     `GITHUB_TOKEN` would not trigger the build stage below.
2. **`.github/workflows/release.yml`** runs **only** on that published release
   (plus a manual `workflow_dispatch` by tag, for re-builds). It checks out
   the tag and, on `macos-latest` and `ubuntu-22.04` in parallel, builds the
   bundle for that OS (`bundle.targets` in `tauri.conf.json` restricts this to
   `app`+`dmg` on macOS, `deb`+`appimage` on Linux) via
   [`tauri-action`](https://github.com/tauri-apps/tauri-action) — signing +
   notarizing the macOS DMG when the Apple secrets are set. Artifacts upload
   onto the existing release (found by id), and the macOS leg renders +
   uploads the Homebrew cask (`scripts/render-cask.sh`).

Nothing beyond merging the release PR is manual — the version bump is
automatic. `ubuntu-22.04` (not `-latest`) is deliberate: an AppImage/deb built
against an older glibc stays installable on older distros; `-latest` would
narrow that.

## Build variants: full vs lite

Two build variants, selected by the `browser-import` Cargo feature (default on,
in `src-tauri/Cargo.toml`):

| Variant | Feature | Bundle id | What it does |
|---|---|---|---|
| **Full** (default) | `browser-import` on | `com.mpecan.rusted-claude-meter` | Includes automated session import — reads the browser's claude.ai cookie store (Chrome/Safari/Firefox) via the `rookie` crate. |
| **Lite** | `--no-default-features` | `com.mpecan.rusted-claude-meter-lite` | Compiles out browser import **and `rookie`** entirely — the binary never reads another app's cookie/credential store. Manual session-key paste only. |

The lite build exists for endpoint-protection–restricted environments: reading
browser cookie stores is behaviourally identical to an infostealer, so EDRs
(e.g. Cortex XDR) flag the full build (see the README "Antivirus / EDR false
positives"). The lite build removes that behaviour at compile time, so there's
nothing for the heuristics to catch — and it's a genuinely smaller binary.

Build it locally:

```
npm run tauri build -- --no-default-features --config src-tauri/tauri.lite.conf.json
```

`tauri.lite.conf.json` overrides only `productName` (→ "Rusted Claude Meter
Lite", so its `.app`/`.dmg` filenames don't collide) and `identifier` (→ the
`-lite` bundle id, so the two variants keep separate Keychain items and can
coexist on one machine). Everything else — icon, version, entitlements — is
inherited. The `store.rs` Keychain service string is `-lite`-aware via the same
feature flag, so each variant stores its own session key.

CI lints the lite config on every run (`Clippy (lite build …)` in the Lint job)
so it can't rot.

> **Release publishing of the lite variant is not yet wired into `release.yml`**
> — the release currently builds/ships the full variant only. Publishing both
> as release assets (sign both on the macOS runner, notarize+staple both on the
> Linux runner, render a second Homebrew cask) is a follow-up.

## macOS signing & notarization

Signing and notarization are **split across two runners** so the 10x-cost
macOS runner never idles on Apple's (unbounded, often slow) notary wait:

- **`build-macos`** code-signs the `.app`/`.dmg` with the Developer ID cert
  (hardened runtime, per `tauri.conf.json`) and stops — no notary round-trip.
- **`notarize`** (1x-cost Linux runner) notarizes + staples that DMG with
  [`rcodesign`](https://github.com/indygreg/apple-platform-rs), which drives
  Apple's notary API and staples without a Mac, then uploads the notarized DMG
  and renders the Homebrew cask from it.

Repository secrets (Settings → Secrets and variables → Actions):

| Secret | Used by | What it is |
|---|---|---|
| `APPLE_CERTIFICATE` | sign | base64 of a `Developer ID Application` `.p12` export (cert + key) |
| `APPLE_CERTIFICATE_PASSWORD` | sign | password used to export that `.p12` |
| `APPLE_SIGNING_IDENTITY` | sign | e.g. `Developer ID Application: Name (TEAMID)` |
| `APPLE_API_ISSUER_ID` | notarize | App Store Connect API key **Issuer ID** (a UUID) |
| `APPLE_API_KEY_ID` | notarize | App Store Connect API **Key ID** (e.g. `DEADBEEF42`) |
| `APPLE_API_KEY_P8` | notarize | base64 of the downloaded `AuthKey_*.p8` |

The API key is created in App Store Connect → Users and Access → Integrations
→ App Store Connect API (role: Developer). `rcodesign` reconstructs its key
file with `encode-app-store-connect-api-key <issuer-id> <key-id> AuthKey.p8`.

Without the signing secrets the DMG still ships *unsigned* (Gatekeeper warns);
without the notary secrets it ships signed-but-un-notarized. The build is
never blocked on secrets — only real signing/notarization is.

**Stapling scope:** the `.dmg` is stapled (so download + mount verifies
offline). The `.app` *inside* is signed + notarized but not itself stapled —
rebuilding the read-only DMG to staple the inner app would need macOS again —
so an offline first-launch straight from the mounted image falls back to
Gatekeeper's online notarization check. This is the deliberate trade for
keeping the notary wait off the paid runner; for a normally-online install
(incl. the Homebrew cask) it's transparent.

The legacy `notarytool` path (Apple ID + app-specific password:
`APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID`) is no longer used and those
secrets can be removed.

## Keychain access & avoiding re-prompts

The session key is stored in the OS credential store (macOS Keychain via
`apple-native-keyring-store`'s `keychain::Store`; see `src-tauri/src/store.rs`).
The recurring *"…wants to use your confidential information stored in your
keychain — enter your password"* prompt users sometimes hit is a **code-signing
issue, not a storage bug**, and this is how we avoid it.

**How silent access works.** When the app writes the key, macOS records the
creating binary's *designated requirement* (its code signature) in the item's
access-control list and grants **silent** access only to a binary that matches.
So:

- A **stably Developer ID-signed + notarized** build reads its own item with no
  prompt, launch after launch. The designated requirement is anchored to your
  **Team ID + bundle ID**, so it survives certificate *renewals* and app
  *updates* — trust does not reset as long as those two are unchanged.
- An **unsigned or ad-hoc-signed** build has a different (or absent) signature,
  so it never matches the stored ACL → prompt on every access.

**Therefore the fix is simply to ship signed** (the section above). No keychain
entitlement is required for an app to read its *own* login-keychain item — the
committed `src-tauri/entitlements.plist` is intentionally empty (hardened
runtime with no exceptions). Our `save` updates the item in place and we only
re-save when the key actually changes, so the ACL is never needlessly churned.

**One gotcha to expect on first signed release.** The ACL is owned by whoever
*created* the item. A user who previously ran an **unsigned** local/dev build
has an item whose ACL trusts that old ad-hoc signature, so the first signed
release will prompt **once**. Clicking *Always Allow* fixes it; alternatively,
clearing and re-saving the key from the signed app re-creates the ACL under the
trusted identity (the setup wizard's "paste a key" / re-import flow already does
a fresh `save`).

**Dev builds.** `just dev` / unsigned local builds are ad-hoc-signed and get a
new signature each build, so they re-prompt — this is expected, not a
regression. To avoid it while developing you can sign locally with a stable
self-signed cert and click *Always Allow* once.

**Optional upgrade — the Data Protection keychain.** For an iOS-style model
with *no ACL prompt even on first access*, the key can move to
`apple-native-keyring-store`'s `protected::Store`, where access is governed by a
**keychain access group** (via the `keychain-access-groups` entitlement) plus an
accessibility policy such as `AfterFirstUnlock` (readable by a login-launched
instance after the first unlock per boot). This needs: (1) the entitlement with
your literal Team ID — a template is in `src-tauri/entitlements.plist`; (2)
swapping the store backend in `src-tauri/src/store.rs`; and (3) accepting that
**unsigned local dev builds can't use it** (they lack the entitlement). We stay
on the legacy keychain for now because stable signing already solves the
prompts without breaking `just dev`.

**Linux.** The key lives in the login keyring (Secret Service default
collection), which the desktop's PAM stack unlocks automatically at login *when
the keyring password equals the login password* (the default on most distros).
A user only gets a per-session unlock prompt if they set a **separate** keyring
password or use passwordless auto-login without a PAM keyring-unlock module —
that is desktop configuration, not something the app can change. We already
store in the default collection; there is nothing further to do app-side.

**Not the app's storage.** The browser-import step (issue #10) prompts for each
Chromium browser's own "Safe Storage" Keychain item — that item belongs to
Chrome and its ACL will never trust us, so that prompt is unavoidable, but it is
a **one-time import action**, unrelated to the app's own recurring reads.

## Linux: AppImage + `.deb`

Built via the same `tauri-action` step, using the system deps already listed
in `.github/workflows/ci.yml`'s Linux jobs
(`libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev
libssl-dev libgtk-3-dev`). `tauri.conf.json`'s `bundle.linux.deb.depends`
pins the matching runtime packages so `apt`/`dpkg` pull them in on install.

**Deferred / not verified in this change:** the issue asks for tray +
notification behaviour to be exercised on GNOME (with the AppIndicator
extension), KDE Plasma, and one minimal WM (e.g. i3/sway). That needs real
desktop sessions this environment doesn't have. What *is* already covered:

- The tray-menu-as-primary-surface behaviour on Linux (no click events, no
  tooltip on StatusNotifierItem) is implemented and unit-tested — see
  `src-tauri/src/tray/`.
- The README already documents that GNOME needs the AppIndicator extension
  for the tray icon to appear at all; KDE works out of the box.

A fresh-machine install + tray/notification smoke test on at least one real
GNOME, one KDE, and one minimal-WM (or headless Xvfb + i3) machine remains
manual follow-up before this is called verified rather than "should work
based on the StatusNotifierItem contract."

## Flatpak — evaluated, not shipped

The issue asks to evaluate Flatpak separately, since tray icons under it have
known rough edges. Findings, from Flatpak's/`xdg-desktop-portal`'s documented
sandboxing model (not hands-on-tested here — no Flatpak runtime available in
this environment):

- A sandboxed app has no direct D-Bus access to `org.kde.StatusNotifierWatcher`
  (or its freedesktop equivalent) unless the manifest grants
  `--talk-name=org.kde.StatusNotifierWatcher` (and, for the freedesktop
  tray spec, `--talk-name=org.freedesktop.StatusNotifierItem` on the app's
  own bus name pattern). This is a known extra manifest requirement beyond a
  typical GUI app.
- Even with that permission granted, GNOME still requires the AppIndicator
  extension host-side — Flatpak doesn't change that requirement, it just
  adds a sandboxing hoop on top of it.
- `xdg-desktop-portal`'s own tray/notification portals are inconsistently
  implemented across desktop environments and portal backends; several
  Flatpak'd tray apps in the wild ship a runtime detection fallback (nag the
  user to install the AppIndicator extension, or fall back to a regular
  window) rather than relying on the portal.
- Autostart under Flatpak goes through the
  `org.freedesktop.portal.Background` portal rather than a plain
  `~/.config/autostart/*.desktop` file — this app's current
  `tauri-plugin-autostart` integration (issue #12) assumes the latter and
  would need portal-specific handling to work sandboxed.

**Conclusion:** Flatpak is not part of this release's packaging targets. The
extra manifest permissions it needs for the tray to work at all — on top of
an autostart mechanism that doesn't apply unmodified — make it a separate,
larger piece of work rather than "add one more bundle target." Revisit as a
follow-up issue if Flathub distribution becomes a goal; until then AppImage +
`.deb` cover the Linux install path this issue asks for.

## Homebrew cask

`Casks/rusted-claude-meter.rb.tmpl` is a template; `scripts/render-cask.sh`
fills in the version, the built DMG's sha256, and its GitHub Release download
URL, producing `rusted-claude-meter.rb`, which the release workflow attaches
to the release as an asset (macOS leg only).

The repo is currently private, so a **private tap** is the right home for it
per the issue's scope ("private tap OK while the repo is private"). To
consume a release's cask from a private tap you control:

```sh
# one-time: create (or reuse) a private tap repo, e.g. mpecan/homebrew-tap
brew tap-new mpecan/tap   # or clone your existing private tap
curl -fsSL -o "$(brew --repo mpecan/tap)/Casks/rusted-claude-meter.rb" \
  "https://github.com/mpecan/rusted-claude-meter/releases/latest/download/rusted-claude-meter.rb"
cd "$(brew --repo mpecan/tap)" && git add Casks/rusted-claude-meter.rb && git commit -m "rusted-claude-meter <version>" && git push

# thereafter
brew install --cask mpecan/tap/rusted-claude-meter
```

This repo does not push to a tap automatically — that would require a
separate tap repo and a token with write access to it, neither of which
exist yet. Publishing the rendered cask to a tap remains a manual (or
follow-up-automated) step per release; only rendering it is automated here.

## Fresh-machine install acceptance criteria

The issue's acceptance criteria — "fresh-machine installs (macOS + one Linux
distro) reach a working meter with no manual steps beyond OS keyring
unlock" — needs an actual clean VM/machine per platform, which this
environment doesn't have. What's already true by construction, verified by
the existing test suite rather than a live fresh-machine run:

- Session-key storage goes through the OS keyring (issue #1) and the setup
  wizard (issue #11) prompts for a session key on first run when none is
  stored — so "unlock the keyring, paste/import a session key" is the only
  expected manual step, and nothing else in the app's startup path
  (`src-tauri/src/lib.rs::run`) requires configuration before the tray shows
  a state.
- `just check`'s coverage/test gates exercise the scheduler, cache, tray, and
  wizard logic without a real keyring or network — that's necessarily a
  proxy for, not a replacement for, an actual fresh install.

A real fresh-VM install pass (macOS + one Linux distro) is the concrete
remaining verification step, deferred here for the same reason as the
Linux desktop-environment matrix above: no such machine is available in this
environment.
