# Errors

## errors

Each library crate has one error enum, derived with `thiserror`:
`RegistryError`, and one per crate as they come. A lower crate's error enters
a higher one through `#[from]`, so `source()` walks to the leaf. Callers
render the chain with `easel_cli::report`: the command first, then each
`Display`, joined by a colon. `Error::Internal` in `easel-cli` is the one
variant that is our fault, and no caller branches on it. Every other variant
names a thing the owner can act on. Results are not dropped: `unused_must_use`
and `let_underscore_must_use` are deny. `anyhow`, `eyre`, other error crates
and `Box<dyn Error>` do not appear.

**Why.** A typed error says what went wrong and who can fix it, and the chain
reaches the leaf without wrap machinery.

**Paths.** All crates.

**Check.** `cargo xtask errors` fails on a crate that lists `anyhow`, `eyre`,
`color-eyre`, `failure`, `error-chain` or `snafu`, and on a line that boxes a
`dyn Error`. `cargo clippy` enforces the must-use lints.

**Example.** `RegistryError` in `crates/registry/src/error.rs`; `Error` and
`report` in `crates/easel-cli/src/main.rs`.
