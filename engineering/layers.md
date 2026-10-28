# Layers

## layers

A crate depends only on crates in strictly lower layers. Binaries and tooling
are exempt, and no layered crate depends on them.

| Layer | Crate | Holds |
|---|---|---|
| 0 | `rsb-gauge-schema` | Rule and heuristic types, the TOML loader, their JSON Schemas, the registry linter, `repos.toml`. No I/O past reading rule and heuristic files and `repos.toml`. |
| 1 | `rsb-gauge-reconcile` | The closed enums, the pure `decide`, the store port and its in-memory fake. No I/O. |
| 2 | `rsb-gauge-store` | The SQLite adapter for the store port. |
| exempt | `rsb-gauge-cli`, `agent-driver`, `xtask` | Binaries and tooling. |

Planned: `rsb-gauge-render` and `rsb-gauge-checks` at 1, `rsb-gauge-score`
and `rsb-gauge-mint` at 2, `rsb-gauge-lab` at 3,
`insights` exempt. A crate joins the table when it is created, not before.

**Why.** The schema stays a pure data layer every other crate can read
without pulling in adapters, scoring or git, and the engine stays headless
with the CLI and the app on top.

**Paths.** Every `Cargo.toml` in the workspace.

**Check.** `cargo xtask layers`, against `[workspace.metadata.layers]` in the
root `Cargo.toml`. A new crate fails the check until it has a layer there.

**Contract.** `Cargo.toml`, `[workspace.metadata.layers]`.

## custody

Only `agent-driver` depends, directly or transitively, on a crate in the
custody table: a GitHub client, an HTTP client, the JWT signer or a process
launcher. No crate depends on `agent-driver`. Only `agent-driver` and `xtask`
start a process, outside test code. The GitHub App key exists in
that one binary; the `rsb-gauge-*` libraries, `rsb-gauge-cli` and `insights`
cannot link a client that would use it.

A crate under `transitive` counts at any depth of the resolved graph, through
workspace and registry crates alike. A crate under `direct` counts only where
a crate lists it itself: `libc` and `nix` sit under every async runtime and
socket library, and what custody forbids is calling them. Dependencies of
every kind count, dev and build included.

**Why.** An agent holding a token is the one hard line (PRD §6.4). A crate
boundary the check enforces holds where a convention would drift, and it
runs on Gauge itself.

**Paths.** Every `Cargo.toml` in the workspace, and every `.rs` file in a
crate other than `agent-driver` and `xtask`, outside `tests/`.

**Check.** Two checks, one per side of the boundary.

- The dependency graph: `cargo xtask layers`, against
  `[workspace.metadata.custody]` in the root `Cargo.toml`. A violation names
  the crate, the custody crate and the path between.
- The source: `cargo xtask launch` fails on a line naming `Command::new`, or
  `std::process` other than `ExitCode`, `Termination`, `exit`, `abort` and
  `id`, outside comments and `#[cfg(test)]` items. The standard library is no
  dependency, so the graph cannot see it. The scan reads text: a launcher
  reached through an import that splits the path, such as
  `use std::{process}`, is caught only where `Command::new` follows.

The seeded workspaces in `xtask/tests/fixtures` prove each failure.

**Contract.** `Cargo.toml`, `[workspace.metadata.custody]`: `holder`, and the
`transitive` and `direct` lists. The scan exempts the holder and `xtask`.
