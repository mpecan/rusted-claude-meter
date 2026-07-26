#!/usr/bin/env bash
# Drive the harness VMs.
#
#   vm.sh up      gnome|kde   create (if needed), boot and provision
#   vm.sh down    gnome|kde   stop
#   vm.sh delete  gnome|kde   stop and destroy
#   vm.sh shell   gnome|kde   a shell inside the VM
#   vm.sh vnc     gnome|kde   open the desktop in macOS Screen Sharing
#   vm.sh shot    gnome|kde   screenshot the desktop to harness/artifacts/
#   vm.sh install gnome|kde   install the built app (.deb / AppImage)
#   vm.sh launch  gnome|kde   start the app inside the desktop session
#   vm.sh logs    gnome|kde   tail the app's output
#   vm.sh status              list both instances
#
# See harness/README.md.

set -euo pipefail

HARNESS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="$(cd "$HARNESS/.." && pwd)"
ARTIFACTS="$HARNESS/artifacts"

die() {
    echo "error: $*" >&2
    exit 1
}

# Map the short name used on the command line onto its instance, template,
# provisioning script and forwarded VNC port.
resolve() {
    case "${1:-}" in
    gnome)
        INSTANCE=rcm-gnome
        TEMPLATE="$HARNESS/lima/ubuntu-gnome.yaml"
        PROVISION=ubuntu-gnome.sh
        VNC_PORT=5901
        ;;
    kde)
        INSTANCE=rcm-kde
        TEMPLATE="$HARNESS/lima/fedora-kde.yaml"
        PROVISION=fedora-kde.sh
        VNC_PORT=5902
        ;;
    *) die "expected 'gnome' or 'kde', got '${1:-}'" ;;
    esac
}

# `limactl shell` defaults to cd'ing to the host's working directory inside the
# guest, which does not exist there and prints two `cd` errors before every
# command. Everything here is path-independent, so pin it to /.
lima_shell() {
    local instance="$1"
    shift
    limactl shell --workdir / "$instance" "$@"
}

instance_exists() {
    limactl list --quiet 2>/dev/null | grep -qx "$INSTANCE"
}

instance_running() {
    [ "$(limactl list --format '{{.Status}}' "$INSTANCE" 2>/dev/null)" = "Running" ]
}

# Lima has no environment expansion for mount paths, so the host paths are
# substituted into a rendered copy of the template. Written next to the
# artifacts dir (gitignored) rather than a temp file, so a failed create can be
# inspected.
render_template() {
    mkdir -p "$ARTIFACTS"
    # Not a dotfile: limactl rejects template filenames starting with `.`.
    RENDERED="$ARTIFACTS/$INSTANCE.rendered.yaml"
    sed -e "s|__REPO__|$REPO|g" -e "s|__ARTIFACTS__|$ARTIFACTS|g" \
        "$TEMPLATE" >"$RENDERED"
}

cmd_up() {
    resolve "$1"
    if ! instance_exists; then
        render_template
        echo "==> creating $INSTANCE (first boot downloads the cloud image)"
        limactl create --name="$INSTANCE" --tty=false "$RENDERED"
    fi
    instance_running || limactl start "$INSTANCE"

    echo "==> provisioning $INSTANCE (this takes a while on first run)"
    # Copied in rather than run from a mount: the KDE VM does not mount the
    # repo at all, and copying keeps both paths identical.
    lima_shell "$INSTANCE" mkdir -p /tmp/rcm-provision
    limactl copy "$HARNESS/provision/common.sh" "$INSTANCE:/tmp/rcm-provision/common.sh"
    limactl copy "$HARNESS/provision/$PROVISION" "$INSTANCE:/tmp/rcm-provision/$PROVISION"
    lima_shell "$INSTANCE" chmod +x "/tmp/rcm-provision/$PROVISION"
    lima_shell "$INSTANCE" "/tmp/rcm-provision/$PROVISION"

    echo
    echo "$INSTANCE ready — desktop at vnc://localhost:$VNC_PORT"
}

cmd_down() {
    resolve "$1"
    limactl stop "$INSTANCE"
}

cmd_delete() {
    resolve "$1"
    limactl stop --force "$INSTANCE" 2>/dev/null || true
    limactl delete "$INSTANCE"
}

cmd_shell() {
    resolve "$1"
    shift
    lima_shell "$INSTANCE" "$@"
}

cmd_vnc() {
    resolve "$1"
    open "vnc://localhost:$VNC_PORT"
}

