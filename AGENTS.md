# AGENTS.md

Read `README.md` first. Sessions run from this repository. Standards shared by
the rsb apps live in `../rules`; read its `README.md`. The rules Easel reads
live there too; this repository holds no rule content beyond test fixtures.

Commits follow Conventional Commits. Never add `Co-Authored-By` or any other AI
attribution to commits or pull requests. Outside ticket work, do not commit or
push unless asked. Dates and times are America/Los_Angeles.

The engine is headless. Every capability is a library crate with a command in
`easel-cli` before it has a window. The desktop app, when it comes, calls the
library and adds no logic the CLI lacks.

Checks. Add `--message-format short` to check, test and clippy. CI runs the
last five on every push.

- Typecheck: `cargo check --workspace --all-targets`
- One test file: `cargo test -p <crate> --test <file>`
- Full suite: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format: `cargo fmt --check`
- Layers: `cargo xtask layers`
- Errors: `cargo xtask errors`

The rules these enforce are in `engineering/`.
