---
id: S-0001/2
title: Store location on the desktop
slice: human
depends_on: []
serves: [gauge §2.5, D5 as revised 2026-10-28]
contracts:
  provides:
    - to: S-0001/3
      shape: >
        A directory on the desktop for the store's SQLite files, outside
        every repo and never mounted into agent-box:
        `~/.local/state/gauge/`, mode 700. The live store is
        `~/.local/state/gauge/live.sqlite`; dev stores are any other
        file the owner names. The path is passed to the driver and CLI
        through `GAUGE_STORE`; nothing defaults to it.
  relies_on: []
seams: []
---

# Store location on the desktop (owner)

## Decision (owner, 2026-10-28)

At M1 the store is a SQLite file on the desktop. M1 is one machine and
one tick, so nothing needs to be shared yet. SQLite is the store's
long-term dialect: at M5, when two machines, leases and the phone need a
shared store, it moves behind the Worker into Cloudflare (Durable
Objects or D1, both SQLite) in the owner's personal paid account, never
the gordana-milan account. The schema and queries written now carry
over; only what surrounds them changes. That M5 move gets its own
design session. This revises D5: no Supabase project is created for
Gauge.

## Steps

- Create `~/.local/state/gauge/` with mode 700.
- Confirm agent-box mounts nothing at or above it (search agent-box and
  agent-host for `.local`).
- Record the path in the runbook (`gauge/runbooks/store.md`, prose, no
  secrets) together with the decision above.

## Criteria

- [x] `~/.local/state/gauge/` exists with mode 700 and is outside every
      checkout.
- [x] Nothing in agent-box or agent-host mounts it or a parent of it.
