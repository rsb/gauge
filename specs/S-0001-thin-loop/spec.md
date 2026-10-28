---
id: S-0001
slug: thin-loop
repo: gauge
status: draft
prd: prd/gauge.md (Gauge PRD, Draft 0.1)
serves: [gauge §1.1, §3.1, §3.2, §3.3, §3.4, §3.5, §3.7, §6.1, §6.2, §6.3, §6.4, §7 M1]
records:
  - decisions/2026-10-26-user-flows-and-boundaries.md
  - decisions/2026-10-26-review-escapes-and-debt.md
  - decisions/2026-10-26-registry-entities-releases-blueprints.md
  - decisions/2026-10-27-profile-components.md
  - decisions/2026-10-27-effects-and-unfinished-work.md
target_repo: ref-rsb-serverless-stack (role `reference`)
---

# S-0001 Thin loop: one ticket from label to merge, by tick

Items marked **proposal** are Claude's and not yet confirmed. The spec
format itself is a proposal until the spec schema lands in
`agent-registry` (skills alignment step 1); this spec is meant to be
the first worked example that schema is checked against.

## Intent

The owner labels one ticket issue on the reference app
`rsb-workflow:run`. From then on nothing is done by hand until the
merge: `agent-driver tick`, run by systemd on the desktop, opens a Work
Stream Loop, starts an attempt in agent-box with a brief, consumes the
attempt's outbox, pushes and opens the PR as the App, reads the
`scorecard` check that Actions posts, sends RED back to the coder with
a resume brief, waits on GREEN or AMBER, and closes the loop with a
history record when the owner merges. Any kill of the tick or the
attempt, at any point, leaves the loop able to reach the same end
state with no effect performed twice. Claude is the coder at M1; Codex
joins at M4 through the same adapter contract.

This is M1 of the Gauge PRD, revised by the build-order review of
2026-10-26 (R2 accepted: the driver observes `scorecard` and does not
post it until M2; R1 declined: the Codex adapter stays at M4).

## Not in this spec

- Posting `scorecard` as the App, the ruleset `integration_id`
  cutover, situation row 5 (M2).
- Registry parsing beyond `repos.toml`; renders; `gauge check`; any
  rule, finding, fingerprint of a finding, baseline or debt (M2, M3).
- Leases, a second machine, the Worker, `/rsb-workflow` comments,
  `run` on the epic (M5). The table is shaped so the epic row is one
  addition.
- Cross-review as a sensor row (after the cross-review session).
- A shared store. At M1 the store is a SQLite file on the desktop;
  the shared Cloudflare store (SQLite in Durable Objects or D1) comes
  at M5 with its own design session (D5 as revised 2026-10-28; ticket
  2).
- JSONL export of history into git (**proposal**: store only at M1;
  Q19 stays open).
- Insights. No empty `insights` package is created (**proposal**).
- gordana-milan. Nothing in this spec touches it.

## How this spec is built

`gauge` is a `factory` repo (record 1, 1b): tickets are implemented at
desk, owner with an agent at the terminal, one ticket one PR, never as
a box attempt. Execution follows topological order. `human` tickets are
owner steps and hold their dependents at wait.

## The graph

```
01 ─┐
    ├─► 03 ─┐
02 ─┘       ├─► 05 ─► 06 ─► 07 ─► 08 ─► 09 ─► 11
04 ─────────┘          └─────┴─────┴──────────►┘
```

(10 dropped: R1 declined; the Codex adapter is M4.)

| Ticket | Slice | Depends on |
|---|---|---|
| 01 Workspace restructure and custody boundary | groundwork | — |
| 02 Store location on the desktop | human | — |
| 03 A run request becomes an open loop | feature | 01, 02 |
| 04 Factory App installed on the reference repo | human | — |
| 05 The world observed: label in, ineligible repos refused | feature | 03, 04 |
| 06 First attempt: workspace, brief, Claude adapter | feature | 05 |
| 07 Hand-back consumed: outbox to effects | feature | 06 |
| 08 CI as the sensor, the owner's wait, close | feature | 07 |
| 09 Stopping the line: parks and owner controls | feature | 08 |
| ~~10 Codex adapter~~ | dropped | — (M4) |
| 11 Supervised and kill-tested on one machine | feature | 06, 07, 08, 09 |

Execution is topological order (record 1, 2a); this is desk work.

## Contracts, by edge

Each edge's contract is stated in both ticket files (`provides` on the
producer, `relies_on` on the consumer). Summary:

