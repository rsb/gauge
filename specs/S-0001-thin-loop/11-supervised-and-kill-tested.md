---
id: S-0001/11
title: Supervised and kill-tested on one machine
slice: feature
depends_on: [S-0001/6, S-0001/7, S-0001/8, S-0001/9]
serves: [gauge §3.3 triggers; §6.2; §7 M1 acceptance; SP §7 tick, triggers, crash scopes]
contracts:
  relies_on:
    - from: S-0001/8
      shape: the verdict and close path
    - from: S-0001/9
      shape: every park and owner control
    - from: S-0001/6
      shape: the Claude adapter and the fake agent passing the contract suite
    - from: S-0001/7
      shape: agent-agnostic outbox recording and the effect machine
    - from: external
      shape: the reference repo with at least two applied one-node specs
  provides: []
seams:
  - end to end, live, on the reference repo, Claude
  - kill test: random kills of the tick and of an attempt during the live runs
---

# Supervised and kill-tested on one machine

## Goal

M1's acceptance. The loop runs on the desktop under systemd with no
hand in it from label to the owner's merge, with Claude as the coder
(Codex joins at M4 through the same contract suite), and
survives being killed anywhere.

## Scope

- systemd user units shipped in `gauge`: a oneshot service for
  `agent-driver tick`, a timer at the provisional cadence (about five
  minutes, Q-C1), and a trigger on an attempt unit's exit that asks
  for a tick. A trigger during a tick yields exactly one more tick.
- The owner runbook for installing and removing them, hand-written now
  (rendered at M3).
- The end-to-end run: two one-node specs on the reference repo
  (written at desk, applied as issues), each labelled
  `rsb-workflow:run` and taken to a merged PR by the owner. At least
  one of them goes RED once and comes back.
- The kill test (PRD §6.2, single machine): during those runs, kill
  the tick and the attempt at random points, a fixed number of times
  each (number set in the ticket), and restart from the store and the
  world only. The fixed kill points 1 to 8 of the effects record §12
  are each hit live at least once.
- The first **probe ticket** (effects record §11): a one-node spec on
  the reference repo whose purpose is to drive the loop into
  ordered-and-stop. Its spec names the owner act and window (close the
  ticket issue, or otherwise make the world refuse, between the attempt
  sealing and the tick recording) and writes the expected end state per
  effect before the run: `criteria-tick` `refused`, `issue-comment`
  `superseded`, one park `effect_refused`. The probe loop carries the
  probe mark so Learn excludes it (its form is open in the effects
  record).

## Out of scope

Two machines, leases and takeover (M5). The Codex adapter (M4). The
App-posted `scorecard` and the ruleset cutover (M2). Phone commands
(M5). Tuning backoff and the attempt budget beyond their provisional
values (validated on the reference app after M1).

## Criteria

- [ ] Both specs reach a merged PR with no owner command between the
      label and the merge other than reading the verdict.
- [ ] Across all kills: no duplicate PR, comment, label, criteria
      tick, park notice or event; no attempt started twice for one
      number; every loop reaches the same end state as an unkilled run.
- [ ] Every action in the event log maps to one row of the M1 table;
      no event carries a situation outside it except
      `unrecognized_state` parks, each with its observation.
- [ ] The run records give push to verdict for both PRs.
- [ ] The probe ticket ends exactly in its written end state, and the
      ordered-and-stop cost it shows (a comment blocked by an unrelated
      refusal) is recorded against the effects record's Watch item.
- [ ] After a forced outage of the tick, `tick --observe` reports the
      same current attempt for every loop as the store holds.
- [ ] The kill test procedure and its results are written up for
      `research/rsb-workflow/verification/`.
