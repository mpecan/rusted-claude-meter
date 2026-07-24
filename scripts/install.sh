#!/usr/bin/env bash
# Install Rusted Claude Meter on Linux (x86_64) from the latest GitHub release.
# Downloads the AppImage, installs it under ~/Applications, and registers a
# desktop entry + icon so it shows up in app launchers.
set -euo pipefail

REPO="mpecan/rusted-claude-meter"
INSTALL_DIR="${HOME}/Applications"
BIN_NAME="RustedClaudeMeter.AppImage"
DESKTOP_DIR="${HOME}/.local/share/applications"
ICON_DIR="${HOME}/.local/share/icons/hicolor/256x256/apps"

if [[ "$(uname -m)" != "x86_64" ]]; then
  echo "error: only x86_64 Linux builds are published" >&2
  exit 1
fi

command -v curl >/dev/null || { echo "error: curl is required" >&2; exit 1; }

echo "Fetching latest release info for ${REPO}..."
ASSET_URL=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep -o '"browser_download_url": *"[^"]*amd64\.AppImage"' \
  | head -1 \
  | cut -d'"' -f4)

if [[ -z "${ASSET_URL}" ]]; then
  echo "error: could not find an AppImage asset in the latest release" >&2
  exit 1
fi

mkdir -p "${INSTALL_DIR}" "${DESKTOP_DIR}" "${ICON_DIR}"

echo "Downloading ${ASSET_URL}..."
curl -fsSL "${ASSET_URL}" -o "${INSTALL_DIR}/${BIN_NAME}"
chmod +x "${INSTALL_DIR}/${BIN_NAME}"

cat > "${DESKTOP_DIR}/rusted-claude-meter.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Rusted Claude Meter
Comment=Tray app showing your Claude plan usage
Exec=${INSTALL_DIR}/${BIN_NAME}
Icon=rusted-claude-meter
Categories=Utility;
Terminal=false
EOF

echo
echo "Installed to ${INSTALL_DIR}/${BIN_NAME}"
echo "Desktop entry: ${DESKTOP_DIR}/rusted-claude-meter.desktop"
echo
echo "Runtime dependencies (install via your distro's package manager if missing):"
echo "  webkit2gtk (4.1), libayatana-appindicator, librsvg, fuse2"
echo
echo "On GNOME, install the AppIndicator extension for the tray icon to appear:"
echo "  https://extensions.gnome.org/extension/615/appindicator-support/"
echo
echo "Run it with: ${INSTALL_DIR}/${BIN_NAME}"
echo "If you see 'Could not create surfaceless EGL display: EGL_BAD_ALLOC', retry with:"
echo "  WEBKIT_DISABLE_DMABUF_RENDERER=1 ${INSTALL_DIR}/${BIN_NAME}"
