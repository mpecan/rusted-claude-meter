#!/usr/bin/env bash
# Sourced, not executed. The one environment block every entry point shares —
# the session, the app and every `container.sh` subcommand — so they all agree
# on display, session bus and demo endpoint.
#
# Shared by both images. The desktop is the only thing that differs, and the
# Containerfile supplies it via `RCM_DESKTOP`.

export DISPLAY=:99
export LIBGL_ALWAYS_SOFTWARE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP="${RCM_DESKTOP:-GNOME}"
XDG_RUNTIME_DIR="/run/user/$(id -u)"
export XDG_RUNTIME_DIR
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"

# The Mac, from inside the podman machine. The VMs use host.lima.internal for
# the same thing; this is podman's equivalent and is what the demo-endpoint
# banner in Settings will name.
export RCM_API_BASE_URL="${RCM_API_BASE_URL:-http://host.containers.internal:8787}"

# Both are needed, and for different reasons. Without DMABUF the renderer still
# tries EGL and logs `failed to open /dev/dri/card0`; without the compositing
# override WebKit brings up a window whose contents never paint — the app draws
# as a blank white rectangle with a correct title bar, which looks like a
# frontend bug rather than a GL one. There is no GPU behind a container.
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
