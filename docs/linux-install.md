# Linux install process (source for `scripts/install.sh`)

This documents an actual install of the `v0.1.3` release on a Linux box, as
the basis for `scripts/install.sh`. Verified on Arch Linux + KDE Plasma
(Wayland), x86_64.

## Available release assets

`gh release view -R mpecan/rusted-claude-meter` at `v0.1.3`:

- `Rusted.Claude.Meter_0.1.3_amd64.AppImage`
- `Rusted.Claude.Meter_0.1.3_amd64.deb`
- `rusted-claude-meter.rb` / `rusted-claude-meter-lite.rb` (Homebrew casks, macOS only)
- macOS `.dmg`s

`.deb` only helps on Debian/Ubuntu-family distros. `AppImage` is the only
asset that works everywhere on Linux, so `install.sh` uses it.

## Steps taken

1. Download the AppImage from the latest release:
   ```sh
   gh release download v0.1.3 -R mpecan/rusted-claude-meter -p "*.AppImage"
   ```
2. Make it executable and place it under `~/Applications`:
   ```sh
   chmod +x Rusted.Claude.Meter_0.1.3_amd64.AppImage
   mkdir -p ~/Applications
   cp Rusted.Claude.Meter_0.1.3_amd64.AppImage ~/Applications/RustedClaudeMeter.AppImage
   ```
3. Runtime dependency check (`pacman -Q <pkg>` on Arch; equivalents apply
   elsewhere) — `libayatana-appindicator`, `librsvg`, `fuse2` were already
   present, but **`webkit2gtk-4.1` was missing** and had to be installed
   separately:
   ```sh
   sudo pacman -S webkit2gtk-4.1
   ```
   Distro package managers vary; on Debian/Ubuntu the equivalent is
   `libwebkit2gtk-4.1-0` (already listed as a runtime need in the main
   README's dev-prereqs section, but worth calling out for end users too).
4. Launch:
   ```sh
   ~/Applications/RustedClaudeMeter.AppImage
   ```
   First run hit:
   ```
   Could not create surfaceless EGL display: EGL_BAD_ALLOC. Aborting...
   ```
   This is a known webkitgtk/EGL quirk on some GPU/driver combos, unrelated
   to this app specifically. Workaround — disable the DMA-BUF renderer path:
   ```sh
   WEBKIT_DISABLE_DMABUF_RENDERER=1 ~/Applications/RustedClaudeMeter.AppImage
   ```
   With that set, the process stayed up and the tray icon appeared (KDE
   Plasma shows AppIndicator tray icons natively — no extension needed,
   unlike GNOME).

   Opening a webview window (Settings, popover) still crashed
   `WebKitWebProcess` deterministically on this box — filed as
   [issue #50](https://github.com/mpecan/rusted-claude-meter/issues/50) with
   backtraces. The tray menu itself is unaffected; this only blocks
   webview-backed windows on this class of hardware.

## Implications for `install.sh`

- Download the AppImage from the `latest` release via the GitHub API rather
  than pinning a version.
- Install to `~/Applications`, `chmod +x`, and register a `.desktop` entry so
  the app shows up in launchers (AppImages don't do this themselves).
- Print, rather than silently handle, the two known rough edges: missing
  `webkit2gtk` on distros that don't ship it by default, and the
  `EGL_BAD_ALLOC` workaround — both are runtime/environment issues the script
  can't reliably detect or fix across every distro, so surfacing them to the
  user beats guessing.
