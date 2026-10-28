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
| `rsb-gauge-schema` | 0 | The rule and heuristic types, the TOML loader, their JSON Schemas, the registry linter, and `repos.toml` with its closed set of roles. |
| `rsb-gauge-reconcile` | 1 | Observation, situation, action and event as closed enums; the pure `decide` with first-match precedence; the store port and its in-memory fake. No I/O. |
| `rsb-gauge-store` | 2 | The store as a SQLite file at `GAUGE_STORE`: the port's adapter, WAL mode, a busy timeout, versioned migrations. |
| `rsb-gauge-cli` | exempt | The headless binary `gauge`. Every command the app will have exists here first. No key. |
| `agent-driver` | exempt | The binary that holds the GitHub App key, mints narrowed tokens and talks to GitHub as the App, and runs the tick. The only crate allowed a GitHub or HTTP client. |
| `xtask` | exempt | `cargo xtask layers` (layers and custody), `cargo xtask launch` (custody in source) and `cargo xtask errors`. |

Planned, each joining the layer table when it is created:

| Crate | Layer | Holds |
|---|---|---|
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
cargo run -p rsb-gauge-cli -- registry lint <root>                # load <root>/rules and <root>/heuristics and report problems
cargo run -p rsb-gauge-cli -- registry schema rule|heuristic      # print the JSON Schema for a rule or heuristic file
cargo run -p agent-driver -- check --repo <owner/name> --issue <n>   # print an issue's title with a read-only token
```

The loop. Every command reads the store file from `GAUGE_STORE`; nothing
defaults it. A store is tied at creation to the `repos.toml` whose repos its
ticks may open loops on: `agent-registry`'s for the live store, a file
listing only a scratch repo for a dev store. Where the store lives and how
it is created: `runbooks/store.md`.
The GitHub App, its key and which repos it reaches:
`runbooks/github-app.md`.

```sh
export GAUGE_STORE=~/.local/state/gauge/live.sqlite
gauge store init --repos ../agent-registry/repos.toml   # create the store, once
gauge run <repo>#<issue> [--agent claude|codex]         # write a run request
gauge pause [<loop>]  /  gauge drain [<loop>]           # write a flag
agent-driver tick [--observe]                           # one pass; --observe writes nothing
gauge runs                                              # loops and their last situation
gauge events --since <seq> [--follow]                   # the event log by cursor
```

`agent-driver` reads its App id and key path from
`~/.config/rsb/agent-driver/config.toml`, and how attempts run from its
optional `[attempt]` section:

```toml
[attempt]
agent_name = "..."        # the bot identity agent-box commits as;
agent_email = "..."       # both required to start an attempt
model = "claude-opus-5-5"                              # the default
runtime_bound = 3600                                   # seconds, the default
agent_box = "~/code/software/rsb/factory/agent-box"    # the default
rsb_skills = "~/code/software/rsb/factory/rsb-skills"  # the default
```

Status: S-0001, the thin loop, ticket 06 (first attempt: an open loop
gets its brief, its workspace and one agent-box unit).

An attempt lives in `~/.local/share/gauge/runs/<loop>/<attempt>/`
(`$XDG_DATA_HOME/gauge/runs`): `repo/`, the clone on the attempt's
branch, and `run/` with `brief.json`, `outbox/`, and the unit's
`agent.json` and `drive.log`. It runs as the transient user unit
`gauge-run-<loop>-<attempt>` (`systemctl --user status <name>`).

A tick polls every repo `repos.toml` gives role `project` or `reference`
that the driver's config lists, for open issues carrying
`rsb-workflow:run`; each new label event is one request. A request opens
a loop only when the default branch carries `.rsb/governance.lock` and
the issue's `<!-- rsb-ticket S-NNNN/N -->` marker resolves to a ticket
file there; otherwise it is refused with its reason.

Related repositories, siblings under `~/code/software/rsb/factory/`:
`../agent-registry` holds the standards shared by the rsb apps and the
rule registry Gauge reads, `../agent-box` is the container agents run
in, `../agent-host` holds the at-desk sandbox configuration, and
`../rsb-skills` holds how work is done. `../agent-driver` is the old
home of the driver, folded in here and archived.
