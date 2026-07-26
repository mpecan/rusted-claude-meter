#!/usr/bin/env bash
# Drive a containerised Linux desktop — the fast path through the harness.
#
#   container.sh build   gnome|kde    build (or rebuild) the image
#   container.sh up      gnome|kde    start the container and wait for the desktop
#   container.sh down    gnome|kde    stop and remove it
#   container.sh install gnome|kde    install the artifact from harness/artifacts/
#   container.sh launch  gnome|kde    start the app inside the session
#   container.sh wallet  kde          create the KWallet wallet (drives the dialogs)
#   container.sh setup   gnome|kde    click through the wizard with a demo key
#   container.sh shot    gnome|kde [NAME]  screenshot the desktop to artifacts/
#   container.sh tray    gnome|kde [NAME]  screenshot just the tray, magnified
#   container.sh status  gnome|kde    what the desktop thinks is in its tray
#   container.sh eval    gnome JS     evaluate JS in GNOME Shell (GNOME only)
#   container.sh appindicator gnome on|off
#   container.sh logs    gnome|kde    the app's stdout/stderr
#   container.sh journal gnome|kde    the session's journal
#   container.sh shell   gnome|kde [CMD...]  run a command in the session
#
# See harness/README.md.

set -euo pipefail

HARNESS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARTIFACTS="$HARNESS/artifacts"
# Where `shot`/`tray` write. Overridable so callers that publish elsewhere
# (harness/bin/screenshots.sh) reuse the crop geometry rather than copying it.
SHOT_DIR="${RCM_SHOT_DIR:-$ARTIFACTS}"
UID_RCM=1001
EXT=ubuntu-appindicators@ubuntu.com

die() {
    echo "error: $*" >&2
    exit 1
}

# Map the short name onto its image, container, and the handful of things that
# genuinely differ between the two desktops. Everything else is shared.
resolve() {
    case "${1:-}" in
    gnome)
        TARGET=gnome
        IMAGE=rcm-gnome-container
        NAME=rcm-gnome-c
        CONTAINERFILE=Containerfile
        # The panel is at the top on GNOME and the bottom on Plasma, so a tray
        # crop that works on one shows empty desktop on the other.
        TRAY_CROP=320x30+960+0
        TRAY_ZOOM=300%
        # Wizard click points, in screen coordinates. They differ because the
        # window managers place and decorate the window differently.
        W_START="554 495"    # Get started
        W_FIELD="398 605"    # session key field
        W_SUBMIT="398 644"   # Continue, validates the key
        W_VERIFIED="563 473" # Continue, past "connected and verified"
        W_APPEARANCE="563 595"
        W_FINISH="576 512"
        ;;
    kde)
        TARGET=kde
        IMAGE=rcm-kde-container
        NAME=rcm-kde-c
        CONTAINERFILE=Containerfile.kde
        TRAY_CROP=130x28+1055+762
        TRAY_ZOOM=700%
        W_START="805 454"
        W_FIELD="639 564"
        W_SUBMIT="639 603"
        W_VERIFIED="812 432"
        W_APPEARANCE="812 554"
        W_FINISH="821 434"
        ;;
    *)
        die "usage: $0 <command> <gnome|kde> [args]"
        ;;
    esac
}

running() {
    [ "$(podman inspect -f '{{.State.Running}}' "$NAME" 2>/dev/null)" = "true" ]
}

need_podman() {
    command -v podman >/dev/null || die "podman not found; brew install podman"
    podman info >/dev/null 2>&1 ||
        die "podman machine is not running; podman machine start"
}

need_running() {
    running || die "$NAME is not running; $0 up $TARGET"
}

# Every exec into the session goes through here, so the display, the session
# bus and the demo endpoint are identical to what the session itself uses.
in_session() {
    podman exec -u rcm \
        -e "XDG_RUNTIME_DIR=/run/user/$UID_RCM" \
        -e "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$UID_RCM/bus" \
        "$NAME" bash -c ". /usr/local/bin/rcm-env; $1"
}

cmd_build() {
    need_podman
    podman build -t "$IMAGE" -f "$HARNESS/container/$CONTAINERFILE" "$HARNESS/container"
}

