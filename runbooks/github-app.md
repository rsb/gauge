# The GitHub App

How the driver acts on GitHub, what it may do, where its key lives, and
how a repo is added to or removed from its reach. Prose, no secrets:
the ids below are not credentials, and the key itself is never quoted.

## The App

One App serves every repo the driver works on (decided 2026-10-27).

| | |
|---|---|
| App | `rsb-agent-bot`, owned by the `rsb` account |
| App id | 5352731 |
| Installation id | 172257129 (the `rsb` account) |
| Settings | github.com/settings/apps/rsb-agent-bot |
| Installation | github.com/settings/installations/172257129 |

Only `agent-driver` holds the key. No agent holds a GitHub credential,
in the box or at the desk, and the `gauge` CLI holds none either; the
custody check in `cargo xtask layers` keeps HTTP and JWT crates out of
every crate but `agent-driver`.

## Permissions

Repository permissions, everything else No access, no webhook events
(M1 polls on each tick):

| Permission | Access | Used for |
|---|---|---|
| Contents | read and write | push the loop branch |
| Issues | read and write | comments, ticked criteria, labels, park notices |
| Pull requests | read and write | open the PR from a `pr-open` effect |
| Checks | read | read the Actions `scorecard` (M1 reads; posting is M2) |
| Metadata | read | mandatory |

**Workflows stays No access, on purpose.** GitHub refuses a push from
the App that touches `.github/workflows/`, so no attempt can weaken or
remove the `scorecard` check that judges it. Such a push fails, and the
loop parks. CI changes are the owner's, by PR.

Raising a permission makes GitHub ask the installation to accept it;
accept on the installation page before the next tick, or the App is
treated as not installed.

## The key

`~/.config/rsb/agent-driver/app.pem`, mode 600, beside the driver's
config at `~/.config/rsb/agent-driver/config.toml`:

```toml
[app]
id = 5352731

[repos."rsb/ref-rsb-serverless-stack"]
```

The driver refuses a key file that others can read. The path is outside
every checkout. agent-box mounts no host path but its bundles and its
managed settings, and its home is a fresh tmpfs, so the key cannot be
seen from inside a box. At-desk agent sessions are denied
`~/.config/rsb/agent-driver` in `agent-host`'s `claude/settings.json`.
Never copy the key elsewhere; a second copy is a second boundary to
keep.

## What a token can reach

Two limits, both enforced by GitHub:

- **The installation list is the real boundary.** A leaked key reaches
  every repo the installation holds, whatever any token asks for. A repo
  is added when it joins the loop, not before.
- **Each token is narrower.** The driver mints installation tokens per
  repo with only the permissions the action needs, held in memory and
  never written down. A tick touching one repo cannot use another's.

Installed as of 2026-10-28: `ref-rsb-serverless-stack`, plus `lumen`,
`libraw` and `errors-rs` (kept or removed at the owner's call until they
join). gordana-milan is added in the same change that moves it onto the
loop.

## Adding a repo

1. The repo exists, its `main` ruleset requires `scorecard` from GitHub
   Actions, and `repos.toml` gives it role `project` or `reference`.
2. Installation page → Configure → Only select repositories → add it →
   Save.
3. Add `[repos."rsb/<name>"]` to the driver's `config.toml`.
4. Check: create an issue, then
   `agent-driver check --repo rsb/<name> --issue <n>` prints its title.

Removing is the reverse: take it off the installation first, then out of
the config.

## Checking the boundary

Verified for ticket S-0001/4 on 2026-10-28:

- `agent-driver check --repo rsb/ref-rsb-serverless-stack --issue 1`
  printed the issue's title.
- The same key, with `rsb/gordana-milan` added to the config for the
  test, was refused: the App is not installed on it.
- Inside agent-box (`HOME=/home/node`): no `~/.config/rsb`, no
  `/home/rsb`, and no `app.pem` anywhere on the filesystem.

Repeat these after any change to the installation, the key's location
or agent-box's mounts.

## When the App looks uninstalled

`agent-driver check` says "the App is not installed on <repo>" when the
repo is not on the installation, when a permission change is waiting
for the installation to accept it, or when a change was not saved.
Check the installation page for the repo and for a review banner.
