#!/usr/bin/env bash
#
# Every file carrying this project's version must agree with Cargo.toml's
# `[workspace.package] version`, and every workspace crate must appear in
# release-please's `extra-files`. A file missing from that list is bumped by
# nobody and says so nowhere — 0.1.4 shipped with both lockfiles still on the
# previous version. See docs/packaging.md.
set -euo pipefail

CONFIG=.release-please-config.json
fail=0

expect() {
    local label=$1 actual=$2
    if [ "$actual" != "$EXPECTED" ]; then
        echo "ERROR: $label is $actual, expected $EXPECTED"
        fail=1
    fi
}

# `version` from the `[workspace.package]` section, stopping at the next header
# so a same-named key elsewhere cannot be picked up.
EXPECTED=$(awk '
    $0 == "[workspace.package]" { in_section = 1; next }
    /^\[/ { in_section = 0 }
    in_section && $1 == "version" { gsub(/^"|"$/, "", $3); print $3; exit }
' Cargo.toml)
[ -n "$EXPECTED" ] || { echo "ERROR: no [workspace.package] version in Cargo.toml"; exit 1; }

while read -r label version; do
    expect "$label" "$version"
done < <(node -e '
    const at = (f, path) => {
        const v = path.reduce((o, k) => o?.[k], require(f));
        console.log(`${f}${path.map(k => `[${JSON.stringify(k)}]`).join("")} ${v}`);
    };
    at("./package.json", ["version"]);
    at("./src-tauri/tauri.conf.json", ["version"]);
    at("./package-lock.json", ["version"]);
    at("./package-lock.json", ["packages", "", "version"]);
')

# Workspace crates are the `[[package]]` entries with no `source` — derived
# rather than hardcoded so a crate added to the workspace is covered the day it
# is added, on both counts below.
while read -r crate version; do
    expect "Cargo.lock $crate version" "$version"
    if ! grep -q "@.name.value=='$crate'" "$CONFIG"; then
        echo "ERROR: workspace crate $crate has no Cargo.lock entry in $CONFIG extra-files"
        fail=1
    fi
done < <(awk '
    /^\[\[package\]\]/ { name = ""; version = ""; sourced = 0 }
    $1 == "name"    { gsub(/^"|"$/, "", $3); name = $3 }
    $1 == "version" { gsub(/^"|"$/, "", $3); version = $3 }
    $1 == "source"  { sourced = 1 }
    /^$/            { if (name && !sourced) print name, version; name = "" }
    END             { if (name && !sourced) print name, version }
' Cargo.lock)

if [ "$fail" -eq 0 ]; then
    echo "Lockfile version check passed ($EXPECTED)."
else
    echo
    echo "Resync with 'cargo update --workspace' and 'npm install --package-lock-only',"
    echo "and add anything missing to extra-files in $CONFIG."
fi

exit "$fail"
