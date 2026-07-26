#!/usr/bin/env bash
# Start the app inside the running session, replacing any previous instance.
#
# Run as the `rcm` user. Takes the executable to run, defaulting to the one the
# .deb puts on PATH.
set -euo pipefail
# shellcheck source=/dev/null
. /usr/local/bin/rcm-env

exe="${1:-rusted-claude-meter}"

# The Secret Service, whichever one this desktop provides. On GNOME that is
# gnome-keyring and it has to be started here; on Plasma it is ksecretd in
# front of KWallet, which the session starts itself and which `container.sh
# wallet` sets up. Keying off the binary rather than off the desktop keeps this
# one script correct for both images.
if command -v gnome-keyring-daemon >/dev/null; then
    # An unlocked login keyring, so the Secret Service answers without a
    # prompt. Naming it as the default *before* starting the daemon is what
    # makes the unattended path work: the daemon then creates and unlocks
    # `login` itself off the empty password on stdin.
    mkdir -p "$HOME/.local/share/keyrings"
    [ -f "$HOME/.local/share/keyrings/default" ] ||
        printf 'login\n' >"$HOME/.local/share/keyrings/default"

    # Exactly one daemon, ever. Whichever one claims org.freedesktop.secrets
    # first wins, so a second one started on a relaunch leaves the app talking
    # to a daemon that has since lost the bus.
    if ! pgrep -f '^gnome-keyring-daemon' >/dev/null; then
        eval "$(printf '\n' | gnome-keyring-daemon --unlock --components=secrets --daemonize)"
        export GNOME_KEYRING_CONTROL SSH_AUTH_SOCK
    fi
fi

# `-f` is not optional in the process matching below. `pkill -x` silently
# matches nothing for `rusted-claude-meter` — a process name is truncated to 15
# characters, so it never compares equal.

pkill -f '^rusted-claude-meter$' 2>/dev/null || true
pkill -f '^/.*/rusted-claude-meter$' 2>/dev/null || true
sleep 1

setsid "$exe" >/tmp/rcm-app.log 2>&1 &
sleep 6
echo "launched; log: /tmp/rcm-app.log"
