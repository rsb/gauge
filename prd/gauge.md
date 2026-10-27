# PRD: Gauge

| | |
|---|---|
| Slug | `gauge` |
| Status | Draft 0.1 |
| Date | 2026-10-23 |
| Owner and only user | Rob |
| Level | Engine. Sits under the system PRD (`rsb-workflow`). |
| Home | `gauge/prd/gauge.md` |
| Readers | Rob, and a coding agent reading this cold as the brief for building the engine |

**Conventions.** Same as the system PRD. "Must" is a requirement. Decisions are cited D1 to D6 (system PRD section 11). Anything marked **(proposal)** was added while drafting this document and has not been ruled on. **(proposal, carried)** marks a proposal the system PRD or the handoff already carries. A cell or sentence that says **open** is open on purpose: the sources do not decide it and nothing was invented to fill it.

**Source shorthand.** **SP** is the system PRD (`prd/rsb-workflow.md`), cited by section. **Ref** is the "RSB Workflow Reference" artifact, cited by section. **Handoff** is `handoff/2026-10-23-rsb-workflow-session.md`. Where Ref and the Handoff differ, the Handoff wins. This document does not repeat SP. It cites SP sections for goals, the loop, principles and the latency band, and says what Gauge must do to meet them.

---

## 1. Purpose and position

### 1.1 What Gauge is

Gauge is the engine of RSB Workflow. The workflow is the process. Gauge is the instrument that defines the rules, measures every change against them, learns which rules earn their place, and reconciles each loop one step at a time until a ticket has a verdict the owner can act on without reading the diff.

"Gauge" names the monorepo `gauge`. "The engine" names its library, `rsb-gauge`. The distinction matters because the monorepo contains one binary that holds the GitHub App key and the library does not.

| Package | Kind | Binary | Depends on | Holds the key |
|---|---|---|---|---|
| `rsb-gauge` | library | none | no GitHub client, no network except the recorded model call | no |
| `rsb-gauge-cli` | binary | `gauge` | `rsb-gauge` | no |
| `insights` | binary, built last | `insights` | `rsb-gauge` | no |
| `agent-driver` | workspace package, binary | `agent-driver` | `rsb-gauge`, a GitHub client | yes, alone |

`rsb-gauge` holds Define, Measure, Learn and the reconciler core, pure and key-less. The CLI and Insights must never depend on `agent-driver` (D2). Custody is a crate boundary, checked by a layering rule.

Insights is RSB Workflow's desktop view across all the owner's projects, not a Gauge group. It lives in this monorepo as a package because it shares the library's types. It composes what the CLI offers one command at a time into one place, and operates as a thin client: no domain logic of its own; every measurement, verdict and lifecycle change goes through `rsb-gauge`. It is specified in the system PRD, not here.

### 1.2 Where Gauge sits in the loop

SP section 5 names seven parts. Gauge supplies the machinery for most of them and none of the content of the first.

| Loop part (SP 5) | Gauge's share | Not Gauge's |
|---|---|---|
| 1. Rules | Parse, validate, render, resolve profiles, plan a mint | The content of the rules: `agent-registry` |
| 2. Agent | The adapter contract and the attempt lifecycle | The agent's inner loop, the sandbox (`agent-box`, `agent-host`) |
| 3. Change | Snapshot and classification | The PR itself, owned by GitHub |
| 4. Sensors | Adapter running, judgement invocation, adjudication, exceptions, verification | The tools the adapters call |
| 5. Verdict | Scorecard arithmetic and the history row. The driver posts it | Merging. Only the owner merges |
| 6. Owner | The CLI and Insights as the owner's surfaces | Every decision: merge, label, name, promote |
| 7. Feedback | Statistics, lifecycle, regenerated renders, the Lab, mining | Typing into an agent. Feedback writes to rules, never to the agent |

### 1.3 What Gauge is not

- Not the agent, the sandbox or the supervisor. Gauge keeps nothing alive.
- Not a CI system. At M1 the sensor is the existing CI (section 7).
- Not a durable-execution platform. Ideas are borrowed (exactly once by key, a replay policy per task kind, an event log with a cursor). The machinery is built in-house (Ref 13).
- Not a place where a model computes a score, adjudicates a finding or rewrites a render (SP 9).
- Not a review tool. It replaces reading the diff with evidence.

### 1.4 Where this PRD departs from the Ref

| Ref says | This PRD follows | Why |
|---|---|---|
| Label `gauge:run`, commands `/gauge pause\|resume\|abort` (Ref 16) | `rsb-workflow:run`, `/rsb-workflow …` | The owner addresses the system (Handoff). The exact syntax is still a proposal (SP 7). |
| `agent-driver` is a separate repo (Ref 05, 18) | Workspace package of `gauge` | D2 and the Handoff. |
| Profiles: registry or separate Governance context is open (Ref 06, 19) | Profiles in `agent-registry` under `profiles/`; a separate Governance context in the engine | D1. |
| Resume has two spellings, `gauge resume <loop>` and `gauge run <id> resume`; `run <id> park` exists (Ref 15) | Five top-level verbs: `run`, `pause`, `drain`, `resume`, `abort`. No owner-requested park | Handoff, "Things easy to lose". The park reasons are a closed set of three that the driver chooses (Ref 14). |
| "Four scales plus the instrument" in prose, five rows in the table (Ref 09) | Five scales; the per-rule scale is the instrument | SP 8 and the Handoff. |
| Milestones: SP 10 numbers the skeleton "Milestone 1" and the hand-run loop "Milestone 2" | The Handoff's M0 to M7 | It is the ordered build plan. The SP content is mapped in section 7. |

Ref 16, as dumped for this draft, is "Two machines, and delegating from away" and contains no list of inconsistencies, although the Handoff says it does. The rows above were found by comparing Ref with the Handoff and the decisions.