cmd_up() {
    need_podman
    podman image exists "$IMAGE" || cmd_build

    podman rm -f "$NAME" >/dev/null 2>&1 || true
    # systemd as PID 1 needs a writable cgroup tree and its own tmpfs on /run,
    # which --systemd=always arranges. --privileged is not decoration either:
    # logind refuses to start without it, and neither gnome-shell nor
    # startplasma-x11 will run without logind. These containers are throwaway
    # and hold nothing but demo data.
    podman run -d --name "$NAME" \
        --privileged --systemd=always --shm-size=1g \
        -v /sys/fs/cgroup:/sys/fs/cgroup:rw \
        "$IMAGE" >/dev/null

    # Wait on the StatusNotifierWatcher rather than on any process. It is the
    # thing this harness is about, both desktops publish it once their panel is
    # up (GNOME through the AppIndicator extension, Plasma natively), and it is
    # the same check on both.
    printf 'waiting for the %s desktop' "$TARGET"
    for _ in $(seq 1 90); do
        if in_session 'gdbus call --session --dest org.kde.StatusNotifierWatcher \
            --object-path /StatusNotifierWatcher \
            --method org.freedesktop.DBus.Properties.Get \
            org.kde.StatusNotifierWatcher RegisteredStatusNotifierItems' \
            >/dev/null 2>&1; then
            echo " — up."
            return 0
        fi
        printf .
        sleep 1
    done
    echo
    die "the desktop never came up; $0 journal $TARGET"
}

cmd_down() {
    need_podman
    podman rm -f "$NAME" >/dev/null 2>&1 || true
    echo "removed $NAME"
}

# GNOME installs the shipped .deb. Plasma runs the unbundled binary, for the
# same reason `vm.sh launch kde binary` exists: the AppImage's bundled GL stack
# cannot start without a GPU, and there is no GPU behind a container either.
cmd_install() {
    need_running
    case "$TARGET" in
    gnome)
        local deb="${1:-}"
        if [ -z "$deb" ]; then
            # Newest .deb in artifacts. The pipe through head hides ls's exit
            # status, so the empty case is caught below rather than here.
            deb="$(ls -t "$ARTIFACTS"/*.deb 2>/dev/null | head -1)"
            [ -n "$deb" ] || die "no .deb in $ARTIFACTS; just linux-build"
        fi
        [ -f "$deb" ] || die "no such file: $deb"
        echo "installing $(basename "$deb")"
        podman cp "$deb" "$NAME:/tmp/rcm.deb"
        podman exec "$NAME" bash -c 'apt-get install -y /tmp/rcm.deb 2>&1 | tail -3'
        ;;
    kde)
        local bin="${1:-$ARTIFACTS/rusted-claude-meter}"
        [ -f "$bin" ] || die "no unbundled binary at $bin; just linux-build"
        echo "installing $(basename "$bin")"
        podman cp "$bin" "$NAME:/home/rcm/rusted-claude-meter"
        podman exec "$NAME" bash -c \
            'chmod +x /home/rcm/rusted-claude-meter
             chown rcm:rcm /home/rcm/rusted-claude-meter
             ldd /home/rcm/rusted-claude-meter | grep "not found" && exit 1
             echo "all libraries resolved"'
        ;;
    esac
}

cmd_launch() {
    need_running
    case "$TARGET" in
    gnome) podman exec -u rcm "$NAME" /usr/local/bin/rcm-run-app "$@" ;;
    kde) podman exec -u rcm "$NAME" /usr/local/bin/rcm-run-app \
        "${1:-/home/rcm/rusted-claude-meter}" ;;
    esac
}

# Create the KWallet wallet, by driving the dialogs rather than by dodging
# them. Plasma's Secret Service provider is KWallet, and a wallet cannot be
# created unattended without weakening it — so the harness answers the wizard
# the way a user would: blowfish, blank password, accept the strength warning.
# Substituting gnome-keyring here would mean not testing KDE's real credential
# path, which is most of the point of having a KDE target.
cmd_wallet() {
    need_running
    [ "$TARGET" = kde ] || die "wallet is KDE-only; GNOME uses gnome-keyring"

    if podman exec "$NAME" test -f "/home/rcm/.local/share/kwalletd/Default keyring.kwl"; then
        echo "wallet already exists"
        return 0
    fi

    # The store call blocks until the dialogs are answered, so it has to run in
    # the background while xdotool answers them.
    in_session 'nohup secret-tool store --label=rcm-harness-probe \
        service rcm-harness key probe </dev/null >/tmp/wallet.log 2>&1 &
        sleep 4
        xdotool mousemove 501 299 click 1; sleep 1   # Classic, blowfish
        xdotool mousemove 647 490 click 1; sleep 3   # Next
        xdotool mousemove 737 490 click 1; sleep 3   # Finish
        xdotool mousemove 686 436 click 1; sleep 3   # OK, blank password
        xdotool mousemove 727 366 click 1; sleep 4   # Yes, use it anyway
    ' >/dev/null

    podman exec "$NAME" test -f "/home/rcm/.local/share/kwalletd/Default keyring.kwl" ||
        die "wallet was not created; $0 shot $TARGET to see which dialog is up"
    echo "wallet created (blowfish, blank password)"
}

