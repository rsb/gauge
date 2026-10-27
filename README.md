# Gauge

The engine of RSB Workflow: a registry of engineering rules with
classified checks, per-PR adherence scorecards, repo minting and the
loop reconciler, so the owner governs by measurement instead of reading
every diff. The design is `prd/gauge.md`; the work is cut into specs
under `specs/`.

The system is a feedback control loop. Rules are the set point, the
coding agent is the actuator, correctness and coherence sensors measure
the codebase, a scorecard compares, feedback regenerates the agent's
context, and the owner sets the rules, reads the scorecard and merges.
The owner always merges `main`.

## Layout

One Cargo workspace, the `gauge` monorepo. The engine is the layered
`rsb-gauge-*` library crates; there is no facade crate. Binaries sit on
top. Only `agent-driver` holds the GitHub App key, and custody is a
crate boundary: see `engineering/layers.md`.

| Crate | Layer | Holds |
|---|---|---|
| `rsb-gauge-schema` | 0 | The rule types, the TOML loader, the JSON Schema, the registry linter. |
| `rsb-gauge-cli` | exempt | The headless binary `gauge`. Every command the app will have exists here first. No key. |
| `agent-driver` | exempt | The binary that holds the GitHub App key, mints narrowed tokens and talks to GitHub as the App. The only crate allowed a GitHub or HTTP client. |
| `xtask` | exempt | `cargo xtask layers` (layers and custody), `cargo xtask launch` (custody in source) and `cargo xtask errors`. |

Planned, each joining the layer table when it is created:

| Crate | Layer | Holds |
|---|---|---|
| `rsb-gauge-reconcile` | 1 | Observation, situation, action and event; the pure `decide` (S-0001 ticket 03). |
| `rsb-gauge-render` | 1 | Projections into `AGENTS.md`, `.claude/rules`, ESLint and Clippy tables. |
| `rsb-gauge-checks` | 1 | Check adapters and SARIF findings. |
| `rsb-gauge-score` | 2 | Scorecards and history. |
| `rsb-gauge-mint` | 2 | Profiles, plan and apply. |
| `rsb-gauge-lab` | 3 | Trial evaluation, labels, pattern mining. |
| `insights` | exempt | The desktop view, a thin client, built last. |

The rules themselves live in `rsb/agent-registry`, which Gauge reads.
Gauge holds no rule content of its own beyond test fixtures.

## Run

```sh
cargo run -p rsb-gauge-cli -- registry lint <dir>   # load every rule under <dir> and report problems
cargo run -p rsb-gauge-cli -- registry schema       # print the JSON Schema for a rule file
cargo run -p agent-driver -- check --repo <owner/name> --issue <n>   # print an issue's title with a read-only token
```

`agent-driver` reads its App id and key path from
`~/.config/rsb/agent-driver/config.toml`.

Status: S-0001, the thin loop, ticket 01 (workspace and custody).

Related repositories, siblings under `~/code/software/rsb/factory/`:
`../agent-registry` holds the standards shared by the rsb apps and the
rule registry Gauge reads, `../agent-box` is the container agents run
in, `../agent-host` holds the at-desk sandbox configuration, and
`../rsb-skills` holds how work is done. `../agent-driver` is the old
home of the driver, folded in here and archived.