---

## 2. Responsibilities

Eighteen responsibilities, mint counted once, in four groups over one Record layer (Handoff, Ref 05). Mint is split: planning is Define (R03), applying is Operate, done by the driver with the key, and counted once under R03.

Data flows Define, Measure, Record, Learn, Define. Operate works across the loop but must never feed Learn directly, so nothing an operator surface does biases the statistics except through the record Measure wrote.

| Group | Question | Cadence | Purity | Human act at its boundary |
|---|---|---|---|---|
| Define | What should the code be, and what does each surface read? | per registry revision | pure over git data | Approve a promotion; name a pattern |
| Measure | How far is this change from the rules? | per change | pure except the recorded model call | Label a finding |
| Learn | Which rules earn their place, and what should the agent read next? | per window | pure over the record | Name a cluster |
| Operate | What is the one next action for each loop? | per tick | effectful, idempotent by natural key | Merge |

Each group has a closed vocabulary at its edge, and the complement of the list is a stop (section 6.3).

The cadences nest like cascade control (Ref 13): each outer loop sets the reference for the inner one.

### 2.1 Define

| ID | Responsibility | Must | In | Out | Pure |
|---|---|---|---|---|---|
| R01 | Parse and validate the registry | Parse every rule, pattern, profile and glossary file at a pinned revision into typed records. Reject unknown enum values, duplicate or reused ids, and an enforced rule with no check unless its class is judgement. | Registry revision | Typed registry, or lint problems | Yes |
| R02 | Render | Produce a deterministic projection of registry revision plus rule statistics for one target surface. Enforce the 100-directive budget across all surfaces combined. Stamp an output hash. No model writes a render. | Registry, statistics, target | Render with hash | Yes |
| R03 | Resolve profile chains and plan a mint | Resolve base, kind, variant, repo. For every path in the target repo report one outcome by ownership class. `--check` sets the exit code. `--json` feeds the scorecard. Apply (driver) writes a branch, commits with a `Governance-Rev:` trailer, opens a PR, and updates the lockfile. | Profile chain at revision, lockfile, repo files | Plan; at apply, a PR | Plan yes. Apply no |

### 2.2 Measure

| ID | Responsibility | Must | In | Out | Pure |
|---|---|---|---|---|---|
| R04 | Snapshot | Capture content hash, changed paths and a bounded diff for head against merge-base. Refuse secrets before any model sees them. | PR head, merge-base | Snapshot, or `blocked:secrets_in_diff` | Yes |
| R05 | Classify | Derive a closed set of signals from the snapshot and map them to a risk tier (simple, medium, complex) before any model runs. Record the result as history. | Snapshot, registry revision | Signals, tier | Yes |
| R06 | Run adapters | Run mechanical rules always, heuristic rules where `applies_when` matches, and the correctness adapters. Normalise every result to a finding or a correctness row with a stable fingerprint. | Snapshot, `checks[]` of selected rules | Findings, correctness results | See open Q1 |
| R07 | Judgement invocation | Run review or verify as a read-only model call, with selected lenses only, over the same adapter trait the coder uses. Require coverage attestation. Record the output and its cost under its key. | Snapshot, review brief, lenses | Recorded judgement, tokens, dollars | No. The one non-deterministic input |
| R08 | Adjudicate, exceptions, verify | In code: defer inferred-only evidence, defer findings outside the changed paths unless stale or missing-required, reject duplicates. Count exceptions. Fail the run on a suppression nothing uses. Verify only on a complex-tier change with no surviving findings. | Findings, attestations, exceptions | Accepted, deferred, rejected findings; verification result | Yes, given the recorded output |
| R09 | Scorecard | Compute counts against the merge-base baseline and the ratchet. RED, AMBER, GREEN by worst condition. Arithmetic only. | Accepted findings, correctness, baseline, exceptions, tier | Scorecard rows, verdict, summary text | Yes |
| R10 | Format and emit the row | Produce the history rows and the JSONL export record for the run. | Scorecard, classification, cost, exit reason, agent | History rows | Yes |

### 2.3 Learn

| ID | Responsibility | Must | In | Out | Pure |
|---|---|---|---|---|---|
| R11 | Statistics | Over a trailing window, per rule: sample size, interval bounds on the not-useful rate, fix rate, suppression rate by category, exposure, cost per useful finding. Stratify by agent, model and version. | History, labels, exposure | Rule statistics | Yes |
| R12 | Lifecycle and regenerated renders | Write a Transition with criterion and provenance for every lifecycle edge. Demotion applies automatically. Promotion to blocking stays proposed until the owner merges its PR. Regenerate renders from the new revision and statistics. | Statistics, criteria | Transitions, registry-PR content, new Render | Yes. The PR is opened by the driver |
| R13 | Cases, grading, regrading | Hold cases. Prove each fixture fails its oracle before anyone is graded against it. Grade recall per rule and per tier. Regrade historical runs from recorded outputs without spending tokens. Run the same cases under both agents. | Cases, recorded outputs, rules | Grades | Yes. Hidden tests are mounted only at grade time |
| R14 | Mining | Build co-change transactions (capped at 25 items, mass reformats excluded). Propose cluster candidates ranked by support, confidence, lift, Jaccard and stability. Never name one. | History | Cluster candidates | Yes |

### 2.4 Operate

