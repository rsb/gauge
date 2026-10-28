---
id: S-0001/3
title: A run request becomes an open loop
slice: feature
depends_on: [S-0001/1, S-0001/2]
serves: [gauge §2.4 R15, R17, R18; §3.1 Operate (core); §3.2; §3.3 tick; §3.5; §3.7; §6.1.5; §6.2; §6.3.1–3]
contracts:
  relies_on:
    - from: S-0001/1
      shape: workspace, error convention, layering check green
    - from: S-0001/2
      shape: the store directory `~/.local/state/gauge/` and the `GAUGE_STORE` convention
  provides:
    - to: S-0001/5
      shape: >
        In `rsb-gauge-reconcile` (new, layer 1): `Observation`, `Situation`, `Action` and `Event`
        types as closed enums; a pure `decide(Observation) -> Action`
        with first-match precedence; the store schema v1 as migrations
        owned by the library (loop, request, flag, event with a
        monotonic sequence). In `agent-driver`: a tick that makes one
        pass over every open loop and every unhandled request, catches
        a failed reconcile at the loop boundary only, records one event
        per action, and exits. Store and clock ports with in-memory
        fakes. The kill harness, which later tickets extend with their
        scenarios.
seams:
  - decide: table tests over the rows this ticket owns
  - tick: against the fakes and against a SQLite store in a temporary file
  - kill harness: seeded random kill between port calls; same end state, no repeated event
  - live: one request on the shared store from the desktop
---

# A run request becomes an open loop

## Goal

The tracer bullet. The owner types `gauge run <repo>#<issue>`, runs
`agent-driver tick`, and `gauge events` shows a loop opened, with
nothing else needed. Every later ticket adds rows to the same `decide`
and scenarios to the same harness.

## Scope

- Store schema v1: loop, request, flag, event, in SQLite (a file at
  `GAUGE_STORE`). Migrations owned and applied by the library's
  migration step; version recorded. WAL mode and a busy timeout, since
  the CLI writes request rows while a tick runs. SQL stays within what
  Cloudflare's SQLite (Durable Objects, D1) supports: no extensions, no
  pragmas the schema depends on, so the schema carries over at M5. The
  store sits behind a port in `rsb-gauge-reconcile`; the SQLite adapter
  is its only implementation at M1. "Store unreachable" means the file
  is missing, locked past the timeout, or fails its integrity check.
- CLI: `gauge run <repo>#<issue> [--agent claude|codex]` writes a
  request row (the `--agent` value is stored; 10 uses it);
  `gauge pause|drain [<loop>]` write flags; `gauge runs` lists loops
  and their last derived situation; `gauge events --since <seq>
  [--follow]` reads by cursor. The CLI writes rows only; it never acts.
- `decide` rows from the spec table: store unreachable, 13 pause,
  N1 by role, 11 open loop, 14 drain (as a gate), 10 anything else.
- Role lookup: read `repos.toml` from a local checkout of
  `agent-registry`, record its commit sha on the event. Only `project`
  and `reference` are eligible (record 1, 1a; record 3 §5). A request
  for a factory repo is refused, which makes 1b a check, not a
  convention.
- Tick: run-to-completion, one action per loop per tick, a trigger
  during a tick sets a flag so one more tick follows (the flag only;
  systemd wiring is 11).
- Kill harness skeleton.
- Store selection is explicit: the driver and CLI read the store's
  file path from the environment (`GAUGE_STORE`), never a default. The repos
  a driver may tick come from config tied to the store (live: the
  `repos.toml` repos; dev: a scratch repo only), and a request for a
  repo outside that list is refused, so a dev tick can never open a
  loop on a live repo (ticket 2, two stores).
- `agent-driver tick --observe` (decision 8, effects record §9):
  runs observation and `decide` for every loop, prints the situation,
  the action it would take and any orphan, and acts on nothing. It is
  the same code path as the tick minus the act step, so it grows with
  every ticket's rows at no extra cost.

## Out of scope

GitHub, lockfile observation (05). Attempts (06). Leases and the
shared Cloudflare store (M5).

## Criteria

- [x] A request for an eligible repo opens exactly one loop, however
      many ticks run and wherever a tick is killed.
- [x] A request for a repo with role `factory` or `record`, or a repo
      absent from `repos.toml`, is refused with a reason event and opens
      no loop.
- [x] An observation matching no row parks `unrecognized_state` with
      the observation in the event payload.
- [x] With the store unreachable, a tick performs no action and exits
      non-zero with a named reason.
- [x] `gauge events --since n` returns every event after n in order;
      `--follow` resumes from the last sequence it printed after a
      restart.
- [x] Each closed enum has a test that an unknown outside value is an
      error value, never a default.
- [x] The layering check is still green: none of this is in a crate
      that may not hold it.
- [x] `tick --observe` prints the same situation and action a real
      tick would take for every loop, and writes nothing to the store
      or the world (asserted against the fakes).
