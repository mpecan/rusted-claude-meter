# Packaging & release

Covers issue [#14](https://github.com/mpecan/rusted-claude-meter/issues/14):
signed/notarized macOS DMG, Linux AppImage + `.deb`, and a tag-triggered
release workflow that needs no manual artifact handling.

## Release process

1. Bump `version` in `Cargo.toml`'s `[workspace.package]` and in
   `src-tauri/tauri.conf.json` (Tauri reads its own `version`, it does not
   inherit the workspace one).
2. Tag: `git tag v0.2.0 && git push origin v0.2.0`.
3. `.github/workflows/release.yml` takes it from there:
   - `changelog` renders release notes from Conventional Commits since the
     previous tag with [git-cliff](https://git-cliff.org) (`cliff.toml`) and
     opens a **draft** GitHub Release with them.
   - `build` runs on `macos-latest` and `ubuntu-22.04` in parallel, builds
     the bundle for that OS (`bundle.targets` in `tauri.conf.json` restricts
     this to `app`+`dmg` on macOS, `deb`+`appimage` on Linux) via
     [`tauri-action`](https://github.com/tauri-apps/tauri-action), and
     uploads the artifacts to the same draft release (found by tag). The
     macOS leg also renders and uploads the Homebrew cask
     (`scripts/render-cask.sh`).
   - `publish` un-drafts the release once both legs are done, so it's never
     visible with only half its artifacts attached.

Nothing beyond pushing the tag is manual. `ubuntu-22.04` (not `-latest`) is
deliberate: an AppImage/deb built against an older glibc stays installable on
older distros; `-latest` would narrow that.

## macOS signing & notarization

`tauri-action` signs and notarizes automatically when these repository
secrets are set (Settings → Secrets and variables → Actions):

| Secret | What it is |
|---|---|
| `APPLE_CERTIFICATE` | base64 of a `Developer ID Application` `.p12` export |
| `APPLE_CERTIFICATE_PASSWORD` | password used to export that `.p12` |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Name (TEAMID)` |
| `APPLE_ID` | Apple ID used for notarization |
| `APPLE_PASSWORD` | an **app-specific** password for that Apple ID |
| `APPLE_TEAM_ID` | 10-character Apple Developer Team ID |

Without them the workflow still produces a working, *unsigned* DMG —
Gatekeeper shows a warning on first launch but the app runs. The build is
never blocked on these secrets; only real signing/notarization is.

**Deferred / not verified in this change:** no Apple Developer account or
signing certificate was available in this environment, so the actual
signed+notarized DMG output, and Gatekeeper's acceptance of it, have not been
exercised end to end. The workflow config follows the documented
`tauri-action` contract for these env vars; the first real tagged release is
the first point this can be verified for real.

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