# Click through the first-run wizard with a fabricated key. The demo server
# accepts any key with the right shape and 401s a missing one, so the
# validation path is real even though the key is not.
#
# Coordinates, not selectors: the app is a webview inside a real window on a
# real X server, and the point of this harness is that nothing about it is
# mocked. They are stable because the geometry is (RCM_GEOMETRY, a 1280x800
# screen and a window the app sizes itself), and they live in resolve().
cmd_setup() {
    need_running
    local key="${1:-sk-ant-sid01-harness-demo-key-000000000000000000000000000000000000AA}"

    # On KDE the key save is what triggers wallet creation, and the app reports
    # the credential store as unavailable rather than waiting out the dialogs.
    # So the wallet has to exist first.
    if [ "$TARGET" = kde ]; then cmd_wallet; fi

    # Wait for the window, then make sure it is the thing being clicked. GNOME
    # opens in the Overview when a session has no windows and stays there after
    # the app's window appears, so clicking straight away lands on the
    # Overview's workspace thumbnails and the wizard never advances — the
    # failure looks exactly like "the coordinates are wrong".
    in_session 'xdotool search --sync --onlyvisible --name "^Settings$" >/dev/null'
    if [ "$TARGET" = gnome ]; then
        cmd_eval 'Main.overview.hide(); true' >/dev/null
    fi
    # windowactivate warns about _NET_WM_DESKTOP on a single-workspace session;
    # it still activates, so the noise is dropped rather than the command.
    in_session 'sleep 1
        xdotool search --name "^Settings$" windowactivate --sync 2>/dev/null || true
        sleep 1'

    in_session "
        set -e
        xdotool mousemove $W_START click 1; sleep 3
        xdotool mousemove $W_FIELD click 1; sleep 1
        xdotool type --delay 12 '$key'; sleep 1
        xdotool mousemove $W_SUBMIT click 1; sleep 8
        xdotool mousemove $W_VERIFIED click 1; sleep 2
        xdotool mousemove $W_APPEARANCE click 1; sleep 2
        xdotool mousemove $W_FINISH click 1; sleep 2
    "
    echo "wizard done; $0 shot $TARGET to check"
}

cmd_shot() {
    need_running
    local name="${1:-$TARGET}"
    in_session "import -window root /tmp/shot.png"
    podman cp "$NAME:/tmp/shot.png" "$SHOT_DIR/$name.png"
    echo "$SHOT_DIR/$name.png"
}

# The tray, magnified. The icon is 66x22 at panel scale; at 1:1 in a
# full-screen capture there is not enough of it to judge colour or badge, which
# is the whole point of looking. On Plasma it is smaller still — KDE renders
# tray icons into a square cell, so the gauge draws at a third of panel height.
cmd_tray() {
    need_running
    local name="${1:-$TARGET-tray}"
    in_session "import -window root -crop $TRAY_CROP +repage -resize $TRAY_ZOOM /tmp/tray.png"
    podman cp "$NAME:/tmp/tray.png" "$SHOT_DIR/$name.png"
    echo "$SHOT_DIR/$name.png"
}

