---
id: S-0001/2
title: Shared store reachable from the desktop
slice: human
depends_on: []
serves: [gauge §2.5, D5]
contracts:
  provides:
    - to: S-0001/3
      shape: >
        The separate Supabase project from D5, reachable from the
        desktop. A connection URL and credential stored outside every
        repo, for a role that owns the `gauge` schema and nothing else;
        no Supabase API role has grants on it. The location of the
        credential file is written in the owner runbook.
  relies_on: []
seams: []
---

# Shared store reachable from the desktop (owner)

## Steps

- Stand up or confirm the separate Supabase project (D5), never an
  application's database, from the existing runbooks.
- Create the `gauge` schema and a role that owns it; no grants to the
  API roles; Data API off (as gordana-milan's 0001).
- Put the URL and credential where `agent-driver` and `gauge` read
  them on the desktop; never in a repo, never mounted into agent-box.
- Note the steps in the runbook so the laptop can repeat them at M5.

## Criteria

- [ ] `psql` as the role from the desktop connects and can create a
      table in `gauge` and not in `public`.
- [ ] The credential path is outside every checkout and is not in
      agent-box's mounts.
