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
   Initially looked like a webkitgtk/EGL quirk tied to this box's hybrid
   Intel/NVIDIA GPU under Wayland. Setting
   `WEBKIT_DISABLE_DMABUF_RENDERER=1` got the process past that point, but
   then every webview window (Settings, popover) crashed
   `WebKitWebProcess` deterministically — the tray menu itself stayed up
   and usable throughout, only windows backed by a webview were affected.

   **Root cause, found via a source build (see below): not GPU/Wayland at
   all.** The AppImage bundles its own `libwebkit2gtk`/`libjavascriptcoregtk`,
   built against a `libjxl` newer (0.12) than what this system had installed
   (0.11.2). That version skew is what crashed `WebKitWebProcess`; the
   `EGL_BAD_ALLOC` abort was a symptom of the same mismatch, not a separate
   GPU issue. Full writeup and the corrected diagnosis:
   [issue #50](https://github.com/mpecan/rusted-claude-meter/issues/50).

## Building from source (Arch — no packaged build exists)

There's no Arch/AUR package, so a source build is the reliable path here
until one exists. This is also what confirmed the AppImage's crash was a
bundled-library version mismatch, not a hardware incompatibility — a
source build links the *system's* webkit2gtk/libjxl instead of bundling
its own.

1. Install build + runtime dependencies:
   ```sh
   sudo pacman -S --needed just webkit2gtk-4.1 libayatana-appindicator \
     librsvg xdotool openssl gtk3 base-devel
   ```
   `xdotool` provides `libxdo.pc`, the Arch equivalent of Debian's
   `libxdo-dev`. If `webkit2gtk-4.1`/`libjxl` are already installed but
   stale, `sudo pacman -Syu` first — an out-of-date `libjxl` is exactly
   what caused the AppImage crash above.
2. Build:
   ```sh
   npm install
   npm run build       # tauri-build needs dist/ to exist before any cargo step
   npm run tauri build # NOT `cargo build --release` directly — see gotcha below
   ```
3. Run the built binary directly, or install the generated `.deb`:
   ```sh
   ./target/release/rusted-claude-meter
   # or: sudo pacman -U — n/a on Arch; use dpkg/apt on Debian-family, or:
   sudo dpkg -i "target/release/bundle/deb/Rusted Claude Meter_0.1.3_amd64.deb"  # Debian/Ubuntu only
   ```
   On Arch there's no local package manager step for the `.deb` — running
   the binary from `target/release/` directly is the practical option
   until an Arch-native artifact exists.

   Result: launches clean, no `EGL_BAD_ALLOC`, no `WebKitWebProcess` crash,
   Settings/popover windows render normally — confirming the AppImage issue
   was the bundled-`libjxl` mismatch, not this machine's GPU/Wayland setup.

### Build gotcha: `cargo build --release` alone is not enough

Building with plain `cargo build --release -p rusted-claude-meter` (skipping
the Tauri CLI) produces a binary that tries to load `http://localhost:1420`
(the dev server URL) instead of the bundled `dist/` assets — the Settings
window shows "Could not connect to localhost: Connection refused". Only
`npm run tauri build` (which drives `tauri-cli`) correctly embeds
`frontendDist` into the binary. Always build through `just build` /
`npm run tauri build`, never raw `cargo build`, when you want a runnable
release binary.

The `npm run tauri build` AppImage-bundling step itself failed on this
box (`failed to run linuxdeploy` — a `linuxdeploy` tool download/run
issue, separate from the app itself); the `.deb` bundle and the raw
`target/release/rusted-claude-meter` binary both built and ran fine.

## Implications for `install.sh`

- Download the AppImage from the `latest` release via the GitHub API rather
  than pinning a version.
- Install to `~/Applications`, `chmod +x`, and register a `.desktop` entry so
  the app shows up in launchers (AppImages don't do this themselves).
- Print, rather than silently handle, the two known rough edges: missing
  `webkit2gtk` on distros that don't ship it by default, and a stale
  `libjxl` — both are runtime/environment issues the script can't reliably
  detect or fix across every distro, so surfacing them to the user beats
  guessing. `WEBKIT_DISABLE_DMABUF_RENDERER=1` is a fallback workaround, not
  a real fix — the real fix is an up-to-date `libjxl` (`sudo pacman -Syu` /
  distro equivalent) or building from source against system libs.
