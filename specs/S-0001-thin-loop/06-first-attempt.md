---
id: S-0001/6
title: "First attempt: workspace, brief, Claude adapter"
slice: feature
depends_on: [S-0001/5]
serves: [gauge §2.4 R16; §3.1 Operate (contract); §3.4; record 1 §3, §4; D6, D7]
contracts:
  relies_on:
    - from: S-0001/5
      shape: open eligible loops with resolved ticket path; GitHub port for clone
    - from: external
      shape: brief schema v1 in agent-registry; agent-box non-interactive launch; `implement` reading the brief
  provides:
    - to: S-0001/7
      shape: >
        Per attempt, a directory `<root>/<loop>/<attempt>/` holding
        `repo/` (a clone of the loop branch, created from the base sha
        on attempt 1) and `run/` with `brief.json` (v1) and an empty
        `outbox/`. The attempt runs as a transient unit named
        `gauge-run-<loop>-<attempt>` whose state and exit status the tick can
        observe. `brief_issued` recorded in the store with the brief's
        payload before the unit starts, keyed by (loop, attempt). An
        attempt record in the store: number, unit name, agent, model,
        start time, runtime bound.
    - to: S-0001/11
      shape: >
        `AgentAdapter` trait in `rsb-gauge-reconcile` (`AttemptSpec` in,
        `AttemptResult` out: commits, exit code, events where offered,
        tokens and dollars), with no process code in the library. A
        contract test suite any adapter must pass, run against the fake
        agent and the Claude adapter; the Codex adapter (M4) is held to
        the same suite.
seams:
  - fake agent adapter: scripted commit, scripted outbox, scripted exit
  - contract test suite over the trait
  - kill harness: tick killed after unit start and before the attempt record, and the reverse
  - live: one attempt in agent-box on the reference repo, Claude, ending in an outbox
---

# First attempt: workspace, brief, Claude adapter

## Goal

An open loop gets its first attempt in agent-box, with only the brief,
the branch and (later) the last verdict to work from (PRD §3.4
requirement 1).

## Scope

- Rows N6 (start attempt 1, gated by drain), 1 (running, wait), N2
  (past the runtime bound: stop the unit).
- Brief v1 written by the driver from observed state: repo, base sha,
  branch, ticket file path, spec id, the issue numbers for `Refs #n`
  and `Closes #n` (record 1 §3: the brief carries them, ticket files
  carry none), attempt number, kind `fresh`, outbox path, runtime
  bound, and `allowed_effects` computed from the role in code (coder:
  `pr-open`, `issue-comment`, `criteria-tick`, `deviation`,
  `flagged-for-human`, `blocked`; effects record §7). Its targets
  (issue numbers, branch) are the allowed targets. No GitHub
  credential, no store credential, nothing the agent could use to
  reach either.
- `brief_issued` recorded with the brief's payload before the unit
  starts (effects record §4).
- Attempt cause recorded at the end of every attempt that does not
  hand back: `exited_nonzero`, `timed_out`, `stopped` here; the rest
  in 07.
- Branch name derived from the ticket (`<type>/<issue>-<slug>`, as
  today); created locally from the base sha; not pushed here.
- Claude adapter: `claude -p`, JSON output, `CLAUDE.md` as the one-line
  pointer; whether `--bare` is used is decided here and recorded (Q17).
- The attempt's unit exit is observable by the next tick (the trigger
  wiring is 11).
- Agent: Claude only at M1 (R1 declined; Codex at M4). The request's
  `--agent` is stored; `codex` is refused with a reason until its
  adapter exists.

## Out of scope

Consuming the outbox, pushing, PRs (07). Resume briefs (07, 08).

## Criteria

- [ ] On the next tick after a loop opens, exactly one unit
      `gauge-run-<loop>-1` runs, in agent-box, with the workspace mounted
      and no credential in its environment or mounts beyond the agent's
      own auth (D7).
- [ ] A tick killed between starting the unit and recording the
      attempt does not start a second unit on restart: the unit name
      is the natural key and is looked up first.
- [ ] An attempt past its runtime bound is stopped and recorded as
      failed.
- [ ] The Claude adapter and the fake agent both pass the contract
      suite.
- [ ] The brief validates against brief schema v1; an invalid brief is
      never written (fail closed: the loop parks instead).
- [ ] `brief_issued` exists in the store for every started unit, and
      kill point 1 (between `brief_issued` and the unit starting)
      starts exactly one unit on restart.
- [ ] An attempt past its bound records cause `timed_out`; a non-zero
      exit with no sealed outbox records `exited_nonzero`.