| Edge | What crosses it |
|---|---|
| 01 → 03 | The `gauge` workspace restructured (`rsb-gauge-schema`, `rsb-gauge-cli`, `agent-driver` folded in); the custody table in `xtask layers` green and kept green |
| 02 → 03 | The store directory `~/.local/state/gauge/` on the desktop, outside every repo and every box mount; the store file is passed as `GAUGE_STORE` |
| 03 → 05 | Observation, Situation, Action types; pure `decide` with first-match precedence; tick skeleton with per-loop boundary; event log with cursor; the store ports and their fakes; the kill harness |
| 04 → 05 | App id, installation on the reference repo, private key path on the desktop readable by `agent-driver` only |
| 05 → 06 | GitHub port with fake; observed issue and lockfile state; eligible open loops |
| 06 → 07 | `AgentAdapter` trait; attempt as a transient unit with a known name; `run/` and `repo/` layout; brief v1 written; fake agent |
| 06 → 11 | The trait and the contract test suite, passed by the Claude adapter and the fake agent |
| 07 → 08 | Pushed branch and open PR by head branch; outbox consumed and recorded; resume brief builder |
| 07 → 11 | Outbox recording and the effect machine, agent-agnostic |
| 08 → 09 | Verdict observation per head; the failing-row set per verdict; attempt counts |
| 08, 09 → 11 | Every M1 row implemented and table-tested |

## External contracts this spec relies on

Not tickets here. Each is produced elsewhere and frozen before the
ticket that names it.

| Contract | Producer | Needed by |
|---|---|---|
| Brief schema v1, with `allowed_effects` and targets as the allowed targets (effects record §7) | `agent-registry`, skills alignment step 1 | 06 |
| Outbox schema v1: the manifest written last; kinds `pr-open`, `issue-comment`, `criteria-tick`, `deviation` (recorded only), `review-findings` (reviewer only), declarations `flagged-for-human` and `blocked`; per kind `replay` and `supervision`, `replay = safe` iff a lookup exists | `agent-registry`, skills alignment step 1 | 07 |
| `implement` reads the ticket file and `allowed_effects` from the brief, writes the outbox and the manifest last | `rsb-skills`, step 5 | 06, 07 |
| `repos.toml` with roles `factory`, `project`, `record`, `reference` | `agent-registry`, owner PR | 03 |
| agent-box can be launched non-interactively with a workspace, a run directory and an agent name, and exits | `agent-box` (verify; extend if not) | 06 |
| The reference repo at align state: walking skeleton, `.rsb/governance.lock` with root chain and mounts per `decisions/2026-10-27-profile-components.md` (present and hashed at M1, validated only at M3), `AGENTS.md` render with one region per mount, Actions `scorecard` as the one required check, a `specs/` folder with at least two one-node specs applied as issues | owner, pre-M1 | 05, 11 |

## The M1 situation table (proposal)

First match wins. Precedence is part of the contract and is tested as
a table in `rsb-gauge-reconcile`. Numbers are the Gauge PRD's where a row exists
there; `N`-rows are new. Row 5 (score and post) and row 18 (lease) are
not M1.

Revised 2026-10-27 by `decisions/2026-10-27-effects-and-unfinished-work.md`
(the effects record): rows N3 to N9 and 19a/b, row 3 narrowed, the
park list and attempt causes from that record.

