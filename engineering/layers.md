# Layers

## layers

A crate depends only on crates in strictly lower layers. Binaries and tooling
are exempt, and no layered crate depends on them.

| Layer | Crate | Holds |
|---|---|---|
| 0 | `registry` | Rule types, the TOML loader, the JSON Schema, the registry linter. No I/O past reading rule files. |
| exempt | `easel-cli`, `xtask` | Binaries and tooling. |

Planned: `render` and `checks` at 1 (they read the registry), `score` and
`mint` at 2, `lab` at 3, `easel` exempt. A crate joins the table when it
is created, not before.

**Why.** The registry stays a pure data layer every other crate can read
without pulling in adapters, scoring or git, and the engine stays headless
with the CLI and the app on top.

**Paths.** Every `Cargo.toml` in the workspace.

**Check.** `cargo xtask layers`, against `[workspace.metadata.layers]` in the
root `Cargo.toml`. A new crate fails the check until it has a layer there.

**Contract.** `Cargo.toml`, `[workspace.metadata.layers]`.
