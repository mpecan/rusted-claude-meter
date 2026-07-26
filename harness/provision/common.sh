#!/usr/bin/env bash
# Shared provisioning: the VNC desktop session and the demo-endpoint
# environment. Sourced by the per-distro scripts after they have installed
# their packages; not run on its own.
#
# Run inside the guest via `limactl shell`, as the Lima user (which has
# passwordless sudo).
#
# ## Two things here are non-obvious, and both cost a debugging round
#
# **It runs as a `systemd --user` unit, with lingering.** The obvious shape — a
# system service running `dbus-run-session -- gnome-session` — does not work.
# `dbus-run-session` creates a private bus that is not the systemd user bus, so
# gnome-session cannot reach `org.freedesktop.systemd1`, finds no logind
# session, and every required component dies behind "Oh no! Something has gone
# wrong." Lingering means the user manager (and its `dbus.socket`) exists with
# nobody logged in.
#
# **The X server and the session are separate units.** `gnome-session` hands off
# to `gnome-session@ubuntu.target` and then *exits 0*. A single unit that runs
# Xvnc alongside it therefore sees a clean exit ~15s in and tears the whole
# desktop down. So `rcm-xvnc` is the long-lived anchor and `rcm-session` is a
# oneshot bound to it — which also means `systemctl --user status` names
# whichever half broke.

set -euo pipefail

# Where the app should look for usage data. `host.lima.internal` is the Mac,
# where `just demo-server` is listening.
: "${RCM_API_BASE_URL:=http://host.lima.internal:8787}"
: "${RCM_VNC_GEOMETRY:=1600x1000}"

# Set by the caller:
#   RCM_SESSION_CMD          how to start the desktop
#   RCM_XDG_CURRENT_DESKTOP  what the session identifies as; the app reads this
#                            to decide whether to show the GNOME AppIndicator
#                            hint (meter_core::desktop_is_gnome)
#   RCM_SESSION_EXTRA_ENV    optional extra `export` lines
#   RCM_KEYRING_SETUP        optional shell run after the session starts, to
#                            put a usable Secret Service on the bus
: "${RCM_SESSION_CMD:?common.sh needs RCM_SESSION_CMD}"
: "${RCM_XDG_CURRENT_DESKTOP:?common.sh needs RCM_XDG_CURRENT_DESKTOP}"
: "${RCM_SESSION_EXTRA_ENV:=}"
: "${RCM_KEYRING_SETUP:=}"

# `systemctl --user` needs both of these, and an ssh session only inherits them
# from pam_systemd — which is exactly what is unreliable on a freshly booted VM
# (logind's D-Bus can still be hanging). Set them explicitly, once, so every
# function here works regardless of how the shell was started.
XDG_RUNTIME_DIR="/run/user/$(id -u)"
export XDG_RUNTIME_DIR
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"

write_session_env() {
    # A .desktop launcher inherits nothing from a login shell, so exporting
    # RCM_API_BASE_URL in .bashrc would not reach the app when it is started
    # from the desktop's own launcher. environment.d is read by `systemd --user`
    # and applies to everything the session launches.
    mkdir -p "$HOME/.config/environment.d"
    cat >"$HOME/.config/environment.d/10-rcm-demo.conf" <<EOF
RCM_API_BASE_URL=$RCM_API_BASE_URL
EOF
    echo "session env -> RCM_API_BASE_URL=$RCM_API_BASE_URL"
}