| Order | # | Situation | Action | Ticket |
|---|---|---|---|---|
| — | — | Store unreachable | tick exits, no action | 03 |
| 1 | 15 | Abort requested | stop the attempt if running (cause `stopped`); close the loop `aborted` | 09 |
| 2 | 13 | Pause flag, global or this loop | none | 03 |
| 3 | 7 | PR merged or closed | stop any attempt (cause `stopped`); close the loop; write the run record | 08 |
| 4 | 16 | Parked, resume requested | clear the park; reset the no-progress memory; continue | 09 |
| 5 | 17 | Parked | wait | 09 |
| 6 | N1 | Run requested on a repo not eligible (role not `project` or `reference`, or no lockfile on the default branch; or a repo the driver's config does not list, `not_reached`) | refuse the request with its reason; no loop | 03 (role), 05 (lockfile) |
| 7a | N11 | Run requested, eligible, a loop for this issue that is not closed (open or parked) | refuse the request `loop_already_open`; leave the loop untouched | 03 |
| 7b | N12 | Run requested, eligible, the issue names no ticket file on the default branch | refuse the request `no_ticket` with its cause; no loop | 05 |
| 7 | 11 | Run requested, eligible, no loop for this issue that is not closed | open the loop; a request for an agent with no adapter yet (`codex` until M4) is refused `no_adapter` | 03, 06 |
| 8 | 1 | Attempt running, within its runtime bound | wait | 06 |
| 9 | N2 | Attempt running past its runtime bound | stop the unit; failed attempt, cause `timed_out` | 06 |
| 9 | N13 | Attempt's unit exited, its end not recorded | record the end; cause `exited_nonzero` when non-zero (or a signal) with no sealed outbox | 06 |
| 10 | 4 | Attempt exited, unpushed commits in its workspace | push the branch; record `pushed` | 07 |
| 11 | N7 | Workspace or sealed outbox for an attempt that is not the loop's current attempt | discard; record `workspace_discarded` (one-machine table test at M1; leases at M5) | 07 |
| 12 | N3 | Sealed outbox on disk, not recorded, valid against the brief | record `outbox_recorded`; effects created `pending` | 07 |
| 13 | N4 | Sealed outbox on disk, not recorded, invalid against the brief | record it `rejected` with payload; park `outbox_rejected` | 07 |
| 14 | N3b | Recorded, effects pending and due | perform pending effects in order, each looked up before written; a refusal supersedes later entries | 07 |
| 15 | N8 | Recorded, effects pending, not yet due (`due_at`) | wait (incomplete) | 07 |
| 16 | N9 | Effect pending with `awaiting: approval` | wait (shaped; no M1 kind reaches it) | — |
| 17 | N10 | Outbox progress `failed` (an effect `refused`, or `unverified`) | park `effect_refused` or `effect_unverified` | 07 |
| 18 | 19a | Outbox progress `done`, declares `flagged-for-human` | park `agent_flagged`, the agent's text attached | 07 |
| 19 | 19b | Outbox progress `done`, declares `blocked` | park `agent_blocked`, the agent's reason attached | 07 |
| 20 | 9 | Attempt budget spent | park `attempt_limit` | 09 |
| 21 | 8 | Same checkpoint fingerprint on two consecutive attempts | park `no_progress` | 09 |
| 22 | N5 | `scorecard` RED on the PR head, budget left | new attempt with a resume brief carrying the verdict, after backoff | 08 |
| 23 | 3 | Attempt failed with a recorded cause (`exited_nonzero`, `timed_out`, `no_outbox`, `intent_lost`, `work_lost`), budget left | new attempt with the cause's resume brief, after backoff | 07 |
| 24 | N6 | Loop open, no attempt yet | start attempt 1 | 06 |
| 25 | 14 | Drain flag | gate: rows 22 to 24 start nothing; lower rows still apply | 03 |
| 25 | N14 | Another attempt's unit (`gauge-run-*`) running | gate: rows 22 to 24 start nothing; lower rows still apply. M1 runs one attempt at a time because every box shares the one Claude sign-in, whose refresh two boxes would race (the known limit from agent-box PR A, de5eed5) | 06 |
| 26 | 2 | PR open, `scorecard` not yet complete on the head | wait | 08 |
| 27 | 6 | `scorecard` GREEN or AMBER on the head, PR open | wait for the owner | 08 |
| 28 | 10 | Anything else | park `unrecognized_state`, observation attached | 03 |

**The structural invariant** (effects record §4), pinned by a table
test: no row below 15 fires while a loop has a sealed unrecorded outbox
or a recorded outbox with pending effects. Push is not an outbox entry;
row 4 above the consume rows is the precondition.

**Observation fields added:** `outbox_on_disk` (none, sealed),
`outbox_recorded` (absent, valid, rejected), `effects` (per index:
state, tries, `due_at`, `awaiting`), `attempt_cause`.

**Workspace gone, nothing recorded.** The cause is decided by whether a
`pushed` event exists for the attempt: yes → `intent_lost` (resume
brief: produce the hand-back from head X); no → `work_lost` (resume
brief: continue the ticket from the previous head). Both count against
the budget.

**Park reasons** (closed, each with attribution): `no_progress`,
`attempt_limit` (agent); `unrecognized_state` (system);
`outbox_rejected` (agent); `effect_refused`, `effect_unverified`
(world); `agent_flagged`, `agent_blocked` (agent, by declaration).

**Attempt causes** (closed): `exited_nonzero`, `timed_out`,
`no_outbox`, `intent_lost`, `work_lost`, `stopped`. `stopped` is
recorded and not counted against the budget (decision 7).

## Natural keys at M1 (proposal where not in the PRD)

| Effect or record | Key |
|---|---|
| Request from a label | (repo, issue, label event id) |
| Loop | (repo, issue, ordinal); at most one open per (repo, issue) |
| Attempt unit | `gauge-run-<loop>-<attempt>`, e.g. `gauge-run-42-1` (decided 2026-10-27) |
| Brief issued | (loop, attempt), recorded before the unit starts |
| Outbox recording | (loop, attempt) |
| Effect (store) | (loop, attempt, index); world key per kind below |
| Push | branch at sha: no-op if the remote already has it; `pushed` recorded per attempt |
| PR | head branch |
| Issue comment | marker in the body carrying (loop, attempt, outbox entry index) |
| Criteria tick | (issue, criterion text) on the issue body |
| Park notice | (loop, reason, attempt) |
| Run record | (repo, pr) |

## Decisions needed before tickets start

| # | Decision | Blocks | Leaning (proposal) |
|---|---|---|---|
| 1 | R1 (Codex in M1) and R2 (observe, not post, `scorecard`) | scope | **decided** 2026-10-27: R1 declined (ticket 10 dropped, Codex at M4); R2 accepted |
| 4 | Attempt budget | 09 | **decided** 2026-10-27: 3, a config value, revisited at the D3 review |
| 2 | Row 19: where `flagged-for-human` and `blocked` land | 07 | **resolved** (effects record §5): declarations beside effects; effects perform, then park `agent_flagged` or `agent_blocked` |
| 3 | An outbox left unconsumed on the same machine | 07 | **resolved** (effects record §8): consume it; row 3 is for a workspace that is gone (`intent_lost`, `work_lost`) or no sealed outbox (`no_outbox`) |
| 7 | `stopped` not counted against the attempt budget | 09 | **accepted** 2026-10-27 (effects record §8) |
| 8 | `agent-driver tick --observe` lands in ticket 3 | 03 | **accepted** 2026-10-27 (effects record §9) |
| 5 | Agent when a request names none | M4 | **decided** 2026-10-27: alternate per repo (the other agent from the last loop opened there), effective when Codex lands at M4; Claude only until then. The brief is the complete handoff, so the agent's identity changes nothing it can know. Retries keep the loop's agent (decided 2026-10-27, matching how the owner works today): one PR, one coder, clean per-agent attribution, and `no_progress` compares like with like |
| 6 | Names: the reference repo (and so its repo leaf; profile node names are in the profile record), the factory App, the unit prefix | 04, 05, 06 | reference repo **decided** 2026-10-27: `ref-rsb-serverless-stack`, the reference for the named profile `rsb-serverless-stack` (repo leaf of the same name). Factory App **decided**: the existing `rsb-agent-bot`, no rename, one App for every repo. Unit name **decided**: `gauge-run-<loop>-<attempt>`; Gauge owns the loop and `agent-driver` is its subsystem. All names settled. |

## Checkpoint fingerprint at M1 (proposal)

There are no findings at M1, so the PRD's fingerprint (rule,
normalised text, symbol) does not exist yet. The M1 checkpoint
fingerprint is the pair (tree hash of the pushed head, sorted set of
failing `scorecard` rows). Two consecutive attempts ending on the same
pair is no progress. At M2 the failing-row set is replaced by the
finding fingerprints, the tree hash stays.

