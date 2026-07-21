#!/usr/bin/env bash
# Renders Casks/<token>.rb.tmpl into a ready-to-use Homebrew cask for the
# matching DMG the release just built, writing <token>.rb. Called once per
# build variant by the release workflow (.github/workflows/release.yml).
#
# Usage: render-cask.sh <tag> [token] [dmg-name-glob]
#   render-cask.sh v0.1.2
#   render-cask.sh v0.1.2 rusted-claude-meter      "Rusted Claude Meter_*.dmg"
#   render-cask.sh v0.1.2 rusted-claude-meter-lite "Rusted Claude Meter Lite_*.dmg"
#
# The full and lite DMGs share a directory; the glob distinguishes them by the
# character after "Meter" ("_" for full, " " for lite). The rendered cask
# points at this GitHub Release's DMG asset, so it only becomes installable
# once the release is published.
set -euo pipefail

tag="${1:?usage: render-cask.sh <tag> [token] [dmg-glob]}"
token="${2:-rusted-claude-meter}"
dmg_glob="${3:-*.dmg}"
version="${tag#v}"
repo="${GH_REPO:-mpecan/rusted-claude-meter}"

dmg_path=$(find target/release/bundle/dmg -maxdepth 1 -name "$dmg_glob" -print -quit)
if [ -z "$dmg_path" ]; then
    echo "render-cask.sh: no .dmg matching '$dmg_glob' under target/release/bundle/dmg" >&2
    exit 1
fi

sha256=$(shasum -a 256 "$dmg_path" | cut -d' ' -f1)
dmg_name=$(basename "$dmg_path")
# GitHub release asset URLs percent-encode spaces as %20; that's the only
# character Tauri's DMG filename ("Product Name_1.0.0_aarch64.dmg") contains.
encoded_name="${dmg_name// /%20}"
url="https://github.com/${repo}/releases/download/${tag}/${encoded_name}"

sed \
    -e "s|__VERSION__|${version}|g" \
    -e "s|__SHA256__|${sha256}|g" \
    -e "s|__URL__|${url}|g" \
    "Casks/${token}.rb.tmpl" > "${token}.rb"

echo "Rendered ${token}.rb for ${dmg_name} (sha256 ${sha256})"