cmd_shot() {
    resolve "$1"
    local name="${2:-$INSTANCE}"
    local out="$ARTIFACTS/$name.png"
    mkdir -p "$ARTIFACTS"
    # `import -window root` against the Xvnc display: captures the desktop as
    # the user sees it, including the tray, with nobody watching the VNC
    # window. This is what makes tray rendering checkable from a script.
    # Through rcm-run-app so it inherits the session's DISPLAY and X cookie.
    lima_shell "$INSTANCE" /usr/local/bin/rcm-run-app import -window root /tmp/rcm-shot.png
    limactl copy "$INSTANCE:/tmp/rcm-shot.png" "$out"
    echo "$out"
}

cmd_install() {
    resolve "$1"
    case "$INSTANCE" in
    rcm-gnome)
        local deb
        # shellcheck disable=SC2012 # our own bundle filenames, no exotic characters
        deb="$(ls -t "$ARTIFACTS"/*.deb 2>/dev/null | head -1)" ||
            die "no .deb in $ARTIFACTS — run 'just linux-build' first"
        echo "==> installing $(basename "$deb")"
        lima_shell "$INSTANCE" sudo apt-get install -y "/artifacts/$(basename "$deb")"
        ;;
    rcm-kde)
        local appimage
        # shellcheck disable=SC2012 # our own bundle filenames, no exotic characters
        appimage="$(ls -t "$ARTIFACTS"/*.AppImage 2>/dev/null | head -1)" ||
            die "no .AppImage in $ARTIFACTS — run 'just linux-build' first"
        echo "==> staging $(basename "$appimage")"
        lima_shell "$INSTANCE" bash -c \
            "install -m755 '/artifacts/$(basename "$appimage")' \"\$HOME/rusted-claude-meter.AppImage\""
        # And the unbundled binary, which is what `launch kde binary` runs.
        if [ -f "$ARTIFACTS/rusted-claude-meter" ]; then
            lima_shell "$INSTANCE" bash -c \
                "install -m755 /artifacts/rusted-claude-meter \"\$HOME/rusted-claude-meter\""
        fi
        ;;
    esac
    echo "installed. Launch it with: $0 launch $1"
}

# Start the app inside the graphical session rather than the ssh session.
# `rcm-run-app` (installed by provisioning) supplies the display, the session
# bus and RCM_API_BASE_URL — without the session bus the app gets no tray and
# no notifications, which looks like an app bug rather than a launch one.
cmd_launch() {
    resolve "$1"
    local exe
    case "$INSTANCE" in
    rcm-gnome) exe="rusted-claude-meter" ;;
    # `launch kde binary` runs the unbundled binary instead of the AppImage.
    # Needed because the AppImage's bundled GL stack cannot start without a
    # GPU — see harness/README.md.
    rcm-kde)
        if [ "${2:-}" = "binary" ]; then
            exe="\$HOME/rusted-claude-meter"
        else
            exe="\$HOME/rusted-claude-meter.AppImage"
        fi
        ;;
    esac
    # Matching the executable is fiddlier than it looks. `pkill -x` silently
    # matches nothing (the process name is truncated to 15 characters, so
    # `rusted-claude-meter` never compares equal). A bare end-anchor is worse
    # than useless: Xvnc runs as `Xvnc :1 … -desktop rusted-claude-meter`, so
    # it ends with the app name and launching the app would kill the X server.
    #
    # So match argv[0] exactly in both forms it takes — the bare name from the
    # .deb on PATH, and the absolute path from `launch kde binary`.
    lima_shell "$INSTANCE" bash -c "
        pkill -f '^rusted-claude-meter\$' 2>/dev/null || true
        pkill -f '^/.*/rusted-claude-meter\$' 2>/dev/null || true
        pkill -f '^/.*/rusted-claude-meter\.AppImage\$' 2>/dev/null || true
        pkill -x AppRun 2>/dev/null || true
        sleep 1
        setsid /usr/local/bin/rcm-run-app $exe >/tmp/rcm-app.log 2>&1 &
        sleep 5
        echo 'launched; log: /tmp/rcm-app.log'
    "
}

# Show the app's stdout/stderr — including the startup line the app prints when
# RCM_API_BASE_URL redirected it.
cmd_logs() {
    resolve "$1"
    lima_shell "$INSTANCE" tail -n "${2:-40}" /tmp/rcm-app.log
}

cmd_status() {
    limactl list | grep -E 'NAME|rcm-' || echo "no harness instances yet"
}

case "${1:-}" in
up | down | delete | shell | vnc | shot | install | launch | logs)
    action="$1"
    shift
    "cmd_$action" "$@"
    ;;
status) cmd_status ;;
*)
    sed -n '3,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
