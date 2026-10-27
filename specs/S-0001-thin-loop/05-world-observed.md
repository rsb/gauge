---
id: S-0001/5
title: "The world observed: label in, ineligible repos refused"
slice: feature
depends_on: [S-0001/3, S-0001/4]
serves: [gauge §3.3 observations, triggers, natural keys; §6.4.1; record 1 §1a, §2 `rsb-workflow:run`]
contracts:
  relies_on:
    - from: S-0001/3
      shape: decide, tick, store ports and fakes, kill harness
    - from: S-0001/4
      shape: App id, installation, key path
  provides:
    - to: S-0001/6
      shape: >
        A GitHub port in `agent-driver` (the only crate that may hold
        it) with an in-memory fake, observing per eligible repo: open
        issues carrying `rsb-workflow:run`, the issue body (spec id and
        ticket path), presence and content hash of `.rsb/governance.lock`
        on the default branch. Requests from labels, keyed by
        (repo, issue, label event). Open loops whose repo passed both
        eligibility tests. A clone operation on the same port: fetch
        an eligible repo at a given sha with an installation token
        scoped to that repo, into a path the caller names, with the
        token never written to the clone's config or to disk.
seams:
  - GitHub port fake with scripted issues and labels
  - kill harness: label seen, request inserted, tick killed, no duplicate request
  - live: label an issue on the reference repo, tick, loop opens
---

# The world observed: label in, ineligible repos refused

## Goal

The phone-shaped trigger works at desk: a label on the reference repo
opens a loop, and a repo that has not been minted or aligned cannot
host one.

## Scope

- App authentication from the key; installation tokens per tick, never
  written to disk or logs.
- Polling (webhooks stay optional, PRD §3.3) of `rsb-workflow:run`
  on repos in `repos.toml` with an eligible role.
- Row N1 by lockfile: no `.rsb/governance.lock` on the default branch
  refuses the request (record 1, 1a, "the lockfile is the mechanical
  test"). Lockfile content is read and its hash recorded on the event;
  it is not validated (M3).
- Spec and ticket resolution from the issue body: the bare spec id and
  local ticket id (record 1 §2), resolved to
  `specs/S-NNNN-<slug>/NN-<slug>.md` in the checkout; an issue whose
  body names no resolvable ticket file is refused with a reason
  (**proposal**: refused, not parked, since no loop exists yet).
- The label stays on the issue at M1 (**proposal**); the driver adds
  nothing to the issue in this ticket.
- Clone for 06: the GitHub port fetches an eligible repo at a sha into a
  caller-named path with a repo-scoped installation token passed per
  command, never stored in `.git/config`; the fake port clones from a
  local bare repository.

## Out of scope

Any write to GitHub (07). Epic `run` (M5).

## Criteria

- [ ] A labelled issue on the reference repo opens one loop on the next
      tick; a second tick and a killed tick open no second loop.
- [ ] The same label on a factory repo, or on a repo without the
      lockfile, opens nothing and records a refusal with its reason.
- [ ] The installation token never appears in an event payload, log
      line or file.
- [ ] The layering check still passes: the GitHub client is reachable
      only from `agent-driver`.