write_desktop_launcher() {
    # Every entry point shares one environment block, so the session, the app
    # and `vm.sh shot` all agree on display, X cookie and session bus.
    sudo tee /usr/local/bin/rcm-env >/dev/null <<EOF
# Sourced, not executed. Common environment for the harness desktop.
export DISPLAY=:1
# Pin the X cookie to a known path. Xvnc otherwise picks its own, and then
# nothing outside the session can open the display — GTK fails with a bare
# "Failed to initialize GTK", which reads like an app bug rather than a
# missing cookie.
export XAUTHORITY="\$HOME/.Xauthority"
# No GPU behind the vz display, so GNOME Shell and Plasma both need llvmpipe.
export LIBGL_ALWAYS_SOFTWARE=1
# Without XDG_SESSION_TYPE the GNOME session launcher picks Wayland, which has
# no display to attach to here and takes the whole session down with it.
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP="$RCM_XDG_CURRENT_DESKTOP"
export XDG_RUNTIME_DIR="/run/user/\$(id -u)"
export DBUS_SESSION_BUS_ADDRESS="unix:path=\$XDG_RUNTIME_DIR/bus"
export RCM_API_BASE_URL="$RCM_API_BASE_URL"
$RCM_SESSION_EXTRA_ENV
EOF

    # The X server: the long-lived process systemd supervises.
    # `-SecurityTypes None` is safe here and only here — the port is reachable
    # only through Lima's forward to the Mac's loopback, and the VM holds
    # nothing but demo data.
    sudo tee /usr/local/bin/rcm-xvnc >/dev/null <<EOF
#!/usr/bin/env bash
set -euo pipefail
. /usr/local/bin/rcm-env

rm -f "\$XAUTHORITY"
touch "\$XAUTHORITY"
xauth -f "\$XAUTHORITY" add :1 . "\$(mcookie)"

exec Xvnc :1 \\
    -auth "\$XAUTHORITY" \\
    -geometry $RCM_VNC_GEOMETRY \\
    -depth 24 \\
    -SecurityTypes None \\
    -localhost=0 \\
    -AlwaysShared \\
    -desktop rcm-harness
EOF

    # The desktop session. Backgrounded on purpose: gnome-session returns as
    # soon as it has delegated to the systemd user manager, while
    # startplasma-x11 stays in the foreground — backgrounding makes both behave
    # the same to systemd, and the cgroup still owns the children either way.
    sudo tee /usr/local/bin/rcm-session >/dev/null <<EOF
#!/usr/bin/env bash
set -euo pipefail
. /usr/local/bin/rcm-env

for _ in \$(seq 1 100); do
    xdpyinfo >/dev/null 2>&1 && break
    sleep 0.2
done
xdpyinfo >/dev/null 2>&1 || { echo "no X server on :1" >&2; exit 1; }

# Grant this user's processes access to the display regardless of the MIT
# cookie. Xvnc mints a fresh cookie every restart, so anything started outside
# the session (\`vm.sh launch\`, \`vm.sh shot\`) intermittently finds a stale one
# and fails with an unhelpful "Failed to initialize GTK". Same-user only, on a
# single-user throwaway VM.
xhost +SI:localuser:"\$(id -un)" >/dev/null

# No blanking, no DPMS. An idle-locked session captures as a lock screen or a
# black frame, which looks exactly like "the app failed to render" in a
# screenshot taken minutes after the desktop came up.
xset s off -dpms >/dev/null 2>&1 || true

# Before the session, never after: the desktop starts its own keyring daemon,
# and whichever daemon grabs org.freedesktop.secrets first wins. Racing it with
# a second one leaves the app talking to a daemon that has since lost the bus.
$RCM_KEYRING_SETUP

setsid $RCM_SESSION_CMD >/tmp/rcm-session.log 2>&1 &
EOF

    # How to start the app so it lands in the running session: same display,
    # same X cookie, same session bus. Without the bus it gets no tray and no
    # notifications, which looks like an app bug rather than a launch one.
    sudo tee /usr/local/bin/rcm-run-app >/dev/null <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
. /usr/local/bin/rcm-env
exec "$@"
EOF

    sudo chmod +x /usr/local/bin/rcm-xvnc /usr/local/bin/rcm-session /usr/local/bin/rcm-run-app

    # Lingering: the user manager (and its dbus.socket) must exist without an
    # interactive login, or `systemctl --user` has nothing to talk to.
    #
    # Tolerated on failure, and verified by the state on disk instead. On a
    # freshly booted VM logind's D-Bus interface can hang long enough for this
    # to time out *after* it has already taken effect — the flag lives in
    # /var/lib/systemd/linger, so that is the honest thing to check.
    sudo loginctl enable-linger "$USER" 2>/dev/null || true
    if [ ! -e "/var/lib/systemd/linger/$USER" ]; then
        echo "could not enable lingering for $USER; the desktop units need it" >&2
        return 1
    fi

    mkdir -p "$HOME/.config/systemd/user"
    cat >"$HOME/.config/systemd/user/rcm-xvnc.service" <<EOF
[Unit]
Description=Xvnc for the rusted-claude-meter harness
After=dbus.socket
Requires=dbus.socket

[Service]
Type=simple
ExecStart=/usr/local/bin/rcm-xvnc
Restart=on-failure
RestartSec=2

[Install]
WantedBy=default.target
EOF

    cat >"$HOME/.config/systemd/user/rcm-session.service" <<EOF
[Unit]
Description=Desktop session for the rusted-claude-meter harness
After=rcm-xvnc.service
BindsTo=rcm-xvnc.service

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/local/bin/rcm-session

[Install]
WantedBy=default.target
EOF

    systemctl --user daemon-reload
    systemctl --user enable rcm-xvnc.service rcm-session.service >/dev/null
    echo "desktop units installed (session: $RCM_SESSION_CMD)"
}

start_desktop() {
    # A stale session from a previous attempt keeps its own Xvnc and its
    # failure dialog on screen, which then shows up in every screenshot.
    systemctl --user stop rcm-session.service rcm-xvnc.service 2>/dev/null || true
    pkill -u "$USER" -f gnome-session 2>/dev/null || true
    pkill -u "$USER" -f startplasma 2>/dev/null || true
    pkill -u "$USER" Xvnc 2>/dev/null || true
    sleep 2

    systemctl --user start rcm-xvnc.service
    systemctl --user start rcm-session.service

    for _ in $(seq 1 120); do
        if ss -ltn 2>/dev/null | grep -q ':5901'; then
            # The port opens as soon as Xvnc binds; give the session itself a
            # moment so the first screenshot is not of an empty root window.
            sleep 10
            if ! systemctl --user is-active --quiet rcm-xvnc.service; then
                echo "Xvnc died after starting; check: systemctl --user status rcm-xvnc" >&2
                return 1
            fi
            echo "VNC is up on :5901"
            return 0
        fi
        sleep 1
    done
    echo "VNC did not come up; check: systemctl --user status rcm-xvnc rcm-session" >&2
    return 1
}
