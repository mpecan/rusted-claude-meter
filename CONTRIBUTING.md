# Contributing

## Setup

```sh
just setup
```

This installs npm dependencies, builds the frontend once (`tauri-build` needs `dist/` to exist), and installs the pre-commit hook.

## Before pushing

```sh
just check
```

That is exactly what CI runs: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and the file-size gate.

## Ground rules

- **No `unwrap`/`expect`/`panic`/`todo` in production code** — these are deny-level lints. Handle the error or return it. Test modules may `#![allow(clippy::unwrap_used)]`.
- **Keep crates in their lane.** `meter-core` has no I/O; `meter-api` owns HTTP; `src-tauri` owns platform integration. UI state stays in the frontend.
- **Files under 500 lines.** If a module grows past that, split it.
- **Every change ships with tests.** API-shape changes update the fixtures in `crates/meter-api/tests/fixtures/` and the contract tests beside them.
- **Never log secrets.** `SessionKey` redacts itself in `Debug`/`Display`; keep it that way, and mark any header carrying it as sensitive.
- Commits follow [Conventional Commits](https://www.conventionalcommits.org/) (`feat(scope): ...`, `fix(scope): ...`).