| ID | Responsibility | Must | In | Out | Pure |
|---|---|---|---|---|---|
| R15 | Reconciler | The core, in the library (D2): Observation and Action types, the closed Situation table, a pure `decide`, the residue schema, lease semantics. The driver observes, acts and runs the tick. | Observation | One Action and one event per loop per tick | `decide` yes |
| R16 | Agent adapter and attempts | One adapter contract for Claude Code and Codex (D6). One attempt is one transient unit with its own workspace. The judge uses the same trait. | Workspace with renders, brief, per-attempt credentials | Commits, exit code, events where offered, cost | No. Driver side |
| R17 | CLI | The owner's commands. `run`, `pause`, `drain`, `resume`, `abort` only write rows the driver acts on. | Owner input | Rows; reports | Mixed |
| R18 | Events and the app | The event stream with a sequence cursor. Insights is a reader of the store and the stream. It never talks to the key. | Event rows | Stream; views | Reader only |

### 2.5 Record

The layer under all four. Append and compare-and-swap only. Entities are in section 3.5.

- **Store schema.** Versioned, owned by the library. Postgres on a separate Supabase project is truth; local SQLite is a cache (D5).
- **History rows** (3.6) and **events with a cursor** (3.7). The database is the live view.
- **JSONL export.** Written into git by the record step. Git is the record (Ref 08).
- **Judgement cache.** Recorded outputs addressed by key (6.1).
- **Leases.** One per loop, compare-and-swap on holder and expiry (D5).

---

## 3. Interfaces

### 3.1 Library API surface (`rsb-gauge`)

Types are named, not specified. Names from D1, D2 and Ref are used as given. Any other name is a working name **(proposal)**. Every function returns a typed error as a value. None panics.

| Group | Types in | Types out | Main operations |
|---|---|---|---|
| Define | `Registry` revision; `Rule`, `Pattern`, `Profile`, `GlossaryTerm`; `Signal` (engine enum, not registry data); `Lockfile`; `Statistics` | `Registry`, `LintProblem`, `Render` (target, inputs, budget, hash), `Plan` (per-path outcome) | load and lint; resolve chain; plan; render for a target |
| Measure | `Snapshot` input; `Registry`; `Baseline` | `Snapshot`, `Classification` (signals, `Tier`), `Finding`, `CorrectnessResult`, `Judgement`, `Exception`, `Scorecard`, `Verdict`, `HistoryRow`, `ExitReason` | snapshot; classify; select checks; adjudicate; verify decision; score; emit rows |
| Learn | `HistoryRow`, `Label`, `Render` exposure | `RuleStatistics`, `Transition`, `Case`, `Grade`, `CoChangeTransaction`, `ClusterCandidate` | compute statistics; propose transitions; verify and grade cases; regrade; mine |
| Operate (core) | `Observation`, `Residue`, `Lease` | `Situation`, `Action`, `Event` | `decide(Observation) -> Action`; lease claim and renewal rules |
| Operate (contract) | `AttemptSpec` (workspace, brief, credentials) | `AttemptResult` (commits, exit code, events, cost) | the `AgentAdapter` trait, implemented in the driver |

Requirements on the surface:

1. Define, Measure and Learn must be callable with no network. The one exception is a judgement call, which arrives through the adapter trait, never directly (see open Q16).
2. The library defines the store schema and the trait. It must not implement a GitHub client, launch a process or hold a credential.
3. Every function in Define and Learn, and `decide`, must be a pure function of its arguments.
4. Closed vocabularies are enums. An unknown outside value is an error value, never a default.

### 3.2 CLI (`gauge`)

No key. Reads and writes the store with a store credential, never the GitHub App key.

| Command | Does | Store |
|---|---|---|
| `gauge registry lint <dir>`, `registry schema` | Lint a registry tree; emit the JSON Schema derived from the Rust types. Exists since the first commit (Ref 18). | none |
| `gauge check` | Run Measure by hand on a change. At M2 it replaces the CI scripts. | open whether it writes a row |
| `gauge grade`, `grade verify-cases` | Grade cases; prove each fixture fails its oracle | open |
| `gauge render`, `gauge plan [--check] [--json]` | Render a target surface; plan a mint or reconcile | none |
| `gauge report` | Read per-rule, per-agent and per-system measures. Exists in the first commit; its content is not specified. Scales 2 to 5 of section 4 are read here **(proposal)** | read |
| `gauge runs` | List loops and their derived situations | read |
| `gauge events --since <seq> [--follow]` | The event stream with a cursor | read |
| `gauge run <ticket>`, `pause`, `drain`, `resume`, `abort` | Owner requests. Write request and flag rows only. The next tick acts | write |

How the owner requests an apply is **open** (Q9). Apply needs the key, so the CLI can only ask for it through a row.

### 3.3 Driver contract (`agent-driver`)

The driver is the only process that observes GitHub, mutates GitHub, launches an agent and runs the tick (D2). SP 7 sets the operation. This section sets what Gauge must define and the driver must obey.

**Tick.** One run-to-completion pass over every loop, then exit. Per loop: observe the world and the store; classify the situation against the closed table; perform the one next action, idempotently, after looking up its natural key; record an event; move on. Nothing is held between ticks. A trigger that arrives during a tick sets a flag so exactly one more follows. A failed reconcile of one loop is caught at that loop's boundary, recorded and counted, and other loops continue. This is the only place a panic may be caught.

**Observations.** Closed by source: git (branch, head, merge-base), GitHub (PR, check run on the sha, sticky comment, labels, comments), the local unit (running or not, exit status), and the store (loop, flags, requests, lease, residue, history).

**Situations.** The closed starting set is the ten rows of SP 7. Order matters: the no-progress and attempt-limit rows must take precedence over the "new attempt" row, or a looping attempt is retried forever.

