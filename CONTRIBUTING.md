# Contributing

## Setup

```sh
just setup
```

This installs npm dependencies, builds the frontend once (`tauri-build` needs `dist/` to exist), and installs the pre-commit hook.

`just check` additionally needs a few one-time cargo tools that `just setup` does *not* install automatically (they're not needed until you run `just check`, and `cargo-llvm-cov`'s component is a sizeable download):

```sh
rustup component add llvm-tools-preview
cargo install cargo-deny cargo-llvm-cov
cargo install cargo-dupes --locked --version 0.2.1
```

## Before pushing

```sh
just check
```

That is exactly what CI runs: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, the file-size gate, `cargo deny check`, `cargo dupes check`, `cargo llvm-cov` (coverage floor), and the frontend suite (`tsc --noEmit` + `vitest`).

The pre-commit hook (`just install-hooks`) runs the fast subset — fmt, clippy, file-size, `cargo deny`, `cargo dupes` — on every commit. It skips coverage and the full test/frontend suites since those are slower; run `just check` yourself before pushing to catch those.

### Quality ratchets

Two of the gates are ratchets, not fixed bars. Their numbers live in exactly
one place — `justfile` — and CI and the pre-commit hook run the `just`
recipes directly (`just dupes`, `just coverage`) rather than restating the
thresholds, so there is nothing to keep in sync:

- **Coverage floor** (`coverage_min_lines`/`_functions`/`_regions` in `justfile`): only ever raise these as coverage improves. Never lower them to make a change pass — add tests instead.
- **Duplication ceiling** (`dupes_max_exact`/`_near`/`_percent` in `justfile`): only ever lower these as duplication is cleaned up. Never raise them to let new duplication in.

If a change is going to move either number, update `justfile` only, and say so in the commit message.

## Ground rules

- **No `unwrap`/`expect`/`panic`/`todo` in production code** — these are deny-level lints. Handle the error or return it. Test modules may `#![allow(clippy::unwrap_used)]`.
- **Keep crates in their lane.** `meter-core` has no I/O; `meter-api` owns HTTP; `src-tauri` owns platform integration. UI state stays in the frontend.
- **Files under 500 lines.** If a module grows past that, split it.
- **Every change ships with tests.** API-shape changes update the fixtures in `crates/meter-api/tests/fixtures/` and the contract tests beside them.
- **Never log secrets.** `SessionKey` redacts itself in `Debug`/`Display`; keep it that way, and mark any header carrying it as sensitive.
- Commits follow [Conventional Commits](https://www.conventionalcommits.org/) (`feat(scope): ...`, `fix(scope): ...`).

## Releasing

Push a `v*` tag; `.github/workflows/release.yml` builds and publishes signed
macOS + Linux artifacts from it with no further manual steps. See
[`docs/packaging.md`](docs/packaging.md).