## Test seams

- `rsb-gauge-reconcile`: `decide` and the precedence table by table tests; every
  closed vocabulary with a test that its complement parks or is
  refused (PRD 7.1 addition).
- `agent-driver`: four ports (store, GitHub, process launcher, clock)
  with in-memory fakes. A fake agent adapter that writes a scripted
  commit and outbox. A SQLite store in a temporary file for store tests, never
  the live store file.
- The kill harness (from 03): drives a scripted world through ticks
  with the fakes, kills at a seeded random point between any two port
  calls, restarts from the store and the fake world, and asserts the
  same end state and no repeated effect. Every later ticket adds its
  rows' scenarios to it. The named kill points 1 to 9 of the effects
  record §12 are fixed scenarios, not left to the random seed (1 in
  06; 2 to 8 in 07; 9 is schema-only at M1, no kind reaches it).
- The terminal ticket (11) is the end-to-end test across all
  contracts: live, on the reference repo, with Claude.

## PRD deltas this spec carries

For the Gauge PRD revision PR, beside the deltas the records list:
§3.3 the table above replaces rows 11 to 19 for M1; row 12 ("branch
pushed, no open PR") becomes the `pr-open` effect of row N3, since the
PR opens only from the outbox (record 1 §3); §3.2 `gauge run` takes
`--agent`; §3.5 adds Outbox consumption and the run record (Q3) as
entities; §3.6 the run record carries push time and verdict time so
D3 latency can be computed (closes the PRD's own proposal); §7 M1
content as R1 and R2.
