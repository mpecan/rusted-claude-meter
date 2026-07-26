#!/usr/bin/env bash
# Provision the Ubuntu 24.04 + GNOME VM: desktop, keyring, VNC, and the Tauri
# build toolchain (this VM builds the .deb and the AppImage for both VMs).
#
# Idempotent — safe to re-run after a failed step.

set -euo pipefail
cd "$(dirname "$0")"

export DEBIAN_FRONTEND=noninteractive

echo "==> apt update"
sudo apt-get update -qq

echo "==> GNOME desktop + session plumbing"
# ubuntu-desktop-minimal, not ubuntu-desktop: the full seed drags in an office
# suite and games that add ~2GB and nothing to this test.
sudo apt-get install -y -qq --no-install-recommends \
    ubuntu-desktop-minimal \
    gnome-shell-extension-appindicator \
    gnome-shell-extension-manager \
    gnome-keyring \
    seahorse \
    dbus-x11 \
    libnotify-bin

echo "==> VNC + screenshot tooling"
# imagemagick provides `import`, which is how vm.sh captures the tray without
# a human at the VNC window.
sudo apt-get install -y -qq --no-install-recommends \
    tigervnc-standalone-server \
    tigervnc-tools \
    x11-utils \
    xdotool \
    imagemagick

echo "==> Tauri build dependencies"
# Same list as .github/actions/tauri-linux-deps/action.yml — keep in step with
# it, or a build here can succeed where CI fails (or vice versa).
sudo apt-get install -y -qq \
    libwebkit2gtk-4.1-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libxdo-dev \
    libssl-dev \
    libgtk-3-dev \
    build-essential \
    curl \
    file \
    pkg-config \
    git \
    rsync \
    xdg-utils

echo "==> Node (for the frontend build)"
if ! command -v node >/dev/null; then
    curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -
    sudo apt-get install -y -qq nodejs
fi
node --version

echo "==> Rust"
if ! [ -x "$HOME/.cargo/bin/cargo" ]; then
    curl -fsSL https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal
fi
# shellcheck disable=SC1091
. "$HOME/.cargo/env"
rustc --version

echo "==> GNOME tray: enable the AppIndicator extension"
# Without this, GNOME Shell hides every StatusNotifierItem — the app's tray
# simply does not appear. That is the single most important Linux-specific
# behaviour this harness exists to demonstrate, so it is enabled here as the
# baseline and toggled off deliberately as a test (see harness/README.md).
mkdir -p "$HOME/.config"
cat >"$HOME/.config/rcm-enable-appindicator.sh" <<'EOF'
#!/usr/bin/env bash
# Run inside the graphical session (gnome-extensions needs the session bus).
gnome-extensions enable ubuntu-appindicators@ubuntu.com 2>/dev/null ||
    gnome-extensions enable appindicatorsupport@rgcjonas.gmail.com 2>/dev/null ||
    echo "no appindicator extension found to enable" >&2
EOF
chmod +x "$HOME/.config/rcm-enable-appindicator.sh"

echo "==> Desktop session"
# Mirrors /usr/share/xsessions/ubuntu-xorg.desktop, which is how Ubuntu itself
# starts GNOME on X11. GNOME_SHELL_SESSION_MODE picks the Ubuntu session
# (dock, indicators) over upstream GNOME's.
export RCM_SESSION_CMD="gnome-session --session=ubuntu"
export RCM_XDG_CURRENT_DESKTOP="ubuntu:GNOME"
export RCM_SESSION_EXTRA_ENV='export GNOME_SHELL_SESSION_MODE=ubuntu'

# The app stores the session key in the Secret Service and has no plaintext
# fallback by design (src-tauri/src/store.rs). A cloud image was never logged
# into interactively, so it has no login keyring at all — the first save fails
# with StoreError::Unavailable, which looks like an app bug rather than a
# missing fixture. Creating the keyring with a blank password makes the
# *normal* path the default; killing the daemon to see the failure is then a
# deliberate test (see harness/README.md), not the out-of-box state.
#
# Feeding a blank password on stdin is what creates login.keyring when none
# exists. Create it, then stop the daemon again and let the desktop session
# start its own — a blank-password login keyring needs no unlock prompt, so the
# session's daemon simply adopts it. (`pkill -x`, never `-f`: the full command
# line of this very script contains the daemon name.)
# Single-quoted on purpose: this block is emitted verbatim into rcm-session and
# expanded there, not here.
# shellcheck disable=SC2016
export RCM_KEYRING_SETUP='
if [ ! -f "$HOME/.local/share/keyrings/login.keyring" ]; then
    mkdir -p "$HOME/.local/share/keyrings"
    pkill -x gnome-keyring-daemon 2>/dev/null || true
    sleep 2
    eval "$(printf "\n" | gnome-keyring-daemon --unlock --components=secrets --daemonize)"
    sleep 2
    printf "login\n" > "$HOME/.local/share/keyrings/default"
    pkill -x gnome-keyring-daemon 2>/dev/null || true
    sleep 2
fi
'
# shellcheck source=./common.sh
. ./common.sh
write_session_env
write_desktop_launcher
start_desktop

echo
echo "GNOME VM ready. Connect: open vnc://localhost:5901"