| # | Situation | Action |
|---|---|---|
| 1 | Attempt running | wait |
| 2 | Attempt exited clean, branch pushed, head advanced | sense |
| 3 | Attempt failed or missing, branch pushed | new attempt from the branch with a resume brief, after backoff |
| 4 | Attempt missing, unpushed commits in its workspace | push, then as 3 |
| 5 | Findings exist for the sha and registry revision, no scorecard | score and post |
| 6 | Scorecard posted, PR open | wait for the owner |
| 7 | PR merged or closed | close the loop, write history, export |
| 8 | Same checkpoint fingerprint twice | park `no_progress` |
| 9 | Attempt budget spent | park `attempt_limit` |
| 10 | Anything else | park `unrecognized_state`, observation attached |

These ten do not yet cover the loop's own control states or the agent's declared outcomes. Without more rows, an owner's `pause` or a `flagged-for-human` outcome would park as unrecognised. The extra rows below are **(proposal)**, as the action each takes, except where marked open.

| # | Situation | Action |
|---|---|---|
| 11 | Run requested (label or CLI) and no loop exists | open the loop; start an attempt if a lease is held |
| 12 | Branch pushed, no open PR for it | open the PR (natural key: PR by head branch) |
| 13 | Pause flag set | no action |
| 14 | Drain flag set | start nothing new; let attempts finish |
| 15 | Abort requested | stop the attempt; close the loop |
| 16 | Parked, resume requested | clear the park; continue from row 3 |
| 17 | Parked | wait |
| 18 | Lease not held | idempotent observation actions only; start no attempt |
| 19 | Attempt reports `flagged-for-human` or `blocked` (SP 5) | **open** (Q14): the park reasons are a closed three and neither is one of them |

The action set is closed and derived from the table: wait, open loop, start attempt, push, open PR, sense, score, post, close, park, resume, abort. It is derived, not separately decided. Each action kind must declare a replay policy in code from the closed set safe, resend, inspect, never (Ref 13). The assignments are an M1 design item.

**Natural keys.** Look up before write. A repeated effect is a no-op.

| Effect | Natural key |
|---|---|
| Agent attempt | transient unit name `<prefix>-<loop>-<attempt>` (Ref uses `gauge-run-…`; prefix not covered by the syntax decision) |
| Scorecard check run | check run named `scorecard` on the head sha |
| Sticky comment | the marker in the comment body |
| Scoring | findings for (sha, registry revision) |
| Judgement call | the judgement key (section 6.1) |
| Park notice | (loop, reason, attempt) |
| PR | head branch **(proposal)** |

**Residue.** The small state the world cannot hold: a model call in flight, a running process (local only), timers (`due_at` rows), attempt counts, no-progress fingerprints, the judgement cache (Ref 14). What both machines need lives in the store.

**Triggers.** Only ask for a tick soon, never carry work: the CLI, timer rows, the agent exiting, GitHub (polled by default; a webhook receiver is optional), phone labels and comments.

**Posting.** The driver posts the check run `scorecard` pinned to the App, one sticky comment updated in place, and park notices (label, comment, desktop notification, event). A park is terminal until `resume` or `abort`. The driver also applies a mint plan (branch, commit with the trailer, push, PR). The CLI never does.

### 3.4 Agent adapter contract

One trait, two implementations at 1.0, used for coder and judge alike (D6, Ref 17).

| | Contract |
|---|---|
| In | Workspace: a clone of the branch with rendered instruction files. A brief on stdin or argv. Credentials per attempt, read-only. |
| Out | Commits on the branch. Exit code. Structured events where the agent offers them. Tokens and dollars. |
| Claude Code | `claude -p`, JSON output. `CLAUDE.md` is a one-line pointer to `AGENTS.md`. |
| Codex | `codex exec`, JSON event stream. The parser lives inside the adapter. Version pinned in the box image. |
| Judge | Same trait, role review or verify. May be another model family than the coder. |
| Not in the contract | Session resume, mid-run steering, tool-level hooks, the agent's own memory, agent telemetry. |

Requirements:

1. Nothing outside the contract may be relied on. Agent memory is off inside the box. A fresh agent must be able to continue from the brief, the branch and the last verdict alone.
2. The agent commits. It never pushes, comments or holds a GitHub credential. The driver pushes at every iteration boundary, so the branch is the checkpoint.
3. Exposure is the render set mounted, by hash, never agent telemetry.
4. Each attempt has a hard runtime bound (Ref 15).

### 3.5 Store schema (entities)

Entities, not columns. Where an entity lives matters more than its shape: some state must be in git or in the world, not in the store.

| Entity | Holds | Lives in | Written by |
|---|---|---|---|
| Loop | One Work Stream Loop: ticket or epic, repo, PR, state | store | driver |
| Request | Owner and delegated asks: run, resume, abort | store | CLI, Insights, Worker, driver (from GitHub labels and comments) |
| Flag | Pause and drain, per loop or global | store | CLI, Insights |
| Lease | Per loop: holder, expiry. Compare-and-swap | store | driver |
| Attempt | Count, unit key, checkpoint fingerprint | store | driver |
| Timer, inbox | `due_at` rows (backoff, CI waits, scheduled jobs); webhook and delegate rows | store | driver; Worker |
| Event | Sequence-numbered record of each action | store | driver |
| Judgement record | Key, output, tokens, dollars | store | driver |
| History row | Section 3.6 | store, exported to git | driver |
| Label | Useful or not useful, category, keyed by fingerprint | store | recorded by driver; given by the owner |
| Exception | Rule, fingerprint, category, reason, author; lifecycle added, confirmed, stale, removed (open Q-D1) | store | driver |
| Transition | Rule, from to, criterion, provenance, applied or proposed | store; applied edge written to the registry by PR | driver |
| Render record | Target, inputs (revision, statistics), budget, hash | store | driver |
| Grade, cluster candidate | Lab and mining output | store | driver or CLI |
| Registry (rules, patterns, profiles, glossary) | The set point | git, `agent-registry` | owner by PR |
| Lockfile | Revision, resolved chain, variables, per-path class and hash | git, target repo `.rsb/governance.lock` | driver at apply |
| Baseline and ratchet | Per package, per rule counts, one line per entry | git | **open** (Q19) |
| Cases, JSONL export | Lab cases (location proposed in Ref 10); history rows | git | owner; record step |
| Branch, PR, check run, comment, labels | Run state | the world (GitHub) | driver, owner |

