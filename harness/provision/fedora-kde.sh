#!/usr/bin/env bash
# Provision the Fedora + KDE Plasma VM: desktop, VNC, and the runtime libraries
# the AppImage needs. No build toolchain — this VM only consumes artifacts
# built in the Ubuntu VM.
#
# Idempotent — safe to re-run after a failed step.

set -euo pipefail
cd "$(dirname "$0")"

echo "==> KDE Plasma"
# The Plasma group without the full Fedora KDE spin's application set.
sudo dnf install -y -q --setopt=install_weak_deps=False \
    @base-x \
    plasma-desktop \
    plasma-workspace-x11 \
    kwalletmanager5 \
    dbus-x11 \
    libnotify

echo "==> VNC + screenshot tooling"
# Modern Fedora has no `xorg-x11-utils` / `xorg-x11-server-utils` umbrella
# packages; xdpyinfo and xhost are their own packages now. util-linux is for
# `mcookie`, which mints the X auth cookie.
sudo dnf install -y -q --setopt=install_weak_deps=False \
    tigervnc-server-minimal \
    xdpyinfo \
    xhost \
    xdotool \
    util-linux \
    ImageMagick

echo "==> AppImage runtime dependencies"
# An AppImage bundles the app's own libraries but not the webkit/GTK stack it
# links against, and Fedora's KDE group pulls in Qt, not GTK. Without these the
# AppImage fails to start with a bare loader error.
sudo dnf install -y -q \
    webkit2gtk4.1 \
    gtk3 \
    libappindicator-gtk3 \
    librsvg2 \
    fuse-libs

echo "==> Desktop session"
export RCM_SESSION_CMD="startplasma-x11"
export RCM_XDG_CURRENT_DESKTOP="KDE"

# WebKitGTK 2.42+ renders through DMA-BUF by default and aborts with
# "Could not create default EGL display: EGL_BAD_PARAMETER" when there is no
# GPU behind the display — which is every VM here. Ubuntu's older webkit2gtk
# does not need this, so it is set for Fedora only rather than globally.
export RCM_SESSION_EXTRA_ENV='export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1'
# shellcheck source=./common.sh
. ./common.sh
write_session_env
write_desktop_launcher
start_desktop

echo
echo "KDE VM ready. Connect: open vnc://localhost:5902"
echo
echo "Note: Plasma's Secret Service provider is KWallet, and creating a wallet"
echo "cannot be done unattended without weakening it. The first time the app"
echo "saves a session key, KWallet will prompt in the VNC window — accept the"
echo "default (blank password is fine; the VM holds only demo data). That one"
echo "click is deliberate: it exercises the real KDE credential path rather"
echo "than substituting gnome-keyring to avoid the dialog."
