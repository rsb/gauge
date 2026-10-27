---
id: S-0001/8
title: CI as the sensor, the owner's wait, close
slice: feature
depends_on: [S-0001/7]
serves: [gauge §3.3 rows 2, 6, 7; §3.6; §6.5; build-order R2]
contracts:
  relies_on:
    - from: S-0001/7
      shape: pushed branch, PR by head branch, resume-brief builder
  provides:
    - to: S-0001/9
      shape: >
        Per PR head: the observed `scorecard` check run (status,
        conclusion, colour read from its output) and the sorted set of
        failing rows parsed from it. Attempt counts per loop. The run
        record (repo, pr) with agent, model, attempts, final colour,
        exit reason, push time of the head that got the final verdict,
        and the time that verdict completed.
    - to: S-0001/11
      shape: >
        The verdict and close path end to end: RED back to the coder,
        GREEN or AMBER held for the owner, merge or close ends the loop
        with one run record carrying push and verdict times.
seams:
  - GitHub fake with scripted `scorecard` check runs (pending, RED with rows, AMBER, GREEN)
  - kill harness: PR merged while an attempt runs; RED seen, tick killed before the resume attempt
  - live: a RED on the reference repo goes back to the coder and returns GREEN
---

# CI as the sensor, the owner's wait, close

## Goal

The loop reads the verdict the reference repo's Actions job already
posts, sends RED back, waits on GREEN or AMBER, and closes when the
owner decides. The driver posts no check run at M1 (R2).

## Scope

- Rows 20 (pending: wait), N5 (RED, budget left: resume brief with the
  verdict summary and the failing rows, after backoff), 21 (GREEN or
  AMBER: wait for the owner), 7 (merged or closed: stop any attempt,
  close the loop, write the run record).
- The colour and failing rows are read from the `scorecard` check run
  that `pr.yml` writes (its output format is the contract, copied from
  gordana-milan's `verdict.mjs`; the ticket names the field it parses).
  An output the parser cannot read parks `unrecognized_state` rather
  than guessing.
- The run record (Q3, run-level, beside the per-rule rows that arrive
  at M2) with push and verdict times, so D3 latency is computable from
  the first PR.
- The owner's acts are unchanged and outside the driver: read the
  verdict, `read-diff:yes|no`, merge with a merge commit.

## Out of scope

Scoring, posting `scorecard`, the ruleset cutover (M2). JSONL export
(not M1). Parks other than `unrecognized_state` (09).

## Criteria

- [ ] RED on the head starts the next attempt once, with a resume
      brief that carries the failing rows; a killed tick starts no
      second one.
- [ ] GREEN or AMBER produces no action until the PR is merged or
      closed.
- [ ] A PR closed or merged while an attempt runs stops the unit and
      closes the loop on the next tick.
- [ ] The run record exists once per PR with both timestamps; push to
      verdict for the reference repo's first loop is reported by
      `gauge runs`.
- [ ] Nothing in the driver creates or updates a check run.