The Worker's store credential should be limited to inserting inbox and request rows **(proposal)**.

### 3.6 History row

Decided shape (Ref 08): one row per (run, repo, sha, pr, rule_id). Fields:

| Group | Fields |
|---|---|
| Identity | run, repo, sha, pr, rule_id |
| Rule at the run's revision | class, level |
| Counts | count_new, count_total, count_suppressed (by category) |
| Size and correctness | lines_changed, coverage_patch, tests_pass |
| Outcome | verdict, risk_tier, signals[], termination_reason |
| Cost | judgement_tokens, judgement_usd |
| Agent | agent, model, version |
| Time | timestamp |

Two gaps, closed here by **(proposal)**.

- **Run-level facts need a home.** Verdict, tier, exit reason, cost and agent repeat on every rule row, and a run that exits early (`blocked:secrets_in_diff`) has no rule rows at all, yet its exit must be recorded. A run-level record keyed by run, beside the per-rule rows, solves both. Whether the Ref intends one is **open** (Q3).
- **Latency cannot be computed from one timestamp.** SP 8 sets p50 and p95 targets from push to verdict. The run record needs the push time and the verdict-posted time **(proposal)**. Mutation score, flaky count and holdout divergence are correctness rows in the scorecard (SP 5 part 4) and are not in the row shape either.

### 3.7 Events with cursor

Every action the driver performs records one event (SP 7). Events carry a monotonically increasing sequence. `gauge events --since <seq> --follow` and Insights read by cursor. A reader must be able to resume from the last sequence it saw.

Events are append-only, and their kinds are a closed set derived from the action set plus parks. Monotonicity under two writing machines is **open** (Q8).

---

## 4. Measurement

The measurand is the distance between the code as it is and the code as the rules say it should be (SP 8). Gauge reads it at five scales. The per-rule scale is the instrument: Gauge measuring its own gauge. It drives lifecycle.

| Scale | Measured | Reference | Computed by |
|---|---|---|---|
| Per change | Per-rule new, total, suppressed counts; correctness results; classification; cost; exit reason | The baseline at the merge-base; the rules at the pinned revision | R04 to R10 |
| Per codebase over time | The baseline and its ratchet; governance drift; the trend across merges | Ratchet counts; the lockfile | R03, R09, R11 |
| Per rule (the instrument) | Not-useful rate with bounds; fix rate; suppression rate by category; exposure; cost per useful finding; recall on seeded cases; classifier misses | Owner labels; behavioural resolution; the Lab's oracles | R11, R13 |
| Per agent | The per-rule measures stratified by agent, model and version; parks; no-progress exits | Other agents on the same rules and cases | R11, R13 |
| Per system | Share of PRs merged without opening the diff; judgement spend per merged PR; verdict latency p50 and p95 | G0 and the D3 band (section 6.5) | R11, R16 |

**Traceability rule (SP 8).** A number that cannot be traced to one of these scales and a reference does not belong in the scorecard. Gauge must enforce it by construction: a scorecard row has no representation without a scale and a reference **(proposal)**.

- A rule that fires on a change the classifier did not select it for is a recorded classifier miss (Ref 07). It belongs to the instrument scale.
- The research gives Wilson-interval thresholds: at least 16 labels per rule before the controller acts; comment-only under a 20% upper bound; blocking under 10% (35 clean labels, or 53 with one miss). These are research values. Promotion criteria are open (SP 12), so every Transition must state its criterion and none may be hard-coded silently.
- The first indicator of G0, merging without opening the diff, has no sensor today (Q4).

---

## 5. Adapters

An adapter turns a tool's output into findings or correctness rows. The adapter set differs by kind and is the only thing that should (SP 4, G6). Cells say what the sources name. "Open" means the sources do not say, and no tool has been invented.

