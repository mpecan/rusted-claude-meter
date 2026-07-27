#!/usr/bin/env bash
#
# Every file that carries this project's version must agree.
#
# release-please bumps them all from the `extra-files` list in
# .release-please-config.json, and it does so by rewriting known fields rather
# than by running a package manager (see .github/workflows/release-please.yml
# for why it cannot run one). That makes an omission from that list completely
# silent: 0.1.4 shipped with Cargo.lock and package-lock.json still saying
# 0.1.3 and 0.1.0, and nothing noticed until someone read a diff. This is what
# notices.
#
# Checked against Cargo.toml's `[workspace.package] version`, which is the
# root the other files derive from.
set -euo pipefail

fail=0

# Read one `key = "value"` out of a TOML section. `section` is matched as a
# literal line; the search stops at the next section header so a same-named key
# in a later section cannot be picked up by mistake.
toml_value() {
    local file=$1 section=$2 key=$3
    awk -v section="$section" -v key="$key" '
        $0 == section { in_section = 1; next }
        /^\[/ { in_section = 0 }
        in_section && $1 == key {
            # value is the last field, quoted
            gsub(/^"|"$/, "", $3); print $3; exit
        }
    ' "$file"
}

# Report one file/field's version against the expected one.
expect() {
    local label=$1 actual=$2
    if [ "$actual" != "$EXPECTED" ]; then
        echo "ERROR: $label is $actual, expected $EXPECTED"
        fail=1
    fi
}

EXPECTED=$(toml_value Cargo.toml '[workspace.package]' version)
if [ -z "$EXPECTED" ]; then
    echo "ERROR: no [workspace.package] version in Cargo.toml"
    exit 1
fi

# The manifests release-please already updated before this check existed.
expect "package.json version" "$(node -p 'require("./package.json").version')"
expect "src-tauri/tauri.conf.json version" \
    "$(node -p 'require("./src-tauri/tauri.conf.json").version')"

# The lockfiles, which it did not.
expect "package-lock.json version" "$(node -p 'require("./package-lock.json").version')"
expect "package-lock.json packages[\"\"].version" \
    "$(node -p 'require("./package-lock.json").packages[""].version')"

# Every workspace member's entry in Cargo.lock. Derived from `cargo metadata`
# rather than a hardcoded list so a crate added to the workspace is covered the
# day it is added.
members=$(cargo metadata --format-version 1 --no-deps \
    | node -e 'const m=JSON.parse(require("fs").readFileSync(0,"utf8"));
               console.log(m.packages.map(p=>p.name).join("\n"))')

for member in $members; do
    locked=$(awk -v name="$member" '
        /^\[\[package\]\]/ { pkg = ""; ver = "" }
        $1 == "name" { gsub(/^"|"$/, "", $3); pkg = $3 }
        $1 == "version" { gsub(/^"|"$/, "", $3); ver = $3 }
        pkg == name && ver != "" { print ver; exit }
    ' Cargo.lock)
    if [ -z "$locked" ]; then
        echo "ERROR: workspace member $member has no [[package]] entry in Cargo.lock"
        fail=1
    else
        expect "Cargo.lock $member version" "$locked"
    fi
done

if [ "$fail" -eq 0 ]; then
    echo "Lockfile version check passed ($EXPECTED)."
else
    echo
    echo "Run 'cargo update --workspace' and 'npm install --package-lock-only' to resync,"
    echo "and add the file to extra-files in .release-please-config.json if it is missing."
fi

exit "$fail"
