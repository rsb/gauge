---
id: S-0001/9
title: "Stopping the line: parks and owner controls"
slice: feature
depends_on: [S-0001/8]
serves: [gauge §3.3 rows 8, 9, 13–17, posting; §6.3.2; SP §7 parking, telling the owner]
contracts:
  relies_on:
    - from: S-0001/8
      shape: failing-row set per head, attempt counts, run record
  provides:
    - to: S-0001/11
      shape: >
        Every park reason in the closed set of the effects record §6
        (no_progress, attempt_limit, unrecognized_state,
        outbox_rejected, effect_refused, effect_unverified,
        agent_flagged, agent_blocked), each with its attribution (agent,
        world, system) and attachment, announced once by label
        `rsb-workflow:parked`, a PR or issue comment keyed (loop,
        reason, attempt), a desktop notification and an event.
        `gauge resume|abort <loop>` honoured on the next tick.
seams:
  - harness scenarios for each park reason and for resume and abort from each parked state
  - GitHub fake: one notice per (loop, reason, attempt) under kills
  - live: a forced `no_progress` on the reference repo, resumed by the owner
---

# Stopping the line: parks and owner controls

## Goal

The loop stops itself, says why and what it saw, and stays stopped
until the owner acts on state (SP §6.3).

## Scope

- Rows 9 (budget spent: `attempt_limit`), 8 (same checkpoint
  fingerprint on two consecutive attempts: `no_progress`; the M1
  fingerprint is the pair in `spec.md`), 15 (abort: stop, close
  `aborted`), 16 (parked and resume requested: clear, reset the
  no-progress memory, continue), 17 (parked: wait).
- `gauge resume <loop>` and `gauge abort <loop>` write request rows;
  nothing else. The attempt budget is a config value (decision 4).
  Counted causes: `exited_nonzero`, `timed_out`, `no_outbox`,
  `intent_lost`, `work_lost`. `stopped` is recorded and not counted
  (decision 7).
- The notice for every park carries its attribution, derived from the
  reason, and its attachment. For `agent_flagged` and `agent_blocked`
  the agent's text is the notice's only prose, byte for byte; the
  driver authors none.
- The per-agent count reads agent-attributed reasons only.
- Park notices as idempotent effects. The notice comment goes on the
  PR when one exists, else on the ticket issue (**proposal**). The
  desktop notification is best effort and never blocks the tick.
- Precedence check: budget and no-progress rows win over every row
  that starts an attempt (PRD §3.3); a table test pins it.

## Out of scope

Phone commands (M5). Owner-requested park (PRD: none exists).

## Criteria

- [ ] An attempt loop that keeps the same tree hash and failing rows
      parks `no_progress` after the second attempt, not later.
- [ ] Attempt n+1 is never started when n equals the budget.
- [ ] Each park produces exactly one label, one comment, one event,
      across kills.
- [ ] `resume` from each park reason continues from row 3's path;
      `abort` closes the loop and stops any unit.
- [ ] For every park reason and every closed vocabulary touched here
      (park reasons, attribution, attempt causes), the complement test
      exists and passes.
- [ ] Each of the eight park reasons produces a notice with the right
      attribution and attachment, driven by a harness scenario.
