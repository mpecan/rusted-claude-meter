#!/usr/bin/env bash
# Xvfb plus a desktop session, as one foreground process systemd owns.
#
# Shared by both images: everything up to the display being ready is identical,
# so only the last step — which desktop to exec, and whatever it needs set up
# first — lives per-image, in `/usr/local/bin/rcm-session-desktop`.
set -euo pipefail
# shellcheck source=/dev/null
. /usr/local/bin/rcm-env

: "${RCM_GEOMETRY:=1280x800x24}"

Xvfb "$DISPLAY" -screen 0 "$RCM_GEOMETRY" -nolisten tcp >/tmp/xvfb.log 2>&1 &
XVFB=$!
trap 'kill $XVFB 2>/dev/null || true' EXIT

for _ in $(seq 1 100); do
    xdpyinfo >/dev/null 2>&1 && break
    sleep 0.1
done
xdpyinfo >/dev/null 2>&1 || { echo "Xvfb never came up" >&2; cat /tmp/xvfb.log >&2; exit 1; }

# Same-user access regardless of the MIT cookie, so `podman exec` can drive it.
xhost "+SI:localuser:$(id -un)" >/dev/null 2>&1 || true

# No blanking, no DPMS — an idle-blanked session captures as a black frame,
# which looks exactly like "the app failed to render".
xset s off -dpms >/dev/null 2>&1 || true

exec /usr/local/bin/rcm-session-desktop
