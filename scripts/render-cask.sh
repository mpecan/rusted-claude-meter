#!/usr/bin/env bash
# Renders Casks/rusted-claude-meter.rb.tmpl into a ready-to-use Homebrew cask
# formula for the DMG that `tauri-action` just built, and uploads it as a
# release asset (see .github/workflows/release.yml, issue #14).
#
# Usage: render-cask.sh <tag>   e.g. render-cask.sh v0.1.0
#
# The rendered formula points at this GitHub Release's DMG asset, so it only
# becomes installable once the release is published. See docs/packaging.md
# for how to add it to a (private, while this repo is private) Homebrew tap.
set -euo pipefail

tag="${1:?usage: render-cask.sh <tag>}"
version="${tag#v}"
repo="${GH_REPO:-mpecan/rusted-claude-meter}"

dmg_path=$(find target/release/bundle/dmg -maxdepth 1 -name '*.dmg' -print -quit)
if [ -z "$dmg_path" ]; then
    echo "render-cask.sh: no .dmg found under target/release/bundle/dmg" >&2
    exit 1
fi

sha256=$(shasum -a 256 "$dmg_path" | cut -d' ' -f1)
dmg_name=$(basename "$dmg_path")
# GitHub release asset URLs percent-encode spaces as %20; that's the only
# character Tauri's default DMG filename (e.g. "App Name_1.0.0_aarch64.dmg")
# ever contains.
encoded_name="${dmg_name// /%20}"
url="https://github.com/${repo}/releases/download/${tag}/${encoded_name}"

sed \
    -e "s|__VERSION__|${version}|g" \
    -e "s|__SHA256__|${sha256}|g" \
    -e "s|__URL__|${url}|g" \
    Casks/rusted-claude-meter.rb.tmpl > rusted-claude-meter.rb

echo "Rendered rusted-claude-meter.rb for ${dmg_name} (sha256 ${sha256})"
