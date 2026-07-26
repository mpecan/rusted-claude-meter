#!/usr/bin/env bash
# Regenerate the Linux screenshots in docs/screenshots/linux/ from the
# containerised desktops.
#
#   screenshots.sh          both desktops
#   screenshots.sh gnome    just one
#
# Expects the demo server running and both containers already set up:
#
#   just demo-server
#   just container-up gnome && just container-up kde
#
# The scenario is pinned to `ahead-of-pace` so the shots show a state worth
# looking at (overuse badge, a limit-hit projection) and are reproducible.
#
# Tray shots go through `container.sh tray`, which owns the crop and zoom per
# desktop — this script must not restate them. Menus are the exception: GNOME's
# is drawn by the Shell rather than being an X window, so there is no id to
# target and the crops below are fixed. They hold as long as RCM_GEOMETRY stays
# 1280x800. App *windows* are located by id, so they need no such assumption.

set -euo pipefail

HARNESS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="$(cd "$HARNESS/.." && pwd)"
OUT="$REPO/docs/screenshots/linux"
CONTAINER="$HARNESS/bin/container.sh"
SCENARIO="$HARNESS/bin/scenario.sh"

# `container.sh shot|tray` publish here instead of harness/artifacts/.
export RCM_SHOT_DIR="$OUT"

die() {
    echo "error: $*" >&2
    exit 1
}

# Run a snippet in the target's session, then copy one file out of /tmp.
# Sequences that open the tray menu press Escape first: clicking the icon
# toggles, so starting from an already-open menu would close it instead.
shot() {
    local target="$1" name="$2" script="$3"
    "$CONTAINER" shell "$target" "$script" >/dev/null
    podman cp "rcm-${target}-c:/tmp/shot.png" "$OUT/$name.png"
    echo "  $name.png"
}

# The app's own window. Located by id so its size and position don't matter,
# but captured as a crop of the root: `import -window` on a client-side-
# decorated GTK window returns a clipped, mis-scaled image on both desktops.
window_shot() {
    local target="$1" name="$2"
    shot "$target" "$name" '
        W=$(xdotool search --name "^Rusted Claude Meter$" | head -1)
        [ -n "$W" ] || { echo "main window is not open" >&2; exit 1; }
        eval "$(xdotool getwindowgeometry --shell "$W")"
        import -window root -crop "${WIDTH}x${HEIGHT}+${X}+${Y}" +repage /tmp/shot.png'
}

# Open the tray menu, capture it, then click "Open Rusted Claude Meter" from
# the menu that is already up — one session round trip instead of two, and the
# menu is only opened once.
menu_and_window() {
    local target="$1" tray_click="$2" crop="$3" open_click="$4"
    shot "$target" "$target-tray-menu" "
        xdotool key Escape; sleep 1
        xdotool mousemove $tray_click click 1; sleep 3
        import -window root -crop $crop +repage /tmp/shot.png
        xdotool mousemove $open_click click 1; sleep 5"
    window_shot "$target" "$target-window"
}

# Force the poll rather than waiting one out: the app's own "Refresh Now" menu
# item is right there, and a fixed sleep long enough for the default interval
# would be most of this script's runtime.
refresh_now() {
    local target="$1" tray_click="$2" refresh_click="$3"
    "$CONTAINER" shell "$target" "
        xdotool key Escape; sleep 1
        xdotool mousemove $tray_click click 1; sleep 2
        xdotool mousemove $refresh_click click 1; sleep 4" >/dev/null
}

gnome() {
    echo "gnome:"
    refresh_now gnome "1140 16" "875 353"
    "$CONTAINER" tray gnome gnome-tray-icon >/dev/null && echo "  gnome-tray-icon.png"
    menu_and_window gnome "1140 16" 481x384+792+32 "927 281"
}

kde() {
    echo "kde:"
    refresh_now kde "1070 777" "972 714"
    # Before/after for the square-cell constraint. The container comes up on
    # the default Battery style, which is exactly the case the hint is about.
    "$CONTAINER" tray kde kde-tray-icon-wide >/dev/null && echo "  kde-tray-icon-wide.png"
    # Scroll Settings down to the tray-icon section and catch the hint.
    shot kde kde-square-tray-hint '
        xdotool search --name "^Settings$" windowactivate --sync 2>/dev/null || true
        xdotool mousemove 640 400
        for _ in $(seq 1 11); do xdotool click 5; sleep 0.2; done
        sleep 2
        import -window root -crop 470x120+415+500 +repage /tmp/shot.png'
    # Take the hint, then show what it buys.
    "$CONTAINER" shell kde 'xdotool mousemove 636 554 click 1; sleep 4' >/dev/null
    "$CONTAINER" tray kde kde-tray-icon-square >/dev/null && echo "  kde-tray-icon-square.png"
    menu_and_window kde "1070 777" 366x274+913+486 "1014 656"
}

command -v podman >/dev/null || die "podman not found"
mkdir -p "$OUT"
"$SCENARIO" ahead-of-pace >/dev/null || die "demo server not reachable; just demo-server"

case "${1:-both}" in
gnome) gnome ;;
kde) kde ;;
both)
    gnome
    kde
    ;;
*) die "usage: $0 [gnome|kde]" ;;
esac

echo
echo "wrote to $OUT — check them before committing; the menu crops are fixed."
