#!/usr/bin/env bash
# The GNOME half of the session (see session.sh).
#
# `--x11` because there is no Wayland compositor here, and `--unsafe-mode`
# because it exposes the Shell's D-Bus Eval endpoint — that is how a test
# asserts on what is actually on screen rather than eyeballing a screenshot.
set -euo pipefail

# The extension GNOME needs before it will show any StatusNotifierItem at all —
# the single behaviour this harness exists to exercise. Enabled before the
# shell starts so the tray is there on the first frame; turning it back off is
# how the *absence* of it gets tested (see harness/README.md).
gsettings set org.gnome.shell enabled-extensions \
    "['ubuntu-appindicators@ubuntu.com']" 2>/dev/null || true

exec gnome-shell --x11 --unsafe-mode
