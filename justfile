# Coverage ratchet floor (line/function/region %). Only ever raise these
# numbers, and only when `just coverage` reports a new sustained value at or
# above the raise — never lower them to make a change pass. See
# CONTRIBUTING.md for the ratchet policy.
coverage_min_lines := "87"
coverage_min_functions := "87"
coverage_min_regions := "87"

# Duplication ratchet ceiling (cargo-dupes). Only ever lower these numbers as
# duplication is cleaned up — never raise them to let new duplication in.
dupes_max_exact := "12"
dupes_max_near := "3"
dupes_max_exact_percent := "5.9"
dupes_max_near_percent := "1.5"
dupes_excludes := "--exclude 'target/*' --exclude 'node_modules/*' --exclude 'dist/*' --exclude 'src-tauri/gen/*'"

# Run all checks (what CI runs)
check: fmt-check lint test file-size lockfile-versions deny dupes coverage frontend-typecheck frontend-test

# One-time setup after cloning
setup:
    npm install
    npm run build
    just install-hooks

# Format code
fmt:
    cargo fmt

# Check formatting
fmt-check:
    cargo fmt -- --check

# Run clippy (warnings are errors)
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Run all Rust tests
test:
    cargo test --workspace

# Run the frontend test suite (vitest)
frontend-test:
    npm run test

# Typecheck the frontend without emitting (fast strict-mode gate)
frontend-typecheck:
    npx tsc --noEmit

# Typecheck and build the frontend (required before first cargo build: creates dist/)
frontend:
    npm run build

# Run the app with hot reload
dev:
    npm run tauri dev

# Produce a release bundle for the current platform
build:
    npm run tauri build

# Check source file sizes
file-size:
    bash scripts/check-file-sizes.sh

# Every version-carrying file agrees with Cargo.toml — the guard on
# release-please's extra-files list (see the script's header)
lockfile-versions:
    bash scripts/check-lockfile-versions.sh

# cargo-deny: licenses, security advisories, banned/duplicate-major crates
deny:
    cargo deny check

# cargo-dupes: structural code duplication, ratcheted against the ceiling above
dupes:
    cargo dupes check --exclude-tests {{dupes_excludes}} \
        --max-exact {{dupes_max_exact}} \
        --max-near {{dupes_max_near}} \
        --max-exact-percent {{dupes_max_exact_percent}} \
        --max-near-percent {{dupes_max_near_percent}}

# Test coverage via cargo-llvm-cov, ratcheted against the floor above.
# Requires the llvm-tools-preview rustup component (`rustup component add
# llvm-tools-preview`), which `just setup` does not install automatically
# since it's a large one-time download — CI installs it via taiki-e/install-action.
coverage:
    cargo llvm-cov --workspace \
        --fail-under-lines {{coverage_min_lines}} \
        --fail-under-functions {{coverage_min_functions}} \
        --fail-under-regions {{coverage_min_regions}}

# Install git hooks (run once after cloning)
install-hooks:
    chmod +x scripts/hooks/pre-commit
    ln -sf ../../scripts/hooks/pre-commit .git/hooks/pre-commit
    @echo "Git hooks installed."

# ---------------------------------------------------------------------------
# Linux desktop harness (harness/README.md). Local developer tooling only —
# nothing below runs in CI, and none of it is part of `just check`.
# ---------------------------------------------------------------------------

# Serve demo usage data on :8787 for the harness VMs. Runs in the foreground.
demo-server:
    cargo run --manifest-path harness/demo-server/Cargo.toml

# Change what the demo server returns, live: `just scenario critical`,
# `just scenario failure 401`, or no argument to show the current state.
scenario *ARGS:
    harness/bin/scenario.sh {{ARGS}}

# Create/boot/provision a harness VM: `just vm-up gnome` or `just vm-up kde`.
vm-up TARGET:
    harness/bin/vm.sh up {{TARGET}}

vm-down TARGET:
    harness/bin/vm.sh down {{TARGET}}

# Open the VM's desktop in macOS Screen Sharing.
vm-vnc TARGET:
    harness/bin/vm.sh vnc {{TARGET}}

# Screenshot the VM's desktop to harness/artifacts/ — how the tray gets checked.
vm-shot TARGET *NAME:
    harness/bin/vm.sh shot {{TARGET}} {{NAME}}

# Install the built .deb (gnome) or AppImage (kde) into the VM.
vm-install TARGET:
    harness/bin/vm.sh install {{TARGET}}

# Start the app inside the VM's desktop session.
vm-launch TARGET:
    harness/bin/vm.sh launch {{TARGET}}

# Build the Linux .deb and AppImage in the GNOME VM, into harness/artifacts/.
linux-build:
    harness/bin/build-linux.sh

# Containerised desktops (podman) — seconds to a desktop instead of the VM's
# minutes, and disposable. Both desktops, but neither the AppImage nor anything
# needing a GPU.

# Fresh container, app installed, wizard done: `just container-up gnome|kde`.
container-up TARGET:
    harness/bin/container.sh up {{TARGET}}
    harness/bin/container.sh install {{TARGET}}
    harness/bin/container.sh launch {{TARGET}}
    harness/bin/container.sh setup {{TARGET}}

container-down TARGET:
    harness/bin/container.sh down {{TARGET}}

# Screenshot the container desktop to harness/artifacts/.
container-shot TARGET *NAME:
    harness/bin/container.sh shot {{TARGET}} {{NAME}}

# Screenshot the tray icon magnified — colour and badge are unreadable at 1:1.
container-tray TARGET *NAME:
    harness/bin/container.sh tray {{TARGET}} {{NAME}}

# Anything else: status, eval <js>, appindicator on|off, wallet, logs, shell.
container *ARGS:
    harness/bin/container.sh {{ARGS}}

# Regenerate docs/screenshots/linux/ from the containers (both must be up).
linux-screenshots *TARGET:
    harness/bin/screenshots.sh {{TARGET}}

# Arch Linux **x86_64**, for packaging checks. The only non-aarch64 target:
# genuine Arch userspace under Rosetta, so `arch=('x86_64')` and the released
# amd64 AppImage are both real here. Headless on purpose.

# Create/boot/provision the Arch VM (first run is slow: bootstrap + keyring).
arch-up:
    harness/bin/arch.sh up

arch-down:
    harness/bin/arch.sh down

# Build packaging/aur/PKGBUILD the way an AUR user would, into harness/artifacts/.
arch-makepkg:
    harness/bin/arch.sh makepkg

# Exercise scripts/install.sh on a real pacman host: refusal, then --force.
arch-install-sh:
    harness/bin/arch.sh install-sh

# Anything else: shell, status, delete.
arch *ARGS:
    harness/bin/arch.sh {{ARGS}}
