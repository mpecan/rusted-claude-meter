#!/usr/bin/env bash
# Install Rusted Claude Meter on Linux (x86_64) from the latest GitHub release.
# Fallback for distros with no native package: downloads the AppImage,
# installs it under ~/Applications, and registers a desktop entry + icons so
# it shows up in app launchers. On Arch (and derivatives), prefer the PKGBUILD
# in packaging/aur — it builds against system libs and so avoids the AppImage's
# bundled-WebKit version skew entirely (see issue #50). Pass --force to install
# the AppImage anyway.
set -euo pipefail

REPO="mpecan/rusted-claude-meter"
INSTALL_DIR="${HOME}/Applications"
BIN_NAME="RustedClaudeMeter.AppImage"
DESKTOP_DIR="${HOME}/.local/share/applications"
ICON_ROOT="${HOME}/.local/share/icons/hicolor"

FORCE=0
for arg in "$@"; do
  case "$arg" in
    --force) FORCE=1 ;;
    -h | --help)
      echo "usage: install.sh [--force]"
      echo "  --force  install the AppImage even on Arch-based distros"
      exit 0
      ;;
    *)
      echo "error: unknown argument '$arg' (try --help)" >&2
      exit 1
      ;;
  esac
done

if [[ "${FORCE}" -eq 0 ]] && command -v pacman >/dev/null 2>&1; then
  cat >&2 <<'MSG'
Arch Linux (or a derivative) detected. Prefer the PKGBUILD in packaging/aur —
it builds against your system's webkit2gtk instead of shipping its own, which
avoids the AppImage's bundled-library version skew (see issue #50):

  git clone https://github.com/mpecan/rusted-claude-meter
  cd rusted-claude-meter/packaging/aur && makepkg -si

Re-run with --force to install the AppImage here anyway.
MSG
  exit 1
fi

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "error: this installer is for Linux only" >&2
  exit 1
fi

if [[ "$(uname -m)" != "x86_64" ]]; then
  echo "error: only x86_64 Linux builds are published" >&2
  exit 1
fi

command -v curl >/dev/null || { echo "error: curl is required" >&2; exit 1; }

echo "Fetching latest release info for ${REPO}..."
# Fetch first, parse second. Under `set -o pipefail` any consumer that exits
# early (`head -1`, `grep -m1`) closes the pipe and kills whatever is upstream
# with EPIPE — grep in the first case, curl in the second ("curl: Failed
# writing body", exit 23). Short responses hide it on one machine and it fails
# every time on another. In a variable there is no upstream left to kill.
RELEASE_JSON=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest")
ASSET_URL=$(printf '%s' "${RELEASE_JSON}" \
  | grep -m1 -o '"browser_download_url": *"[^"]*amd64\.AppImage"' \
  | cut -d'"' -f4)

if [[ -z "${ASSET_URL}" ]]; then
  echo "error: could not find an AppImage asset in the latest release" >&2
  exit 1
fi

mkdir -p "${INSTALL_DIR}" "${DESKTOP_DIR}" "${ICON_ROOT}"

echo "Downloading ${ASSET_URL}..."
curl -fSL --progress-bar "${ASSET_URL}" -o "${INSTALL_DIR}/${BIN_NAME}"
chmod +x "${INSTALL_DIR}/${BIN_NAME}"

# Launcher icons live inside the AppImage; without copying them out, the
# desktop entry's Icon= key resolves to nothing and launchers show a blank
# tile. --appimage-extract needs no FUSE, so this works on hosts where
# *running* the AppImage would not.
extract_icons() {
  local root="$1" src
  # Newer AppImage runtimes accept a path pattern and extract only that;
  # older ones ignore it and unpack everything, which also works.
  (cd "${root}" && "${INSTALL_DIR}/${BIN_NAME}" --appimage-extract 'usr/share/icons/*' >/dev/null 2>&1) || true
  if [[ ! -d "${root}/squashfs-root/usr/share/icons/hicolor" ]]; then
    (cd "${root}" && "${INSTALL_DIR}/${BIN_NAME}" --appimage-extract >/dev/null 2>&1) || return 1
  fi

  if [[ -d "${root}/squashfs-root/usr/share/icons/hicolor" ]]; then
    cp -r "${root}/squashfs-root/usr/share/icons/hicolor/." "${ICON_ROOT}/"
    return 0
  fi

  # linuxdeploy also drops a copy at the AppDir root. No size to read off it
  # without an image tool, so file it under the size the project actually
  # ships; hicolor lookup falls back to scaling anyway.
  for src in "${root}/squashfs-root/rusted-claude-meter.png" "${root}/squashfs-root/.DirIcon"; do
    [[ -f "${src}" ]] || continue
    mkdir -p "${ICON_ROOT}/128x128/apps"
    cp "${src}" "${ICON_ROOT}/128x128/apps/rusted-claude-meter.png"
    return 0
  done

  return 1
}

install_icons() {
  local tmpdir status=0
  tmpdir=$(mktemp -d)
  extract_icons "${tmpdir}" || status=1
  rm -rf "${tmpdir}"
  return "${status}"
}

if install_icons; then
  echo "Installed launcher icons under ${ICON_ROOT}"
else
  echo "warning: could not extract launcher icons from the AppImage;" >&2
  echo "         the app will still run, but its launcher tile may be blank." >&2
fi

# StartupWMClass matches what the app sets as its WM class (same value the
# .deb's own desktop entry uses), so the running window associates with this
# launcher entry instead of showing up as a second, unnamed one.
cat > "${DESKTOP_DIR}/rusted-claude-meter.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Rusted Claude Meter
Comment=Claude plan usage meter in your tray
Exec=${INSTALL_DIR}/${BIN_NAME}
Icon=rusted-claude-meter
StartupWMClass=rusted-claude-meter
Categories=Utility;
Terminal=false
EOF

command -v update-desktop-database >/dev/null 2>&1 \
  && update-desktop-database "${DESKTOP_DIR}" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 \
  && gtk-update-icon-cache -qtf "${ICON_ROOT}" >/dev/null 2>&1 || true

echo
echo "Installed to ${INSTALL_DIR}/${BIN_NAME}"
echo "Desktop entry: ${DESKTOP_DIR}/rusted-claude-meter.desktop"
echo
echo "Runtime dependencies (install via your distro's package manager if missing):"
echo "  webkit2gtk (4.1), libayatana-appindicator, librsvg, libxdo, fuse2"
echo
echo "On GNOME, install the AppIndicator extension for the tray icon to appear:"
echo "  https://extensions.gnome.org/extension/615/appindicator-support/"
echo
echo "Run it with: ${INSTALL_DIR}/${BIN_NAME}"
echo
echo "If it aborts with an EGL error, or the window opens blank, see the"
echo "troubleshooting section in docs/linux.md — the usual first thing to try is:"
echo "  WEBKIT_DISABLE_DMABUF_RENDERER=1 ${INSTALL_DIR}/${BIN_NAME}"
echo "If that does not settle it, the .deb (or a source build) uses your system's"
echo "WebKit instead of the bundled one and sidesteps the whole class of problem."
