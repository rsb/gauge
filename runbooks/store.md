# The store

Where Gauge keeps its loops, requests, flags and event log, and how the
owner creates and points at it. Prose, no secrets: the store holds none,
and nothing here names a credential.

## Decision (owner, 2026-10-28)

At M1 the store is a SQLite file on the desktop. M1 is one machine and
one tick, so nothing needs to be shared yet. SQLite is the store's
long-term dialect. At M5, when two machines, leases and the phone need a
shared store, it moves behind the Worker into Cloudflare (Durable
Objects or D1, both SQLite) in the owner's personal paid account, never
the gordana-milan account. The schema and queries written now carry
over; only what surrounds them changes, and that move gets its own
design session. This revises D5: no Supabase project is created for
Gauge.

## Where it lives

Store files live in `~/.local/state/gauge/`, mode 700, outside every
checkout. The live store is `~/.local/state/gauge/live.sqlite`. The
directory was created once by hand:

```sh
mkdir -p ~/.local/state/gauge && chmod 700 ~/.local/state/gauge
```

Nothing mounts it into agent-box: the box mounts only named volumes.
At-desk agent sessions are denied reading it; `agent-host`'s
`claude/settings.json` lists `~/.local/state/gauge` under `denyRead`.
An agent therefore never sees the store; only the driver and the
`gauge` CLI, run by the owner or by the driver's own service, open it.

## Pointing at it

Every command takes the store file from `GAUGE_STORE`, and nothing
defaults it. A command run without it stops and says so, so a shell
that forgot the variable never writes a stray store somewhere else.

```sh
export GAUGE_STORE=~/.local/state/gauge/live.sqlite
```

## Creating it

A store is created once, with the `repos.toml` it is tied to:

```sh
gauge store init --repos ~/code/software/rsb/factory/agent-registry/repos.toml
```

This creates the file (mode 600), applies the schema and records the
absolute path of that `repos.toml` inside the store. Every tick reads
roles from that file and records its commit on the events that rest on
it. `init` refuses a file that already exists. Nothing else creates a
store: a tick or a command pointed at a missing file treats the store
as unreachable, takes no action and exits non-zero, so a typo in
`GAUGE_STORE` cannot start a fresh, empty store.

## Dev stores

A dev store is any other file the owner names, in the same directory or
in a scratch one. It is tied to a scratch `repos.toml` that lists only a
scratch repo, never `agent-registry`'s:

```sh
export GAUGE_STORE=~/.local/state/gauge/dev.sqlite
gauge store init --repos ~/scratch/repos.toml
```

A run request for any repo outside that file is refused `not_listed`,
so a dev tick can never open a loop on a live repo. To start over,
delete the dev file and run `init` again. Never delete `live.sqlite`.

## When it is unreachable

The tick treats a missing file, a lock held past five seconds and a
failed integrity check alike: it takes no action, names the reason
(`store_unreachable: …`) and exits non-zero. Check that `GAUGE_STORE`
names the right file, that no other process holds it, and, for an
integrity failure, keep the file aside for inspection before doing
anything else with it.
