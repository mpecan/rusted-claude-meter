#!/usr/bin/env bash
# Provision the Arch x86_64 rootfs inside the rcm-arch VM.
#
# Run over `limactl shell` by harness/bin/arch.sh, not from a `provision:`
# block, so a failed step can be read, fixed and re-run on its own.
#
# The VM's own userspace is aarch64 Ubuntu. Everything Arch lives in a rootfs
# at $ROOT and only runs at all because the instance has
# `vmOpts.vz.rosetta.binfmt` — see harness/lima/arch-x86.yaml.
#
# Three approaches were tried before this one; the notes are here because each
# fails in a way that does not name its own cause:
#
# 1. Plain `chroot`. Rosetta reads /proc/self/exe on every exec, so without
#    /proc mounted every x86_64 binary dies with
#    "rosetta error: Unable to open /proc/self/exe: 2" — which reads as a
#    corrupt binary rather than a missing mount.
# 2. `chroot` with /proc, /sys, /dev, /run bind-mounted. Rosetta works, but
#    pacman fails with "could not determine cachedir mount point" and then
#    "not enough free disk space" on a disk that is 8% full: /proc/self/mounts
#    inside the chroot still describes the *host's* paths, so pacman cannot
#    match /var/cache/pacman/pkg to any mount and gives up on the space check.
# 3. `systemd-nspawn`. Refuses a pre-populated /dev, and then fails with
#    "Failed to determine whether the unified cgroups hierarchy is used" —
#    there is no cgroup setup to inherit under `limactl shell`.
#
# What works is `arch-chroot` (Ubuntu's arch-install-scripts) over a rootfs
# that has been bind-mounted onto itself. The self-bind is the load-bearing
# part: it makes $ROOT a real mount point, so "/" appears in the chroot's
# mount table and pacman's space check resolves. Without it arch-chroot warns
# "not a mountpoint" and pacman fails exactly as in (2).

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

# Rosetta will not run an AppImage without this, and the reason is not
# guessable from the error ("cannot execute binary file: Exec format error" on
# a binary that `file` calls a perfectly good x86-64 ELF).
#
# Lima registers Rosetta with this mask:
#   magic 7f454c46 02010100 0000000000000000 02003e00
#   mask  ffffffff fffefe00 ffffffffffffffff feffffff
#                           ^^^^^^^^^^^^^^^^
# Those eight mask bytes are 0xff, so bytes 8-15 of the header — the unused
# e_ident padding — have to be zero for the handler to match. A type-2
# AppImage stamps its own magic there ("AI\x02", 41 49 02), so it never
# matches and the kernel finds no interpreter at all.
#
# This registers the same interpreter with those eight bytes masked out, which
# is the whole fix. Also lost on reboot, like the bind mount above.
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

    # pacman 7 sandboxes downloads with Landlock and the Lima guest kernel does
    # not have it: without this, every operation fails with "restricting
    # filesystem access failed because Landlock is not supported by the
    # kernel!" then "switching to sandbox user 'alpm' failed!". Arch containers
    # under podman hit the same wall.
    grep -q '^DisableSandbox' "$ROOT/etc/pacman.conf" ||
        sudo sed -i '/^\[options\]/a DisableSandbox' "$ROOT/etc/pacman.conf"

    # Slow: gpg gathering entropy, translated. Minutes, not seconds.
    log "initialising the keyring"
    in_arch 'pacman-key --init >/dev/null 2>&1 && pacman-key --populate archlinux >/dev/null 2>&1'
    in_arch 'pacman -Sy --noconfirm'
}

provision_packages() {
    log "installing toolchain and app dependencies"
    # webkit2gtk-4.1 and friends are both build and runtime dependency on Arch;
    # there is no separate -dev split as on Debian. This list is deliberately
    # the PKGBUILD's depends+makedepends, so a missing entry there shows up
    # here as a build failure rather than being silently papered over.
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
