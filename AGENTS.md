# AGENTS.md

Read `README.md` first. Sessions run from this repository. Standards shared by
the rsb apps live in `../agent-registry`; read its `README.md`. The rules Gauge
reads live there too; this repository holds no rule content beyond test
fixtures. How work is done lives in `../rsb-skills`.

Commits follow Conventional Commits. Never add `Co-Authored-By` or any other AI
attribution to commits or pull requests. Outside ticket work, do not commit or
push unless asked. Dates and times are America/Los_Angeles.

The engine is headless. Every capability is a library crate with a command in
`rsb-gauge-cli` (the `gauge` binary) before it has a window. The desktop app,
when it comes, calls the library and adds no logic the CLI lacks. Only
`agent-driver` holds the GitHub App key; no other crate depends on it or on a
GitHub or HTTP client, or starts a process (`engineering/layers.md`).

Checks. Add `--message-format short` to check, test and clippy. CI runs the
last six on every push, and `--version` on both binaries.

- Typecheck: `cargo check --workspace --all-targets`
- One test file: `cargo test -p <crate> --test <file>`
- Full suite: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format: `cargo fmt --check`
- Layers: `cargo xtask layers`
- Errors: `cargo xtask errors`
- Launch: `cargo xtask launch`

The rules these enforce are in `engineering/`.
