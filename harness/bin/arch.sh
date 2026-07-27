#!/usr/bin/env bash
# Drive the Arch x86_64 packaging VM.
#
#   arch.sh up          create (if needed), boot and provision
#   arch.sh down        stop
#   arch.sh delete      stop and destroy
#   arch.sh shell       a shell inside the Arch x86_64 rootfs
#   arch.sh makepkg     build packaging/aur/PKGBUILD, package into artifacts/
#   arch.sh install-sh  run scripts/install.sh against this pacman host
#   arch.sh status      what the instance and the rootfs look like
#
# This is the only x86_64 target in the harness. Everything else is aarch64,
# because the Macs this is developed on are Apple Silicon. See
# harness/lima/arch-x86.yaml for why it is a Rosetta VM rather than a container
# or an emulated guest, and harness/provision/arch-x86.sh for the three
# approaches that did not work before this one.
#
# Deliberately headless: it answers packaging questions. Anything with a window
# belongs on the GNOME/KDE targets.

set -euo pipefail

HARNESS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="$(cd "$HARNESS/.." && pwd)"
ARTIFACTS="$HARNESS/artifacts"
INSTANCE=rcm-arch
ROOT=/opt/arch-x86

die() {
    echo "error: $*" >&2
    exit 1
}

running() {
    [ "$(limactl list --format '{{.Status}}' "$INSTANCE" 2>/dev/null)" = "Running" ]
}

need_running() {
    running || die "$INSTANCE is not running — start it with 'just arch up'"
}

# Everything Arch runs through arch-chroot; see the provisioning script for why
# plain chroot and systemd-nspawn both fail here.
in_vm() { limactl shell --workdir / "$INSTANCE" -- "$@"; }
in_arch() { in_vm sudo arch-chroot "$ROOT" /bin/bash -c "$1"; }

# A bind mount and a binfmt registration do not survive a reboot, and both fail
# in ways that do not name themselves (a disk-space error, an exec-format
# error). Cheap enough to just re-assert before every command.
remount() { in_vm sudo bash "$ROOT/../arch-provision.sh" remount >/dev/null 2>&1 || true; }

cmd_up() {
    if ! limactl list --format '{{.Name}}' 2>/dev/null | grep -qx "$INSTANCE"; then
        echo "==> creating $INSTANCE"
        local rendered="$ARTIFACTS/$INSTANCE.rendered.yaml"
        mkdir -p "$ARTIFACTS"
        # Lima has no environment expansion for mount paths, so the template
        # carries placeholders and they are substituted here.
        sed -e "s|__REPO__|$REPO|" -e "s|__ARTIFACTS__|$ARTIFACTS|" \
            "$HARNESS/lima/arch-x86.yaml" >"$rendered"
        limactl create --name "$INSTANCE" --tty=false "$rendered"
    fi
    running || limactl start "$INSTANCE"

    echo "==> provisioning (first run downloads ~120MiB and builds a keyring; minutes)"
    in_vm sudo cp /repo/harness/provision/arch-x86.sh /opt/arch-provision.sh
    in_vm sudo bash /opt/arch-provision.sh all
}

cmd_down() { limactl stop "$INSTANCE"; }

cmd_delete() {
    limactl stop "$INSTANCE" 2>/dev/null || true
    limactl delete "$INSTANCE"
}

cmd_shell() {
    need_running
    remount
    in_vm sudo arch-chroot "$ROOT" /bin/bash
}

# The point of this instance. Builds the PKGBUILD exactly as an AUR user would.
cmd_makepkg() {
    need_running
    remount
    echo "==> building packaging/aur/PKGBUILD (a full Tauri build, translated — slow)"
    in_vm sudo mkdir -p "$ROOT/root/aur"
    in_vm sudo cp /repo/packaging/aur/PKGBUILD "$ROOT/root/aur/PKGBUILD"
    # makepkg refuses to run as root, hence the builder user.
    in_arch '
        set -e
        rm -rf /home/builder/aur && mkdir -p /home/builder/aur
        cp /root/aur/PKGBUILD /home/builder/aur/
        chown -R builder:builder /home/builder/aur
        cd /home/builder/aur && sudo -u builder makepkg -f --noconfirm
    '
    echo "==> copying the package out to harness/artifacts/"
    in_vm sudo bash -c "cp $ROOT/home/builder/aur/*.pkg.tar.zst /artifacts/ 2>/dev/null" ||
        die "makepkg produced no package"
    ls -la "$ARTIFACTS"/*.pkg.tar.zst
}

# Exercises the pacman detection, the refusal, and --force, on a host where
# `command -v pacman` is genuinely true.
cmd_install_sh() {
    need_running
    remount
    in_vm sudo cp /repo/scripts/install.sh "$ROOT/root/install.sh"
    echo "==> plain run (expect the refusal and exit 1)"
    in_arch 'bash /root/install.sh; echo "exit=$?"' || true
    echo
    echo "==> --force (downloads the AppImage, extracts icons, writes the entry)"
    in_arch '
        export HOME=/root
        rm -rf /root/.local /root/Applications
        bash /root/install.sh --force
        echo "--- icons ---"
        find /root/.local/share/icons -name "*.png"
        echo "--- desktop entry ---"
        cat /root/.local/share/applications/rusted-claude-meter.desktop
    '
}

cmd_status() {
    limactl list "$INSTANCE" 2>/dev/null || echo "(no $INSTANCE instance)"
    running || return 0
    echo "--- binfmt handlers ---"
    in_vm ls /proc/sys/fs/binfmt_misc/
    echo "--- rootfs ---"
    in_arch 'echo "arch: $(uname -m)"; echo "rust: $(rustc --version 2>/dev/null || echo none)"; pacman -Q webkit2gtk-4.1 libjxl 2>/dev/null'
}

case "${1:-}" in
up) cmd_up ;;
down) cmd_down ;;
delete) cmd_delete ;;
shell) cmd_shell ;;
makepkg) cmd_makepkg ;;
install-sh) cmd_install_sh ;;
status) cmd_status ;;
*)
    sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
