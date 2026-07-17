# Coverage ratchet floor (line/function/region %). Only ever raise these
# numbers, and only when `just coverage` reports a new sustained value at or
# above the raise — never lower them to make a change pass. See
# CONTRIBUTING.md for the ratchet policy.
coverage_min_lines := "87"
coverage_min_functions := "87"
coverage_min_regions := "87"

# Duplication ratchet ceiling (cargo-dupes). Only ever lower these numbers as
# duplication is cleaned up — never raise them to let new duplication in.
dupes_max_exact := "13"
dupes_max_near := "4"
dupes_max_exact_percent := "6.0"
dupes_max_near_percent := "2.5"
dupes_excludes := "--exclude 'target/*' --exclude 'node_modules/*' --exclude 'dist/*' --exclude 'src-tauri/gen/*'"

# Run all checks (what CI runs)
check: fmt-check lint test file-size deny dupes coverage frontend-test

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
