---
id: S-0001/7
title: "Hand-back consumed: outbox to effects"
slice: feature
depends_on: [S-0001/6]
serves: [gauge §3.3 rows 3, 4, 19, actions, natural keys, posting; §6.2; §6.3; record 1 §3, §4; D2; effects record §3–§8, §12]
contracts:
  relies_on:
    - from: S-0001/6
      shape: attempt directory layout, unit name, attempt record, brief v1 with allowed_effects, brief_issued recorded
    - from: external
      shape: outbox schema v1 (manifest, kinds, per-kind replay and supervision); `implement` writing it and the manifest last
  provides:
    - to: S-0001/8
      shape: >
        The loop branch pushed by the App with `pushed` recorded per
        attempt; a PR opened from a `pr-open` effect, found by head
        branch, body exactly as the agent wrote it; each sealed outbox
        recorded once as `outbox_recorded` (valid, effects `pending`)
        or `rejected`, keyed by (loop, attempt); the effect machine
        (pending, performed, refused, superseded; unverified shaped)
        with every transition an event; attempt causes `no_outbox`,
        `intent_lost`, `work_lost`. A resume-brief builder per cause,
        with a slot for the last verdict that 08 fills.
    - to: S-0001/11
      shape: outbox recording and the effect machine, depending on no agent-specific field, so the M4 Codex adapter needs no change here
seams:
  - fake agent writing each outbox kind, sealed and unsealed, valid and invalid, with and without declarations
  - GitHub fake recording every write and able to refuse a write (closed issue, deleted branch) or fail transiently
  - kill harness: the fixed kill points 2 to 8 of the effects record §12
  - live: the reference repo shows the PR opened by the App, issue comment, ticked criteria
---

# Hand-back consumed: outbox to effects

## Goal

`handback apply`, which the owner runs by hand today, becomes the
driver's: what the agent asked for happens once, as the App, observed
before written, and the record of what was asked lives in the store,
not the tree (record 1 §4). Unfinished hand-backs are named and
finished by the custodian, or sent back to the agent, never left
ambiguous (effects record §1).

## Scope

- Rows 4 (push, record `pushed`), N7 (orphan, one-machine table test),
  N3 (record), N4 (reject, park `outbox_rejected`), N3b (perform), N8
  (pending, not due: wait), N10 (park `effect_refused`), 19a and 19b
  (park `agent_flagged`, `agent_blocked` after effects), 3 (failed
  attempt with a recorded cause: resume brief after backoff). Row N9
  (approval wait) is in the table as shaped and unreachable.
- **Sealing.** An outbox exists only when its manifest, written last,
  lists every entry file with a matching hash. Anything else is no
  outbox: cause `no_outbox`, row 3, never a rejection.
- **Validation before recording**, against the exact brief recorded as
  `brief_issued`: schema v1; every kind in `allowed_effects`; every
  target among the brief's targets; commits on the loop branch only.
  Invalid is rejected as a whole, payload attached, no effect rows.
- **Effect machine** per effects record §3: look up by world key; if
  present record `performed` and write nothing; else write, look up
  again, record. Transient responses are `pending` with tries and
  `due_at` (values provisional config, Q13; validated on the reference
  app, never on gordana-milan). A non-transient refusal, or tries
  exhausted, is `refused` with the response, and every later entry is
  `superseded` in the same step (ordered and stop).
- **Kinds at M1:** `pr-open` (title and body as written, never edited
  by the driver), `issue-comment` (marker in the body), `criteria-tick`
  on the ticket issue body, `deviation` (recorded only). Declarations
  `flagged-for-human` and `blocked`: no world write; the park notice
  carries the agent's text byte for byte. `review-findings` is not in a
  coder's `allowed_effects`, so it is a rejection.
- **Lost workspace**, nothing recorded: `intent_lost` if a `pushed`
  event exists for the attempt (brief: produce the hand-back from head
  X), else `work_lost` (brief: continue from the previous head). Both
  count against the budget.
- The `attempting` event and `unverified` state exist in the schema and
  the code paths are table-tested; no M1 kind emits them.
- Retiring the owner-run `handback apply` copy: once this ticket passes
  on the reference repo, the hand script stays only for gordana-milan's
  by-hand path.

## Out of scope

Reading `scorecard` (08). Park notices, the label, resume and abort
(09). Approval (later). Any kind with no lookup (later).

## Criteria

- [ ] An attempt that exits without a sealed outbox records `no_outbox`
      and starts a resume attempt; it is never recorded as rejected.
- [ ] An outbox with an unknown kind, a kind outside `allowed_effects`,
      a target not in the brief, or a missing field is recorded
      `rejected` with its payload, creates no effect rows, posts
      nothing, and parks `outbox_rejected`.
- [ ] For each M1 kind the effect is performed exactly once across any
      number of ticks and every fixed kill point 2 to 8; the GitHub
      fake shows no second write for any world key.
- [ ] A refused `criteria-tick` leaves the following `issue-comment`
      `superseded`, and the loop parks `effect_refused` once.
- [ ] A transient failure leaves the effect `pending` with a `due_at`;
      the loop waits and retries; exhausted tries end in `refused`.
- [ ] An outbox carrying `blocked` beside an `issue-comment` performs
      the comment, then parks `agent_blocked` with the reason verbatim.
- [ ] The structural invariant holds by table test: no row below the
      consume rows fires while an outbox is sealed and unrecorded, or
      recorded with pending effects.
- [ ] A workspace removed after the push and before recording yields
      `intent_lost`; removed before the push, `work_lost`.
- [ ] The PR's author is the App and its body is byte-identical to the
      `pr-open` entry.
