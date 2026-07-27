#!/usr/bin/env bash
# Provision the Arch x86_64 rootfs inside the rcm-arch VM.
#
# Run over `limactl shell` by harness/bin/arch.sh, not from a `provision:`
# block, so a failed step can be read, fixed and re-run on its own.
#
# The VM's userspace is aarch64 Ubuntu; everything Arch lives in a rootfs at
# $ROOT and runs only because the instance sets `vmOpts.vz.rosetta.binfmt`.
#
# Plain chroot and systemd-nspawn were both tried and both fail here. What
# works is `arch-chroot` over a rootfs bind-mounted onto itself. The failure
# modes are written up in harness/README.md under "Arch x86_64", because not
# one of them names its own cause.

set -euo pipefail

ROOT=/opt/arch-x86
BOOTSTRAP_URL=https://geo.mirror.pkgbuild.com/iso/latest/archlinux-bootstrap-x86_64.tar.zst
MIRROR='Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch'

log() { echo "==> $*"; }

in_arch() { sudo arch-chroot "$ROOT" /bin/bash -c "$1"; }

provision_host() {
    log "installing host prerequisites"
    sudo apt-get update -qq
    # arch-install-scripts is what provides arch-chroot; see the header.
    sudo apt-get install -y -qq zstd curl arch-install-scripts
}

provision_bootstrap() {
    if [ -x "$ROOT/usr/bin/pacman" ]; then
        log "bootstrap already unpacked at $ROOT"
        return 0
    fi
    log "downloading Arch x86_64 bootstrap"
    curl -fsSL -o /tmp/arch-bootstrap.tar.zst "$BOOTSTRAP_URL"

    log "unpacking to $ROOT"
    sudo mkdir -p "$ROOT"
    # --strip-components=1: the tarball wraps everything in root.x86_64/. The
    # xattr warnings it emits are security.capability bits GNU tar does not
    # understand and nothing here needs.
    sudo tar -I zstd -xf /tmp/arch-bootstrap.tar.zst -C "$ROOT" --strip-components=1
    rm -f /tmp/arch-bootstrap.tar.zst
}

# Idempotent, and needed again after every VM reboot — a bind mount does not
# survive one, and its absence comes back as a disk-space error.
mount_self() {
    mountpoint -q "$ROOT" || sudo mount --bind "$ROOT" "$ROOT"
}

# Lima's own Rosetta registration masks ELF bytes 8-15 as must-be-zero, but a
# type-2 AppImage stamps "AI\x02" into that e_ident padding, so it never
# matches and AppImages die with "Exec format error". Same interpreter, those
# eight bytes masked out. Lost on reboot, like the bind mount above.
register_appimage_binfmt() {
    [ -e /proc/sys/fs/binfmt_misc/rosetta-appimage ] && return 0
    [ -e /proc/sys/fs/binfmt_misc/rosetta ] || {
        echo "no rosetta binfmt handler — is vmOpts.vz.rosetta.binfmt set?" >&2
        return 1
    }
    log "registering an AppImage-tolerant Rosetta binfmt handler"
    local magic mask
    magic='\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\x00\x3e\x00'
    mask='\xff\xff\xff\xff\xff\xfe\xfe\x00\x00\x00\x00\x00\x00\x00\x00\x00\xfe\xff\xff\xff'
    echo ":rosetta-appimage:M::${magic}:${mask}:/mnt/lima-rosetta/rosetta:OCF" |
        sudo tee /proc/sys/fs/binfmt_misc/register >/dev/null
}

provision_pacman() {
    log "configuring mirror and keyring"
    echo "$MIRROR" | sudo tee "$ROOT/etc/pacman.d/mirrorlist" >/dev/null

    # pacman 7 sandboxes downloads with Landlock, which the Lima guest kernel
    # lacks; without this every pacman call fails in that sandbox.
    grep -q '^DisableSandbox' "$ROOT/etc/pacman.conf" ||
        sudo sed -i '/^\[options\]/a DisableSandbox' "$ROOT/etc/pacman.conf"

    # Slow: gpg gathering entropy, translated. Minutes, not seconds.
    log "initialising the keyring"
    in_arch 'pacman-key --init >/dev/null 2>&1 && pacman-key --populate archlinux >/dev/null 2>&1'
    in_arch 'pacman -Sy --noconfirm'
}

provision_packages() {
    log "installing toolchain and app dependencies"
    # Deliberately the PKGBUILD's own depends+makedepends, so anything missing
    # there surfaces here as a build failure instead of being papered over.
    in_arch 'pacman -S --noconfirm --needed base-devel git sudo \
        rust nodejs npm \
        webkit2gtk-4.1 libayatana-appindicator librsvg gtk3 openssl xdotool'
}

provision_user() {
    # makepkg refuses to run as root, by design and with no override.
    log "creating the build user"
    in_arch 'id builder >/dev/null 2>&1 || useradd -m -s /bin/bash builder'
    in_arch 'echo "builder ALL=(ALL) NOPASSWD: ALL" >/etc/sudoers.d/builder && chmod 440 /etc/sudoers.d/builder'
}

# The two steps that a reboot undoes. `arch.sh` calls this on every command so
# the instance is usable after `limactl stop`/`start` without a reprovision.
remount() {
    mount_self
    register_appimage_binfmt
}

main() {
    case "${1:-all}" in
    remount)
        remount
        ;;
    all)
        provision_host
        provision_bootstrap
        remount
        provision_pacman
        provision_packages
        provision_user
        log "provisioned $(in_arch 'uname -m') Arch in $ROOT"
        ;;
    *)
        echo "usage: arch-x86.sh [all|remount]" >&2
        exit 1
        ;;
    esac
}

main "$@"
