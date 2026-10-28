---
id: S-0001/4
title: Factory App installed on the reference repo
slice: human
depends_on: []
serves: [gauge §6.4.1, D2]
contracts:
  provides:
    - to: S-0001/5
      shape: >
        The existing GitHub App `rsb-agent-bot` (one App for every repo
        the driver serves), installed on `ref-rsb-serverless-stack`,
        with the permissions the M1 effects need (contents write for
        push, pull requests write, issues write for comments, labels and
        body edits, checks read). Its private key on the desktop at a
        path readable by the user that runs `agent-driver`, outside
        every checkout and never mounted into agent-box. The App id and
        installation id written in the runbook. Installation tokens are
        minted per tick scoped to the repos of the loops being ticked
        (the access-token request names the repositories), so the
        installation covering more repos does not widen what one tick
        can touch.
  relies_on: []
seams: []
---

# Factory App installed on the reference repo (owner)

## Steps

- Add `ref-rsb-serverless-stack` to `rsb-agent-bot`'s installation
  on the `rsb` account, once the repo exists (it is created by hand
  pre-M1). No new App and no rename (decided 2026-10-27).
- The installation list is the key's real boundary (a leaked key
  reaches every installed repo, whatever the per-tick token scope), so
  a repo is added when it joins the loop, not before. As of 2026-10-27
  the installation holds `libraw`, `lumen`, `errors-rs`
  (`gm-serverless` removed by the owner). gordana-milan is added in the
  same change that moves it onto the loop. `lumen`, `libraw` and
  `errors-rs` may stay or go until they join (owner's call).
- Check the App's permissions against the list above; add only what is
  missing.
- Store the key on the desktop if not already there; record the App id,
  installation id and key path in the runbook.
- Confirm the reference repo's `main` ruleset requires `scorecard` from
  Actions (M1 reads it and does not post it; R2).

## Criteria

- [x] An installation token minted from the key and scoped to
      `ref-rsb-serverless-stack` can read its issues and cannot read
      any other repo.
- [x] The key is not readable from inside agent-box.

Done 2026-10-28: App id 5352731, installation 172257129, key at
`~/.config/rsb/agent-driver/app.pem`; checks recorded in
`runbooks/github-app.md`. Workflows permission left at No access, so no
attempt can change the `scorecard` workflow.