| Concern | cloudflare (gordana-milan; TypeScript on Workers) | rust-linux (Lumen; Rust desktop) | k8s (Rollout; the owner's feature-flag platform) |
|---|---|---|---|
| Profile chain | base, service, cloudflare, gordana-milan | base, desktop, rust-linux, lumen | base, service, k8s, rollout. Minted fresh after January (Handoff) |
| Formatter | not named | rustfmt | open |
| Linter, with a table rendered from the registry | eslint | clippy | open |
| Layer and graph checks | dependency-cruiser | `xtask layers` | open |
| Error-handling check (errors as values) | first candidate base rule; check not yet chosen | `xtask errors`; `catch_unwind` only at the dispatch boundary | open |
| ast-grep and heuristics | ast-grep. A tree-sitter scanner and jscpd are in the adapter set; which app uses them is not fixed | not named | open |
| Unit and integration tests | one hermetic vitest gate on the real local Workers runtime | `cargo test`; `cargo nextest` on affected crates in the inner loop | open |
| End to end | sharded playwright | not listed | open |
| Mutation on changed lines | tool not chosen | `cargo-mutants` | open |
| Patch coverage (20-line guard), holdout divergence, flaky count | tool not named | tool not named | open |
| Toolchain in the box | TypeScript on the Workers runtime | Rust toolchain | open |
| Local run shape | real local Workers runtime | desktop; headless engine with a thin UI | open. Rollout's stack and local run shape are not yet stated |

Requirements:

1. Each adapter must normalise to the finding shape: rule id, class, fingerprint, cause, typed evidence. An adapter that emits no rule id emits no finding. Every finding cites a rule (Ref 07).
2. A rule written once must render into the lint table, the agent instruction files and the review brief without rewriting (SP 4, G6). The lint table is never a hand-written copy.
3. The fingerprint is rule, normalised text and enclosing symbol, stable across line moves. Labels attach to it, and the same fingerprint on two iterations is the no-progress signal.
4. A new kind adds adapters and a profile chain. It must not change Measure, Learn or Operate. The k8s column is the test of that claim, and it stays open until Rollout's stack is stated.
5. The agent's inner loop is the mechanical checks plus hermetic tests, within the 30 s and 90 s band (SP 8).

---

## 6. Non-functional requirements

### 6.1 Determinism and replay

State is decoupled from execution (SP 6). The one non-deterministic input is the model call.

1. Define, Learn and everything downstream of the model call must be replayable from recorded inputs. Classification is a function of (snapshot, registry revision). The verdict is arithmetic.
2. A judgement output must be recorded before anything downstream uses it, keyed by the invocation key. The Ref defines the key as hash(snapshot, brief) (Ref 07, 13). A repeat reads the record and spends nothing, and a crashed tick that already paid reads it back. Whether the role, the lens and the model are inside the brief hash is **open** (Q2).
3. The Lab must regrade historical runs against new rules without spending tokens, for rules whose evidence is already recorded. A new judgement lens has a new brief and needs a new call.
4. A render's output hash is the brief hash on every finding it produced. Its statistics input must be reproducible (Q15).
5. Every start is a resume. Gauge has no recovery code path distinct from the tick.

### 6.2 Kill test **(proposal, carried)**

SP 7 and 10 propose it as a release test. Gauge must provide it. For each tick and each agent attempt, kill the process at a random point, restart with only the store and the world, and the loop must reach the same end state. Gauge's elaboration of what is asserted is **(proposal)**.

- No second spend on a judgement whose output was already recorded.
- No duplicate check run, comment, label, PR or park notice.
- No attempt started without a held lease (from M5), and an unclassifiable loop parks.

The Handoff puts "kill test passes" at M1, before leases exist (M5). The M1 test is single machine.

### 6.3 Failure semantics

1. **Fails closed.** Every exit from the pipeline is one named reason in the history row. A run that stops early is data, never a pass. Malformed judgement output is `blocked:malformed_judgement`, never read as "no findings". A finding at verification is RED, never a pass. An unused suppression is `blocked:unused_suppression`.
2. **The complement of the list is a stop.** The closed vocabularies are below. Anything outside parks with the observation attached. "Cannot safely resume" is the complement of the situation table, never a judgement made at recovery time.
3. **Store unreachable: no lease, no action.** An outage pauses the system (D5).
4. **Errors are values.** No library function throws or panics. `catch_unwind` appears once, at the dispatch boundary. Lumen is the reference for Gauge's own Rust conventions (SP 3).
5. **Backoff and hysteresis** are required (SP 6). Parameters are open (Q13).

| Vocabulary | Members (from the sources) |
|---|---|
| Signals | code, test, docs, generated, policy, interface, state, type, removal, layering. Ref adds `comment`. Final set open (Q6) |
| Check class, risk tier | mechanical, heuristic, judgement; simple, medium, complex |
| Lifecycle | draft, trial, enforced-comment, enforced-blocking, retired |
| Cause | introduced, worsened, stale, missing-required |
| Evidence | direct, spec, policy, test, validation, missing, inferred |
| Adjudication | accepted, deferred, rejected |
| Exception category | false-positive, convention, test/generated, debt |
| Verdict | RED, AMBER, GREEN |
| Exit reasons | `blocked:secrets_in_diff`, `blocked:unused_suppression`, `blocked:malformed_judgement`, a verification finding (RED), and a verdict. List not complete in the sources |
| Park reasons | `no_progress`, `attempt_limit`, `unrecognized_state` |
| Agent outcome | pr-opened, flagged-for-human, blocked with a stated reason |
| Ownership class, plan outcome | owned, pointer, merged, sample (last two undefined: SP 12); unchanged, update, merge, conflict, drift |

### 6.4 Custody

1. The GitHub App key must exist in exactly one binary, `agent-driver`. `rsb-gauge`, `rsb-gauge-cli` and `insights` must not link a GitHub client. A layering rule checks it, and it runs on Gauge itself (D2).
2. The agent never holds the key, never pushes and cannot widen its limits (SP 4, G5). The box holds no store credential. Holdout tests are never mounted in the box. Secrets are refused at the snapshot.
3. Every new tool on the agent's path extends the guard tests (SP 4, G5). Governance drift in a target is counted as findings (D1).

### 6.5 Latency band (D3)

Targets, measured from the first PR, reviewed after 30 merged PRs. The band spans everything between push and verdict. Gauge's own compute is a part of it.

| Measure | p50 | p95 |
|---|---|---|
| Any change, push to verdict | 5 min or less | 10 min or less (15 tolerable) |
| Small change, push to verdict | 3 min or less | 5 min or less |
| No-op report | within 90 s | |
| Agent inner loop | 30 s or less | 90 s or less |
| Post-merge full run | 30 min or less | |

Gauge must record enough to compute these (section 3.6). The sources give no budget for Gauge's own share.

The required check `scorecard` must have no path filter and no job-level `if`, since a skipped required job counts as passing (Handoff). Gauge must measure the browser end-to-end share of p95 so the A to B trigger (above 50%, D3) can be read from data.

### 6.6 Interaction of rule state and verdict

The Ref writes verdict rules by class and keeps lifecycle states separate. How they combine is not in the sources (Q5). The Handoff default is that a `trial` rule reports and does not decide the colour on gordana-milan. An imported draft is rendered and enforced nowhere (D4), so Gauge must not run one against any repo.

---

## 7. Milestones

Milestones follow the Handoff (M0 to M7). Weeks are the Handoff's sizes and are proposals. Each milestone is adopted on gordana-milan the day it lands, as its acceptance, except where noted. Gauge must not put gordana-milan's 2027-01-01 deadline at risk (SP 1). M3 may slip past January. M4 onward follows it.

| Milestone | Gauge must deliver | Responsibilities | Acceptance |
|---|---|---|---|
| **M0 Align** (this week) | Nothing built. A hand-run loop that leaves the world-state Gauge will later adopt: branch, PR, `scorecard` check, sticky comment, labels, a history row (SP 10 "hand-run loop"). The verdict is read before the diff and measurement starts | none | gordana-milan |
| **M1 Driver's thin loop** (weeks 2 to 3) | `agent-driver` tick from issue label to posted verdict with CI as the sensor. Store on Supabase. Reconciler core, the situation table as the starting set, natural keys for posting, the Claude adapter, requests and flags, events. Kill test passes (single machine) | R15, R16 (Claude), R17 (`run`, `pause`, `drain`, `resume`, `abort`, `runs`, `events`), R18 (events), Record subset | gordana-milan |
| **M2 Measure** (weeks 4 to 6) | Registry parser and linter, snapshot, classifier, cloudflare adapters, scorecard arithmetic with baseline and ratchet, history schema, `gauge check` replacing the CI scripts. Three Lab cases graded, each shown to fail its oracle. The first real number: recall of the mechanical tier on known slop (SP 10) | R01, R04, R05, R06, R09, R10, R13 (verify and grade), exceptions from R08 **(proposal)** | gordana-milan |
| **M3 Define** (weeks 7 to 9; may slip past January) | Renders with hashes and the directive budget. Profiles, `gauge plan`, the lockfile. Drift as findings. Apply in the driver. Lumen minted from `base, desktop, rust-linux, lumen` | R02, R03 (plan and apply) | Lumen is the first minted repo; gordana-milan adopts renders |
| **M4 Learn** (after January) | Labels, statistics, lifecycle, regenerated renders, judgement lenses, adjudication and verification. Codex adapter by M4 **(proposal)** so per-agent statistics are meaningful from the start | R07, R08 (rest), R11, R12, R16 (Codex) | gordana-milan |
| **M5 Two machines** | Leases, takeover, the Worker, `/rsb-workflow` commands and the `rsb-workflow:run` label. Kill test extended to leases | R15 (lease), Record leases, R17 | both machines; delegation from the phone |
| **M6 Rollout** | The k8s column filled once Rollout's stack is stated. Profile `base, service, k8s, rollout`. Rollout minted | R03, R06 (k8s adapters) | Rollout |
| **M7 Importer, mining, Insights** | The importer (D4), mining, Insights last | R01 (importer), R14, R18 (app) | n/a |

SP 12 asks where the reconciler, leases and store fall in the order. The Handoff answers: reconciler and store at M1, leases at M5.

Flags:

- The heuristic tier has no milestone in the Handoff (Q11). Gauge 1.0 needs at least one heuristic rule graded (below).
- The ten mechanical rules needed at M2 cannot come from the M7 importer. gordana-milan's registered rules are the likely source (Q12).
- **M1 cutover of the required check.** Since M0 (2026-10-24), gordana-milan's `main` ruleset (id 25499646) requires `scorecard` with `integration_id` 15368, GitHub Actions, so only Actions can satisfy it. When `agent-driver` starts posting `scorecard` as the GitHub App, the ruleset's `integration_id` must switch to the App's id in the same change, and the Actions job must stop reporting under that name; otherwise every PR is blocked (wrong source) or two `scorecard` checks compete. Sequence: driver posts as the App on a branch; confirm its `app.id` on a check run; PUT the ruleset with the App's id; remove the Actions `scorecard` job. Rollback: PUT the Actions id back.
- M0 has already registered twelve rules (`rl-0001` to `rl-0012`, eight pointing into gordana-milan's `engineering/`, four containment), checked in CI by hand-written scripts. They are M2's starting set for `gauge check` and the first answer to Q12.
- SP 10's "Milestone 1: the skeleton" (classifier, about ten mechanical rules, scorecard, history rows, three seeded cases) is inside M2. Its "Milestone 2: the hand-run loop" is M0.

### 7.1 Gauge 1.0 **(proposal, carried)**

From SP 10. Gauge is 1.0 when all of these hold:

- Both kinds run end to end through the loop, Lumen and gordana-milan, with merged PRs.
- Claude Code and Codex both complete tickets through the one adapter contract.
- Ticks run under systemd on both machines with leases, and the kill test passes.
- The `scorecard` check, sticky comment and history rows are produced for every PR.
- Rule lifecycle runs, with owner-approved promotion to blocking.
- The Lab grades the mechanical tier and at least one heuristic rule on seeded cases.
- The D3 latency band has been reviewed after 30 merged PRs.
- Insights is not required.

Mapping **(proposal)**: these are met at the end of M5. M6 (k8s) and M7 fall after 1.0, since SP names only Lumen and gordana-milan. Two additions **(proposal)**: every closed vocabulary in section 6.3 has a test that its complement parks or blocks; the custody layering rule runs in CI on `gauge` and passes.

---

## 8. Open questions

### 8.1 Carried from SP 12 and the Handoff

| ID | Question | Leaning | Gauge impact |
|---|---|---|---|
| Q-D1 | Is Exception its own aggregate root? | Yes, with a lifecycle of added, confirmed, stale, removed. It outlives the run that first saw it | Shapes the store (section 3.5) and R08 |
| Q-D2 | Who owns Render: the Controller as output, or the Registry as projection? | Unresolved. It is the join point, with two inputs and two consumers. D1 fixes it as the join between Governance and the rest | Which crate owns R02 |
| Q-D3 | Does Classification live on the Change or the Run? | Change, as a pure function of snapshot and revision. It moves to Run if reclassifying after registry changes is common | R05, history |
| Q-D4 | What do the `merged` and `sample` ownership classes mean? | Open. Owned and pointer are clear | R03 cannot plan these two |
| Q-D5 | Who retires a rule: owner, lifecycle or sunset? | Open. Demotion is automatic | R12 |
| Q-D6 | What are the promotion criteria other than to blocking? | Open | R12 |
| Q-C1 | Tick cadence (about 5 minutes), night wake, lease TTL (15 minutes) and takeover delay (one cadence): all **(proposal, carried)** | Open | M1, M5 |
| Q-C3 | Codex inside the box: whether its own sandbox must be disabled, and how its version is pinned | Open. Verify against the pinned version | R16 |
| Q-C4 | Owner adoption of latency bands beyond p50 5 min and p95 10 min | Open | Section 6.5 |
| Q-C6 | Exact label and command syntax | `rsb-workflow:run`, `/rsb-workflow …` **(proposal, carried)** | M5 |
| Q-C7 | Rollout's stack and local run shape | Open | Section 5 k8s column |

Settled leanings: a Case belongs to the Lab; a judgement invocation belongs to Measure.

### 8.2 Found while drafting

| ID | Question | Why it matters |
|---|---|---|
| Q1 | Measure is "pure except the recorded model call", but adapter output is not: tests flake and tools differ by version. Is it recorded as an input? | Whether the regrade promise (R13) holds for correctness rows |
| Q2 | The judgement key is hash(snapshot, brief). Are role, lens and model inside the brief hash? | A different model or lens on the same snapshot could collide with a cached output (D6 allows a different judge) |
| Q3 | Is there a run-level record beside the per-rule rows? | Early exits have no rule rows (section 3.6) |
| Q4 | What produces "share of PRs merged without opening the diff"? GitHub exposes no such signal (research) | G0's first indicator has no sensor |
| Q5 | How do `default_level` and lifecycle state combine with check class in the verdict? What does `enforced-comment` do? | R09 cannot be completed without it. The level values are not listed in Ref |
| Q6 | Is the Signal enum final? Ref includes `comment`, SP does not, and the Ref list ends with an ellipsis | R05 and the closed-vocabulary test |
| Q7 | Where does a heuristic rule's allowance live? The rule file has no such field, and a project-level override never lives in the rule file | AMBER cannot be computed without it. The profile is the likely home |
| Q8 | Can the event sequence stay monotonic for readers when two machines write? | A cursor reader can skip rows that commit out of order |
| Q9 | How does the owner request a mint apply or a repo creation? The CLI has no key | Needs a request row type, or a command that goes through the driver |
| Q10 | Where does the Worker's code live, and under which store role? | It is not a listed package |
| Q11 | Which milestone delivers heuristic checks? | Needed before 1.0 |
| Q12 | The ten mechanical rules needed for M2 and the D4 trial in the Lab predate the M7 importer. How do they reach the registry? | M2 acceptance |
| Q13 | Backoff schedule and hysteresis rules | Without them level-triggered loops flap |
| Q14 | Where do the agent outcomes `flagged-for-human` and `blocked` land? They match no row and no park reason | Row 19 of the situation table |
| Q15 | How is a render's statistics input pinned? Labels arrive late and change windows | The render hash is not reproducible otherwise |
| Q16 | A key-less `gauge check` with judgement lenses: who launches the model, since process launch lives in the driver (D2)? | Hand runs of the full pipeline |
| Q17 | Does the Claude adapter use `--bare`? The research recommends it for scripted calls but it changes which instruction files load | Which renders the agent sees |
| Q18 | How does the owner label a finding from the phone? | R11 needs labels (SP 3) |
| Q19 | Who writes the committed baseline and ratchet counts, given `main` requires a PR? Does a hand `gauge check` write history? | The ratchet moves by a commit someone must make |

---

## 9. Glossary deltas

Terms not in SP section 13.

- **Record.** The layer under the four groups: store schema, history, events, judgement cache, leases, JSONL export.
- **Adapter.** Either a sensor adapter (eslint, clippy, xtask and so on) or the agent adapter (the trait over `claude -p` and `codex exec`).
- **Signal.** A member of the closed enum the classifier derives from a snapshot.
- **Risk tier.** Simple, medium or complex. Selects judgement lenses and whether verification is required.
- **Classification.** The signals and tier for a snapshot, a pure function of snapshot and registry revision.
- **Fingerprint.** Rule, normalised text and enclosing symbol. A finding's stable identity.
- **Cause, evidence.** Typed fields on a finding that drive adjudication in code.
- **Lens.** A scoped review question a judgement invocation answers.
- **Coverage attestation.** A judge's per-check statement of pass, finding or not-applicable, with evidence.
- **Adjudication.** Accept, defer or reject a finding, computed in code.
- **Verify.** A judgement pass that confirms the negative on a clear complex-tier change.
- **Label.** The owner's useful or not-useful mark on a finding, with a category.
- **Behavioural resolution.** A finding gone at the next snapshot without a suppression.
- **Transition.** A recorded lifecycle edge with criterion and provenance, applied or proposed.
- **Holdout.** Hidden tests mounted only at grade time from a path the box never sees.
- **Situation.** One entry in the closed table a tick classifies the observed world into.
- **Attempt.** One run of the coding agent in a loop: a transient unit with its own workspace.
- **Checkpoint.** The pushed branch. A fresh attempt resumes from it.
- **Judgement key.** The key under which a judgement output is recorded. Defined as hash(snapshot, brief).
- **Replay policy.** Per action kind, one of safe, resend, inspect, never.
