# Never crash

## never-crash

Outside tests, no code can panic: no `unwrap`, `expect`, `panic!` or
indexing. `unsafe` is forbidden in every crate. One `catch_unwind`, around
command dispatch in `easel-cli`, turns a panic that slips through into
`Error::Internal` naming the command, and the run reports instead of
crashing. It is the only catch in the codebase, and it moves into an engine
crate when the desktop app needs one.

**Why.** A control center that crashes mid-run leaves the owner without the
report that was the point of the run, and an agent driving the CLI gets a
line it can act on instead of a dead process.

**Paths.** All crates. `crates/easel-cli/src/main.rs` for the catch.

**Check.** `cargo clippy --workspace --all-targets -- -D warnings`, through
`[workspace.lints]` in the root `Cargo.toml` and `clippy.toml`, which allows
the four in tests.

**Contract.** `dispatch` in `crates/easel-cli/src/main.rs`.
