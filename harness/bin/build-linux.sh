#!/usr/bin/env bash
# Build the Linux .deb and AppImage inside the GNOME VM and drop them in
# harness/artifacts/ on the Mac.
#
# The repo is mounted read-only at /repo, so the build runs from a copy in the
# VM's own filesystem: `npm install` and `cargo build` both write into the tree,
# and a Tauri target dir on a virtiofs mount is slow enough to be worth avoiding
# anyway.

set -euo pipefail

INSTANCE=rcm-gnome
HARNESS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [ "$(limactl list --format '{{.Status}}' "$INSTANCE" 2>/dev/null)" != "Running" ]; then
    echo "error: $INSTANCE is not running — start it with 'just vm-up gnome'" >&2
    exit 1
fi

mkdir -p "$HARNESS/artifacts"

# Single-quoted on purpose: every expansion below belongs to the guest shell.
# shellcheck disable=SC2016
limactl shell --workdir / "$INSTANCE" bash -lc '
set -euo pipefail
. "$HOME/.cargo/env"

BUILD="$HOME/build"
mkdir -p "$BUILD"

# rsync, not cp: repeat builds then only copy what changed, and the target dir
# and node_modules living in $BUILD survive between runs.
rsync -a --delete \
    --exclude target --exclude node_modules --exclude dist --exclude .git \
    /repo/ "$BUILD/"
cd "$BUILD"

echo "==> frontend dependencies"
# Only the install. The `beforeBuildCommand` in tauri.conf.json is already
# `npm run build`, so the CLI runs tsc + vite itself before bundling; calling
# it here too would do the whole typecheck-and-bundle twice.
npm install --no-audit --no-fund

echo "==> bundling .deb and AppImage"
npm run tauri build -- --bundles deb,appimage

# The workspace target dir is at the repo root, not under src-tauri/.
cp -v target/release/bundle/deb/*.deb /artifacts/
cp -v target/release/bundle/appimage/*.AppImage /artifacts/
# The unbundled binary too: the AppImage cannot start in a GPU-less VM (see
# harness/README.md), so this is what actually gets KDE covered.
cp -v target/release/rusted-claude-meter /artifacts/
'

echo
echo "artifacts:"
ls -lh "$HARNESS/artifacts"/*.deb "$HARNESS/artifacts"/*.AppImage 2>/dev/null
