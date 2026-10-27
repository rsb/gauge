---
id: S-0001/1
title: Workspace restructure and custody boundary
slice: groundwork
depends_on: []
serves: [gauge §1.1, §3.1 requirements 2 and 4, §6.3.4, §6.4.1]
contracts:
  provides:
    - to: S-0001/3
      shape: >
        The `gauge` Cargo workspace (formerly `easel`) with crates under
        `crates/`: `rsb-gauge-schema` (library, layer 0, renamed from
        `registry`), `rsb-gauge-cli` (binary `gauge`, renamed from
        `easel-cli`), `agent-driver` (binary, the kept modules folded in
        from the old `agent-driver` repo), and `xtask`. Library crates
        are named `rsb-gauge-*` and layered; there is no facade crate.
        The existing never-crash and errors-as-values conventions hold
        in every crate. `cargo xtask layers` also enforces custody:
        only `agent-driver` may depend, directly or transitively, on a
        crate in the custody table (GitHub client, HTTP client, process
        launcher), and no crate depends on `agent-driver`. Later tickets
        add crates through the same tables and keep both checks green.
  relies_on: []
seams:
  - "`cargo xtask layers` against seeded violating manifests in test fixtures"
  - "`gauge --version` and `agent-driver --version` from CI"
  - the existing `registry` tests, unchanged in substance, under the new crate name
---

# Workspace restructure and custody boundary

## Goal

Turn the existing `easel` workspace into the `gauge` monorepo the PRD
names, bring `agent-driver` inside it, and make custody a crate boundary
the layering check enforces, before any code that could break it is
written. This is a restructure of what exists, not a new workspace.

## What exists (read 2026-10-27)

- `gauge` (the repo renamed from `easel`, remote `rsb/gauge`): edition
  2024 workspace, members `crates/*` and `xtask`; crates `registry`
  (rule types, TOML loader, JSON Schema, linter; layer 0) and
  `easel-cli` (binary); `xtask layers` driven by
  `[workspace.metadata.layers]`; `xtask errors`; workspace lints deny
  `unwrap`, `expect`, `panic`, indexing and dropped results;
  `engineering/` holds `errors.md`, `layers.md`, `never-crash.md`; CI
  `check.yml` runs fmt, clippy, test and both xtasks on push. README and
  AGENTS.md still say Easel, `orchestration/` and `../skills`.
- `agent-driver` (separate repo, remote `rsb/agent-driver`): one crate,
  a gatekeeper design (`serve` answering agents' requests from a spool,
  a `gh` shim client, `desk` starting Claude Code with a masked token in
  `GH_TOKEN`). Record 1 §3 replaced that model with brief in, outbox
  out, and no credential for any agent at desk or in the box.

## Scope

1. **Owner step first:** confirm nothing on either machine runs
   `agent-driver serve` or `agent-driver desk`. If anything does, stop
   it before this ticket lands; an agent holding a token is the one hard
   line in record 1.
2. **Renames:** `registry` → `rsb-gauge-schema`; `easel-cli` →
   `rsb-gauge-cli` with binary `gauge`. Error and report names that say
   Easel follow (`easel_cli::report` and the like). The CLI keeps the
   subcommand `gauge registry lint|schema`: the command names the
   content it reads.
3. **Fold in `agent-driver`** as `crates/agent-driver`, keeping only:
   `jwt.rs`, `tokens.rs`, `github.rs`, `config.rs`, `repo.rs`,
   `error.rs`, and `audit.rs` as the seed of events. Not moved: `desk`,
   `serve`, `client` (the `gh` shim), `spool`, `policy`. Its CLI keeps
   only what the kept modules support (the read-only `check` command is
   a useful smoke test for ticket 4). Dependencies join the workspace
   table. The old repo is archived on GitHub with a README pointer to
   `gauge`; its history stays readable there (no subtree merge).
4. **Custody table** in the root `Cargo.toml` beside the layer table:
   the crates that count as a GitHub client, HTTP client or process
   launcher (`ureq`, `jsonwebtoken`, `libc` and the like, as data), and
   the one crate allowed them. `xtask layers` fails on a violation,
   including a transitive one, and on any crate depending on
   `agent-driver`. Layer table: `rsb-gauge-schema = 0`; binaries and
   `xtask` exempt.
5. **Docs:** README and AGENTS.md say Gauge, the `factory/` layout and
   `../rsb-skills`; the crate table lists the planned `rsb-gauge-*`
   crates with their layers. `engineering/layers.md` gains the custody
   rule beside the layer rule. `engineering/` otherwise stays until the
   factory profile exists (its rules then move to the registry; the
   xtasks stay as code).
6. **No `insights` crate** yet (created when it has content).
7. **Containment path:** done before this ticket (2026-10-27,
   `chore/factory-layout-paths`): the agent-box deny path is
   `~/code/software/rsb/factory/agent-box/**`, and sibling references
   point at `../agent-registry` and `../rsb-skills`. Docs still say
   Easel; renaming them is step 5.

## Out of scope

Any domain type, store or tick code (03). New GitHub calls (05).

## Criteria

- [x] Owner confirms `serve` and `desk` are not in use (desktop
      checked 2026-10-27: no process, no service, no alias, no caller
      in the factory repos; only the installed binary
      `~/.local/bin/agent-driver`, removed once `gauge` installs its
      own). Laptop to check the same way.
- [ ] The `agent-driver` repo is archived with a pointer.
- [ ] `cargo build`, `cargo test --workspace`, clippy, fmt and both
      xtasks pass in CI under the new names; the schema crate's existing
      tests pass unchanged in substance.
- [ ] `xtask layers` fails on a fixture where `rsb-gauge-schema` (or
      any future `rsb-gauge-*` crate) depends on a custody-table crate,
      directly or transitively, and on one where `rsb-gauge-cli` depends
      on `agent-driver`; it passes on the real workspace.
- [ ] `gauge registry lint <dir>` and `gauge registry schema` behave as
      `easel-cli` did; `gauge --version` and `agent-driver --version`
      exit 0.
- [ ] `agent-driver` contains none of `desk`, `serve`, the `gh` shim, or
      any code path that places a token in a child process's
      environment.
- [ ] No file in the repo still names Easel except history.
