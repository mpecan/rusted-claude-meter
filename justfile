# Run all checks (what CI runs)
check: fmt-check lint test file-size

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

# Install git hooks (run once after cloning)
install-hooks:
    chmod +x scripts/hooks/pre-commit
    ln -sf ../../scripts/hooks/pre-commit .git/hooks/pre-commit
    @echo "Git hooks installed."
