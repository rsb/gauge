---
id: S-0001/10
title: "Codex adapter: the contract's second implementation"
slice: feature
status: dropped
dropped_reason: R1 declined by the owner on 2026-10-27; the Codex adapter stays at M4 (Gauge PRD §7). `spec apply` closes this ticket's issue as not planned.
depends_on: []
serves: [gauge §3.4; D6; D7]
contracts:
  relies_on: []
  provides: []
seams: []
---

# Codex adapter (dropped from S-0001)

Dropped, not deleted: ticket ids are never reused (record 1 §2). The
work moves to a later spec at M4. What it carries forward from S-0001:

- The `AgentAdapter` trait and the contract test suite from S-0001/6.
  The Codex adapter must pass the same suite; any difference it exposes
  is fixed in the adapter or recorded as a contract change, never as a
  special case in the tick.
- Outbox recording and the effect machine from S-0001/7 are
  agent-agnostic by contract, so they need no change for Codex.
- Agent selection (S-0001 decision 5): a new loop with no `--agent`
  gets the other agent from the last loop opened on that repo; retries
  within a loop keep the loop's agent. The brief is the complete
  handoff, so the choice of agent does not change what the agent can
  know.
