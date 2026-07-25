#!/usr/bin/env bash
# Change what the demo API serves, while the app is running.
#
#   scenario.sh                 show the current state and what is available
#   scenario.sh critical        switch to a scenario
#   scenario.sh failure 401     inject an error (401|403|500|hang|none)
#
# The app picks the change up on its next poll — no restart of either side.

set -euo pipefail

ADMIN="${DEMO_SERVER_URL:-http://localhost:8787}/__admin"

pretty() {
    if command -v jq >/dev/null; then jq .; else cat; fi
}

curl_admin() {
    curl -fsS "$@" 2>/dev/null || {
        echo "error: no demo server at $ADMIN — start it with 'just demo-server'" >&2
        exit 1
    }
}

case "${1:-}" in
"")
    curl_admin "$ADMIN/state" | pretty
    ;;
failure)
    # The named HTTP codes are friendlier at the command line than the
    # server's internal mode names.
    case "${2:-}" in
    401 | unauthorized) mode=unauthorized ;;
    403 | forbidden) mode=forbidden ;;
    500 | server-error) mode=server-error ;;
    hang | timeout) mode=hang ;;
    none | off | clear) mode=none ;;
    *)
        echo "usage: $0 failure 401|403|500|hang|none" >&2
        exit 1
        ;;
    esac
    curl_admin -X PUT -H 'Content-Type: application/json' \
        -d "{\"mode\":\"$mode\"}" "$ADMIN/failure" | pretty
    ;;
*)
    curl_admin -X PUT -H 'Content-Type: application/json' \
        -d "{\"name\":\"$1\"}" "$ADMIN/scenario" | pretty
    ;;
esac