# The reason the GNOME session runs with --unsafe-mode: assert on what the
# Shell actually holds, instead of eyeballing a screenshot. Plasma has no
# equivalent worth wrapping — `status` covers what this harness needs there.
#
# The JS travels in an environment variable rather than inside the command
# string, because quoting it through two shells mangles anything containing a
# quote.
#
# Backslash escapes in the JS do not survive the trip regardless — a `"\n"`
# reaches GNOME as a real line break and comes back as a SyntaxError about an
# unterminated string literal. Write `String.fromCharCode(10)`, as cmd_status
# does. Not worth compensating for: the escaping is asymmetric between gdbus's
# GVariant parser and its printer, so any fix here is a guess that breaks on
# the next character somebody uses.
cmd_eval() {
    need_running
    [ "$TARGET" = gnome ] || die "eval is GNOME-only; try: $0 status $TARGET"
    [ $# -gt 0 ] || die "usage: $0 eval gnome <javascript>"
    podman exec -u rcm \
        -e "XDG_RUNTIME_DIR=/run/user/$UID_RCM" \
        -e "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$UID_RCM/bus" \
        -e "RCM_JS=$*" \
        "$NAME" bash -c 'gdbus call --session --dest org.gnome.Shell \
            --object-path /org/gnome/Shell --method org.gnome.Shell.Eval \
            "$RCM_JS"' |
        python3 -c "$UNWRAP"
}

# Shared by cmd_eval. Kept out of the function body so the quoting stays
# readable. gdbus answers `(true, '<result>')` on success and `(false, '<error
# message>')` on a thrown exception — both are exit status 0 from gdbus itself,
# so the false case has to become a non-zero exit here or a broken Eval reads
# as a successful one.
#
# The result is escaped twice and has to be peeled in that order. GNOME's Eval
# hands back JSON, and gdbus then escapes that JSON for its own GVariant
# printer, so a newline in the answer arrives as backslash-backslash-n.
# Undoing them in the wrong order leaves a stray backslash on every line.
UNWRAP='
import json, re, sys
raw = sys.stdin.read().strip()
m = re.match(r"^\((true|false), .(.*).\)$", raw, re.S)
if not m:
    print(raw)
    sys.exit(0)
ok, value = m.group(1) == "true", m.group(2)
value = re.sub(r"\\(.)", r"\1", value)
try:
    value = json.loads(value)
except ValueError:
    pass
# json.dumps for anything but a string, so a boolean prints as `true` the way
# it was written in the JS rather than as the Python `True`. (No apostrophes in
# here: UNWRAP is a single-quoted string, and one would end it.)
print(value if isinstance(value, str) else json.dumps(value))
sys.exit(0 if ok else 1)
'

# What the desktop believes about the tray. The watcher query is the same on
# both — it is the StatusNotifierItem contract itself — so it is the honest
# thing to compare across desktops. GNOME then gets the two facts that are
# GNOME's alone.
cmd_status() {
    need_running
    echo "StatusNotifierItems registered:"
    local items
    # A missing watcher is a result, not an error: on GNOME the AppIndicator
    # extension is what provides it, so `appindicator off` leaves the name with
    # no owner at all and nothing can register a tray icon. Reporting that is
    # the sharpest statement of the GNOME/Plasma difference this harness makes.
    if ! items="$(in_session 'gdbus call --session --dest org.kde.StatusNotifierWatcher \
        --object-path /StatusNotifierWatcher \
        --method org.freedesktop.DBus.Properties.Get \
        org.kde.StatusNotifierWatcher RegisteredStatusNotifierItems' 2>/dev/null)"; then
        echo "  (no StatusNotifierWatcher on the bus — nothing can register one)"
    else
        # gdbus prints `(<['a', 'b']>,)`, and `(<@as []>,)` when the watcher
        # is up but nothing has registered — the type annotation only appears
        # in the empty case, which is why it gets its own arm.
        printf '%s\n' "$items" |
            sed -e 's/^(<@as \[\]>,)$//' -e 's/^(<\[//' -e 's/\]>,)$//' \
                -e "s/', '/\\n/g" -e "s/'//g" |
            sed -e 's/^/  /' -e 's/^  $/  (none)/'
    fi

    [ "$TARGET" = gnome ] || return 0

    echo "GNOME panel status area:"
    cmd_eval 'Object.keys(Main.panel.statusArea).join(String.fromCharCode(10))' |
        sed 's/^/  /'
    echo "extension $EXT:"
    # GNOME's ExtensionState is 1-based and starts at ENABLED, not DISABLED —
    # an off-by-one here reports a working tray as switched off.
    cmd_eval "['ENABLED','DISABLED','ERROR','OUT_OF_DATE','DOWNLOADING','INITIALIZED','DISABLING','ENABLING'][Main.extensionManager.lookup('$EXT')?.state - 1] ?? 'not found'" |
        sed 's/^/  /'
}

# GNOME shows no StatusNotifierItem without this extension; Plasma needs none,
# which is why the KDE target is the control case rather than a duplicate.
cmd_appindicator() {
    need_running
    [ "$TARGET" = gnome ] ||
        die "appindicator is GNOME-only — Plasma needs no extension, which is the point"
    case "${1:-}" in
    on) in_session "gnome-extensions enable $EXT" ;;
    off) in_session "gnome-extensions disable $EXT" ;;
    *) die "usage: $0 appindicator gnome on|off" ;;
    esac
    sleep 2
    cmd_status
}

cmd_logs() {
    need_running
    podman exec "$NAME" cat /tmp/rcm-app.log
}

cmd_journal() {
    need_running
    podman exec "$NAME" journalctl --no-pager -n "${1:-60}"
}

cmd_shell() {
    need_running
    if [ $# -eq 0 ]; then
        podman exec -it -u rcm "$NAME" bash
    else
        in_session "$*"
    fi
}

COMMAND="${1:-}"
[ $# -ge 2 ] || {
    sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    exit 1
}
resolve "$2"
shift 2

case "$COMMAND" in
build) cmd_build "$@" ;;
up) cmd_up "$@" ;;
down) cmd_down "$@" ;;
install) cmd_install "$@" ;;
launch) cmd_launch "$@" ;;
wallet) cmd_wallet "$@" ;;
setup) cmd_setup "$@" ;;
shot) cmd_shot "$@" ;;
tray) cmd_tray "$@" ;;
eval) cmd_eval "$@" ;;
status) cmd_status "$@" ;;
appindicator) cmd_appindicator "$@" ;;
logs) cmd_logs "$@" ;;
journal) cmd_journal "$@" ;;
shell) cmd_shell "$@" ;;
*)
    sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
