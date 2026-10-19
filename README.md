# Easel

Control center for agent-driven development: a registry of engineering
rules with classified checks, per-PR adherence scorecards, and repo
minting, so the owner governs by measurement instead of reading every
diff. Easel is the stand the rsb apps are made on.

The system is a feedback control loop. Rules are the set point, the
coding agent is the actuator, correctness and coherence sensors measure
the codebase, a scorecard compares, feedback regenerates the agent's
context, and the owner sets the rules, reads the scorecard and merges.
The owner always merges `main`.

## Layout

One Cargo workspace. Crates inside it are bare; binaries are `easel-cli`
(headless) and, later, `easel` (the desktop app, a thin client).

| Crate | Holds |
|---|---|
| `registry` | The rule types, the TOML loader, the JSON Schema, the registry linter. |
| `easel-cli` | The headless binary. Every command the app will have exists here first. |
| `xtask` | `cargo xtask layers` and `cargo xtask errors`. |

Planned, in build order: `render` (projections into `AGENTS.md`,
`.claude/rules`, ESLint and Clippy tables), `checks` (adapters and
SARIF findings), `score` (scorecards and history), `mint` (profiles,
plan and apply), `lab` (trial evaluation, labels, pattern mining), and
`easel`.

The rules themselves live in `rsb/rules`, which Easel reads. Easel holds
no rule content of its own beyond test fixtures.

## Run

```sh
cargo run -p easel-cli -- registry lint <dir>    # load every rule under <dir> and report problems
cargo run -p easel-cli -- registry schema        # print the JSON Schema for a rule file
```

Status: phase 1, the registry schema and parser. The design and the
phased roadmap are in the "Agent Workflow Development" project.

Related repositories: `../rules` holds the standards shared by the rsb
apps and the rule registry Easel reads, `../agent-driver` holds the
GitHub App key and does GitHub actions for agents, `../agent-box` is the
container agents run in away from the desk, `../skills` holds how work
is done.
