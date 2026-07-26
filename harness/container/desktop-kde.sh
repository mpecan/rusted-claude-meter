#!/usr/bin/env bash
# The Plasma half of the session (see session.sh).
#
# Unlike gnome-shell, startplasma-x11 stays in the foreground itself, so there
# is nothing to background and nothing to wait on beyond the X server.
set -euo pipefail

exec startplasma-x11
