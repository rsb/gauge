//! The kill harness and the tick's tests. A scripted world runs through
//! ticks against the fakes; a seeded kill stops a tick between any two port
//! calls, store or GitHub; the tick restarts from the store and the fake
//! world alone; and the end state must equal the run with no kill, with no
//! event repeated. Later tickets add their rows' scenarios to [`scenarios`].

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use rsb_gauge_reconcile::adapter::RunOutput;
use rsb_gauge_reconcile::store::{
    Agent, Applied, AttemptCause, Event, EventKind, Flag, FlagKind, LabelKey, Loop, LoopId,
    LoopState, NewRequest, RequestId, SkillsSource, Snapshot, Store, Write,
};
use rsb_gauge_reconcile::{
    Action, MemoryStore, ParkReason, Refusal, Situation, TicketCause, UnitSeen,
};
use rsb_gauge_schema::Repos;
use rsb_gauge_store::SqliteStore;

use crate::attempt::{Attempts, Launch, Settings, Workspace};
use crate::error::Error;
use crate::fake::{FakeAttempts, FakeGitHub, SHA};
use crate::registry::{Registry, Source};
use crate::tick::{self, Clock, Mode, Outcome, Poll, Polled, Report};
use crate::world::{self, Branch, Entry, GitHub, IssueSeen, LABEL, Labelled};

const NOW: i64 = 1_793_000_000;

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> i64 {
        NOW
    }
}

/// A clock set by the script, for the runtime bound.
struct At(i64);

impl Clock for At {
    fn now(&self) -> i64 {
        self.0
    }
}

const REPOS: &str = r#"
schema = 1

[[repo]]
name = "ref-rsb-serverless-stack"
role = "reference"
kind = "service"
profile = "rsb-serverless-stack"
path = "ref/ref-rsb-serverless-stack"
status = "planned"

[[repo]]
name = "lumen"
role = "project"
kind = "desktop"
path = "projects/desktop/lumen"
status = "active"

[[repo]]
name = "libraw"
role = "project"
kind = "lib"
path = "projects/libraw"
status = "active"

[[repo]]
name = "gordana-milan"
role = "project"
kind = "service"
path = "projects/gordana-milan"
status = "active"

[[repo]]
name = "gauge"
role = "factory"
path = "factory/gauge"
status = "active"

[[repo]]
name = "research"
role = "record"
path = "research"
status = "active"
"#;

fn registry() -> Result<Registry, String> {
    let path = Path::new("/registry/repos.toml");
    Ok(Registry {
        source: Source {
            path: path.to_path_buf(),
            sha: Some("0123456789abcdef0123456789abcdef01234567".into()),
        },
        repos: Repos::parse(REPOS, path).map_err(|e| e.to_string())?,
    })
}

/// The ticket every scripted issue names, unless it names none.
const TICKET: &str = "specs/S-0001-thin-loop/01-first-ticket.md";
const MARKER: &str = "<!-- rsb-ticket S-0001/1 -->\n\n# The first ticket\n";

/// Everything outside the store a tick sees: GitHub, the attempts' units
/// and workspaces, the clock, and the driver's attempt settings.
#[derive(Clone)]
struct World {
    github: FakeGitHub,
    attempts: FakeAttempts,
    now: i64,
    settings: Result<Settings, String>,
}

impl std::ops::Deref for World {
    type Target = FakeGitHub;
    fn deref(&self) -> &FakeGitHub {
        &self.github
    }
}

impl std::ops::DerefMut for World {
    fn deref_mut(&mut self) -> &mut FakeGitHub {
        &mut self.github
    }
}

fn world() -> World {
    World {
        github: github(),
        attempts: FakeAttempts::new(),
        now: NOW,
        settings: Ok(crate::attempt::tests::settings()),
    }
}

/// The fake GitHub: the reference repo and `lumen` minted, with the lockfile
/// and the ticket file; `libraw` eligible by role but with no lockfile; and
/// `gordana-milan`, `gauge` and `research` not reached. Issues 1 to 20 name
/// the ticket, but the reference repo's issue 13 names none.
fn github() -> FakeGitHub {
    let issues = |mut r: crate::fake::FakeRepo| {
        for n in 1..=20 {
            r = r.issue(n, MARKER);
        }
        r
    };
    FakeGitHub::new()
        .repo("ref-rsb-serverless-stack", |r| {
            issues(r.file(TICKET)).issue(13, "A bug report, no ticket.")
        })
        .repo("lumen", |r| issues(r.file(TICKET)))
        .repo("libraw", |r| issues(r.file(TICKET)).without_lockfile())
}

fn ask(store: &mut impl Store, repo: &str, issue: u64) -> RequestId {
    store
        .request(
            &NewRequest {
                repo: repo.into(),
                issue,
                agent: Agent::Claude,
            },
            NOW,
        )
        .unwrap()
}

/// One tick against the scripted world with no label on any issue.
fn tick(store: &mut impl Store, mode: Mode) -> Report {
    tick_in(store, &mut world(), mode)
}

fn tick_in(store: &mut impl Store, world: &mut World, mode: Mode) -> Report {
    tick::run(
        store,
        &mut world.github,
        &mut world.attempts,
        &registry(),
        &world.settings,
        &At(world.now),
        mode,
    )
    .unwrap()
}

/// A gated tick's result, whether it was killed, and its calls by name.
type Gated<E> = (Result<Report, Killed<E>>, bool, Vec<&'static str>);

/// One tick behind a gate that kills at call `kill_at`. Returns the tick's
/// result, whether it was killed, and every port call it made, by name.
fn gated<S: Store>(store: &mut S, world: &mut World, kill_at: Option<usize>) -> Gated<S::Error> {
    let gate = Gate::new(kill_at);
    let result = tick::run(
        &mut Killable {
            inner: store,
            gate: &gate,
        },
        &mut KillableGitHub {
            inner: &mut world.github,
            gate: &gate,
        },
        &mut KillableAttempts {
            inner: &mut world.attempts,
            gate: &gate,
        },
        &registry(),
        &world.settings,
        &At(world.now),
        Mode::Act,
    );
    let killed = gate.killed();
    (result, killed, gate.log.into_inner())
}

fn events(store: &impl Store) -> Vec<Event> {
    store.events(0, 1000).unwrap()
}

/// Everything a store holds that a tick can change. Events are compared as a
/// set without their sequence: a restart decides the issues a killed tick
/// did not reach in another interleaving, so the same events can land in
/// another order. Their sequence still only rises. A park's observation is
/// left out: a restart is one more tick, so a park can see the world before
/// a later owner write rather than after it.
fn end_state(store: &impl Store) -> (Vec<Loop>, Vec<String>, Snapshot) {
    let events = events(store);
    assert!(
        events.windows(2).all(|w| w[0].seq < w[1].seq),
        "event sequence does not rise"
    );
    let mut seen: Vec<String> = events
        .iter()
        .map(|e| {
            let mut payload = e.payload.clone();
            if let Some(map) = payload.as_object_mut() {
                map.remove("observation");
            }
            format!(
                "{} {} {:?} {:?} {:?} {}",
                e.at, e.kind, e.repo, e.issue, e.loop_id, payload
            )
        })
        .collect();
    seen.sort();
    (store.loops().unwrap(), seen, store.snapshot().unwrap())
}

// ---------------------------------------------------------------------------
// The kill harness.

/// The count of port calls in one tick, across the store and GitHub, and
/// the one to die at: that call and every later one fail before reaching the
/// port, as if the process had been killed between the previous call and
/// this one.
struct Gate {
    calls: Cell<usize>,
    kill_at: Option<usize>,
    /// Every call by name, so a named kill point can be found.
    log: RefCell<Vec<&'static str>>,
}

impl Gate {
    fn new(kill_at: Option<usize>) -> Self {
        Self {
            calls: Cell::new(0),
            kill_at,
            log: RefCell::new(Vec::new()),
        }
    }

    fn pass(&self, what: &'static str) -> bool {
        let n = self.calls.get();
        self.calls.set(n + 1);
        self.log.borrow_mut().push(what);
        !self.kill_at.is_some_and(|k| n >= k)
    }

    fn killed(&self) -> bool {
        self.kill_at.is_some_and(|k| self.calls.get() > k)
    }
}

/// A store behind the gate.
struct Killable<'a, S> {
    inner: &'a mut S,
    gate: &'a Gate,
}

#[derive(Debug, thiserror::Error)]
enum Killed<E: std::error::Error + 'static> {
    #[error("killed")]
    Killed,
    #[error("store")]
    Store(#[source] E),
}

impl<S: Store> Killable<'_, S> {
    fn gate(&self) -> Result<(), Killed<S::Error>> {
        if self.gate.pass("store") {
            Ok(())
        } else {
            Err(Killed::Killed)
        }
    }
}

impl<S: Store> Store for Killable<'_, S> {
    type Error = Killed<S::Error>;

    fn snapshot(&self) -> Result<Snapshot, Self::Error> {
        self.gate()?;
        self.inner.snapshot().map_err(Killed::Store)
    }

    fn trigger(&self) -> Result<u64, Self::Error> {
        self.gate()?;
        self.inner.trigger().map_err(Killed::Store)
    }

    fn write(
        &mut self,
        write: &Write,
        at: i64,
        payload: &serde_json::Value,
    ) -> Result<Applied, Self::Error> {
        self.gate()?;
        self.inner.write(write, at, payload).map_err(Killed::Store)
    }

    fn request(&mut self, request: &NewRequest, at: i64) -> Result<RequestId, Self::Error> {
        self.gate()?;
        self.inner.request(request, at).map_err(Killed::Store)
    }

    fn label_request(
        &mut self,
        key: &LabelKey,
        agent: Agent,
        at: i64,
    ) -> Result<Option<RequestId>, Self::Error> {
        self.gate()?;
        self.inner
            .label_request(key, agent, at)
            .map_err(Killed::Store)
    }

    fn label_requested(&self, key: &LabelKey) -> Result<bool, Self::Error> {
        self.gate()?;
        self.inner.label_requested(key).map_err(Killed::Store)
    }

    fn flag(&mut self, flag: Flag, at: i64) -> Result<bool, Self::Error> {
        self.gate()?;
        self.inner.flag(flag, at).map_err(Killed::Store)
    }

    fn loops(&self) -> Result<Vec<Loop>, Self::Error> {
        self.gate()?;
        self.inner.loops().map_err(Killed::Store)
    }

    fn events(&self, since: u64, limit: usize) -> Result<Vec<Event>, Self::Error> {
        self.gate()?;
        self.inner.events(since, limit).map_err(Killed::Store)
    }
}

/// GitHub behind the same gate as the store.
struct KillableGitHub<'a, G> {
    inner: &'a mut G,
    gate: &'a Gate,
}

impl<G: GitHub> KillableGitHub<'_, G> {
    fn gate(&self) -> Result<(), Error> {
        if self.gate.pass("github") {
            Ok(())
        } else {
            Err(Error::Failed("killed".to_owned()))
        }
    }
}

impl<G: GitHub> GitHub for KillableGitHub<'_, G> {
    fn reaches(&self, repo: &str) -> bool {
        self.inner.reaches(repo)
    }

    fn labelled(&mut self, repo: &str, label: &str) -> Result<Vec<Labelled>, Error> {
        self.gate()?;
        self.inner.labelled(repo, label)
    }

    fn issue(&mut self, repo: &str, number: u64) -> Result<Option<IssueSeen>, Error> {
        self.gate()?;
        self.inner.issue(repo, number)
    }

    fn default_branch(&mut self, repo: &str) -> Result<Branch, Error> {
        self.gate()?;
        self.inner.default_branch(repo)
    }

    fn list(&mut self, repo: &str, sha: &str, dir: &str) -> Result<Option<Vec<Entry>>, Error> {
        self.gate()?;
        self.inner.list(repo, sha, dir)
    }

    fn read(&mut self, repo: &str, sha: &str, path: &str) -> Result<Option<Vec<u8>>, Error> {
        self.gate()?;
        self.inner.read(repo, sha, path)
    }

    fn clone_at(&mut self, repo: &str, sha: &str, into: &Path) -> Result<(), Error> {
        self.gate()?;
        self.inner.clone_at(repo, sha, into)
    }
}

/// The attempts port behind the same gate.
struct KillableAttempts<'a, A> {
    inner: &'a mut A,
    gate: &'a Gate,
}

impl<A: Attempts> KillableAttempts<'_, A> {
    fn gate(&self, what: &'static str) -> Result<(), Error> {
        if self.gate.pass(what) {
            Ok(())
        } else {
            Err(Error::Failed("killed".to_owned()))
        }
    }
}

impl<A: Attempts> Attempts for KillableAttempts<'_, A> {
    fn dir(&self, loop_id: LoopId, attempt: u32) -> PathBuf {
        self.inner.dir(loop_id, attempt)
    }

    fn unit(&mut self, name: &str) -> Result<UnitSeen, Error> {
        self.gate("unit")?;
        self.inner.unit(name)
    }

    fn start(&mut self, launch: &Launch) -> Result<(), Error> {
        self.gate("start")?;
        self.inner.start(launch)
    }

    fn stop(&mut self, name: &str) -> Result<(), Error> {
        self.gate("stop")?;
        self.inner.stop(name)
    }

    fn ticket(
        &mut self,
        github: &mut dyn GitHub,
        dir: &Path,
        ws: &Workspace<'_>,
    ) -> Result<Option<String>, Error> {
        self.gate("ticket")?;
        self.inner.ticket(github, dir, ws)
    }

    fn prepare(
        &mut self,
        github: &mut dyn GitHub,
        dir: &Path,
        ws: &Workspace<'_>,
        branch: &str,
        brief: &serde_json::Value,
    ) -> Result<(), Error> {
        self.gate("prepare")?;
        self.inner.prepare(github, dir, ws, branch, brief)
    }

    fn sealed(&mut self, dir: &Path) -> Result<bool, Error> {
        self.gate("sealed")?;
        self.inner.sealed(dir)
    }

    fn output(
        &mut self,
        dir: &Path,
        base_sha: &str,
        branch: &str,
        exit: Option<i32>,
    ) -> Result<RunOutput, Error> {
        self.gate("output")?;
        self.inner.output(dir, base_sha, branch, exit)
    }

    fn skills(&mut self, checkout: &Path) -> Result<SkillsSource, Error> {
        self.gate("skills")?;
        self.inner.skills(checkout)
    }
}

/// A seeded xorshift, so a failing seed replays exactly.
struct Seeded(u64);

impl Seeded {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// One step of a scripted world.
#[derive(Clone, Copy, Debug)]
enum Step {
    Ask(&'static str, u64),
    /// The owner applies the run label: a new label event.
    Label(&'static str, u64),
    Unlabel(&'static str, u64),
    Pause(Option<i64>),
    /// The unit exits with a code, or on a signal.
    Exit(&'static str, Option<i32>),
    /// The attempt (loop, number) seals its outbox.
    Seal(i64, u32),
    /// The clock moves on, in seconds.
    Advance(i64),
    Tick,
}

/// The scenarios, each a script of owner writes and ticks.
fn scenarios() -> Vec<(&'static str, Vec<Step>)> {
    use Step::*;
    vec![
        (
            "an eligible request opens one loop, which then starts its first attempt",
            vec![Ask("ref-rsb-serverless-stack", 7), Tick, Tick, Tick],
        ),
        (
            "requests on eligible, factory, record and unlisted repos in one tick",
            vec![
                Ask("ref-rsb-serverless-stack", 1),
                Ask("ref-rsb-serverless-stack", 1),
                Ask("gauge", 2),
                Ask("research", 3),
                Ask("nowhere", 4),
                Ask("lumen", 5),
                Tick,
                Tick,
            ],
        ),
        (
            "a global pause holds a request until the end",
            vec![Pause(None), Ask("lumen", 9), Tick, Tick],
        ),
        (
            "a second request on an issue whose loop is open is refused",
            vec![Ask("lumen", 3), Tick, Ask("lumen", 3), Tick, Tick],
        ),
        (
            "a label opens one loop; the label stays and opens nothing more",
            vec![Label("ref-rsb-serverless-stack", 11), Tick, Tick, Tick],
        ),
        (
            "a label on a repo without the lockfile is refused once",
            vec![Label("libraw", 2), Tick, Tick],
        ),
        (
            "a label on an issue naming no ticket is refused once",
            vec![Label("ref-rsb-serverless-stack", 13), Tick, Tick],
        ),
        (
            "a label and a request on one issue, and a label applied again",
            vec![
                Label("lumen", 12),
                Ask("lumen", 12),
                Tick,
                Unlabel("lumen", 12),
                Label("lumen", 12),
                Label("libraw", 4),
                Tick,
                Label("libraw", 4),
                Tick,
            ],
        ),
        (
            "labels on three repos and requests on factory and unreached ones",
            vec![
                Label("ref-rsb-serverless-stack", 1),
                Label("lumen", 1),
                Label("libraw", 1),
                Ask("gordana-milan", 1),
                Ask("gauge", 2),
                Tick,
                Tick,
            ],
        ),
        (
            "a request on an issue whose loop is parked is refused",
            vec![
                Ask("ref-rsb-serverless-stack", 8),
                Tick,
                Tick,
                Ask("ref-rsb-serverless-stack", 8),
                Ask("ref-rsb-serverless-stack", 8),
                Tick,
            ],
        ),
        (
            "a loop opens, its attempt starts, and the unit exits non-zero",
            vec![
                Ask("ref-rsb-serverless-stack", 7),
                Tick,
                Tick,
                Tick,
                Exit("gauge-run-1-1", Some(3)),
                Tick,
                Tick,
            ],
        ),
        (
            "an attempt that runs past its bound is stopped as timed out",
            vec![
                Label("lumen", 3),
                Tick,
                Tick,
                Advance(1800),
                Tick,
                Advance(1800),
                Tick,
                Tick,
            ],
        ),
        (
            "two loops' attempts: one seals its outbox and exits zero, one is killed",
            vec![
                Ask("lumen", 4),
                Ask("ref-rsb-serverless-stack", 4),
                Tick,
                Tick,
                Seal(1, 1),
                Exit("gauge-run-1-1", Some(0)),
                Exit("gauge-run-2-1", None),
                Tick,
                Tick,
            ],
        ),
    ]
}

/// Runs a script against a fresh [`world`]. At each tick, `kills` picks the
/// port call to die at, or none; a killed tick is restarted until one
/// completes, as systemd would. Settling ticks after the script let both runs
/// reach rest. Returns how many ticks were killed, how many port calls each
/// script tick made in the run that completed, and the world at the end.
fn play<S: Store>(
    store: &mut S,
    script: &[Step],
    mut kills: impl FnMut() -> Option<usize>,
) -> (usize, Vec<usize>, World) {
    let mut world = world();
    let mut killed = 0;
    let mut calls = Vec::new();
    for step in script {
        match *step {
            Step::Ask(repo, issue) => {
                ask(store, repo, issue);
            }
            Step::Label(repo, issue) => {
                world.label(repo, issue, LABEL);
            }
            Step::Unlabel(repo, issue) => {
                world.unlabel(repo, issue, LABEL);
            }
            Step::Pause(scope) => {
                store
                    .flag(
                        Flag {
                            kind: FlagKind::Pause,
                            scope: scope.map(LoopId),
                        },
                        NOW,
                    )
                    .unwrap();
            }
            Step::Exit(unit, code) => world.attempts.set(unit, UnitSeen::Exited { code }),
            Step::Seal(loop_id, attempt) => {
                let dir = world.attempts.dir(LoopId(loop_id), attempt);
                world.attempts.seal(&dir);
            }
            Step::Advance(seconds) => world.now += seconds,
            Step::Tick => loop {
                let (result, was_killed, log) = gated(store, &mut world, kills());
                if !was_killed {
                    let report = result.unwrap();
                    assert_eq!(report.failed, 0, "{report:?}");
                    calls.push(log.len());
                    break;
                }
                killed += 1;
            },
        }
    }
    for _ in 0..3 {
        tick_in(store, &mut world, Mode::Act);
    }
    (killed, calls, world)
}

/// What the attempts port holds at the end: units, starts by name, stops,
/// briefs written. A kill never starts or stops a unit twice, so the lists
/// match the run with no kill.
type AttemptsState = (
    Vec<(String, UnitSeen)>,
    Vec<String>,
    Vec<String>,
    Vec<(PathBuf, serde_json::Value)>,
);

fn attempts_state(world: &World) -> AttemptsState {
    let a = &world.attempts;
    let mut starts: Vec<String> = a.starts.iter().map(|l| l.unit.clone()).collect();
    starts.sort();
    let mut stops = a.stops.clone();
    stops.sort();
    (
        a.units.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        starts,
        stops,
        a.briefs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    )
}

/// Exactly one start per unit name, ever, and `brief_issued` recorded for
/// every unit started.
fn one_start_per_unit_each_briefed(store: &impl Store, world: &World, what: &str) {
    let mut names: Vec<&str> = world
        .attempts
        .starts
        .iter()
        .map(|l| l.unit.as_str())
        .collect();
    let all = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), all, "{what}: a unit started twice");
    let events = events(store);
    for name in names {
        assert!(
            events.iter().any(|e| e.kind == EventKind::BriefIssued
                && e.loop_id
                    .map(|id| rsb_gauge_reconcile::store::unit_name(
                        id,
                        e.payload["attempt"].as_u64().unwrap() as u32
                    ))
                    .as_deref()
                    == Some(name)),
            "{what}: {name} started with no brief_issued"
        );
    }
}

/// More port calls than any scripted tick makes, so a random kill can land
/// anywhere.
const CALLS: usize = 48;

fn no_event_repeats(events: &[String]) {
    let mut seen = std::collections::BTreeSet::new();
    for e in events {
        assert!(seen.insert(e), "event repeated: {e}");
    }
}

#[test]
fn a_kill_at_any_point_reaches_the_same_end_state_on_the_fake() {
    for (name, script) in scenarios() {
        let mut clean = MemoryStore::new();
        let (_, calls, clean_world) = play(&mut clean, &script, || None);
        assert!(calls.iter().all(|&c| c < CALLS), "{name}: {calls:?}");
        let expected = end_state(&clean);
        no_event_repeats(&expected.1);
        one_start_per_unit_each_briefed(&clean, &clean_world, name);
        let expected_attempts = attempts_state(&clean_world);
        let mut killed = 0;
        for seed in 1..=200u64 {
            let mut rng = Seeded(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
            let mut store = MemoryStore::new();
            let (n, _, world) = play(&mut store, &script, || {
                // Die in about two ticks of three, at any port call; more
                // calls than a tick makes is no kill.
                (rng.below(3) != 0).then(|| rng.below(CALLS))
            });
            killed += n;
            assert_eq!(end_state(&store), expected, "{name}, seed {seed}");
            assert_eq!(
                attempts_state(&world),
                expected_attempts,
                "{name}, seed {seed}"
            );
            one_start_per_unit_each_briefed(&store, &world, name);
        }
        assert!(killed > 50, "{name}: only {killed} kills");
    }
}

#[test]
fn every_single_kill_point_reaches_the_same_end_state_on_sqlite() {
    let dir = tempfile::tempdir().unwrap();
    for (n, (name, script)) in scenarios().into_iter().enumerate() {
        let path = dir.path().join(format!("clean-{n}.sqlite"));
        let mut clean = SqliteStore::create(&path, Path::new("/registry/repos.toml")).unwrap();
        let (_, calls, clean_world) = play(&mut clean, &script, || None);
        let expected = end_state(&clean);
        no_event_repeats(&expected.1);
        let expected_attempts = attempts_state(&clean_world);
        // Every call of every tick, and one past the last, which is no kill.
        for (tick_index, &made) in calls.iter().enumerate() {
            for point in 0..=made {
                let path = dir
                    .path()
                    .join(format!("kill-{n}-{tick_index}-{point}.sqlite"));
                let mut store =
                    SqliteStore::create(&path, Path::new("/registry/repos.toml")).unwrap();
                let mut seen = 0;
                let (killed, _, world) = play(&mut store, &script, || {
                    seen += 1;
                    (seen == tick_index + 1).then_some(point)
                });
                let what = format!("{name}, tick {tick_index}, killed at call {point}");
                assert_eq!(end_state(&store), expected, "{what}");
                assert_eq!(attempts_state(&world), expected_attempts, "{what}");
                one_start_per_unit_each_briefed(&store, &world, &what);
                assert_eq!(
                    killed,
                    usize::from(point < made),
                    "{name}, tick {tick_index}, call {point}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The criteria of ticket 03, one test each where the harness does not
// already show it.

#[test]
fn an_eligible_request_opens_exactly_one_loop_however_many_ticks_run() {
    let mut store = MemoryStore::new();
    ask(&mut store, "ref-rsb-serverless-stack", 7);
    ask(&mut store, "ref-rsb-serverless-stack", 7);
    let first = tick(&mut store, Mode::Act);
    assert!(matches!(
        &first.lines[0].outcome,
        Outcome::Applied(Applied::Recorded(e)) if e.kind == EventKind::LoopOpened
    ));
    for _ in 0..5 {
        tick(&mut store, Mode::Act);
    }
    let loops = store.loops().unwrap();
    assert_eq!(loops.len(), 1);
    assert_eq!(loops[0].ordinal, 1);
    let all = events(&store);
    let opened: Vec<&Event> = all
        .iter()
        .filter(|e| e.kind == EventKind::LoopOpened)
        .collect();
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].payload["requests"], serde_json::json!([1, 2]));
    assert_eq!(
        opened[0].payload["registry"]["sha"],
        "0123456789abcdef0123456789abcdef01234567"
    );
}

#[test]
fn a_factory_record_or_unlisted_repo_is_refused_with_a_reason_and_no_loop() {
    let mut store = MemoryStore::new();
    ask(&mut store, "gauge", 1);
    ask(&mut store, "research", 2);
    ask(&mut store, "nowhere", 3);
    tick(&mut store, Mode::Act);
    tick(&mut store, Mode::Act);
    assert!(store.loops().unwrap().is_empty());
    assert!(store.snapshot().unwrap().requests.is_empty());
    let refusals: Vec<serde_json::Value> = events(&store)
        .into_iter()
        .map(|e| {
            assert_eq!(e.kind, EventKind::RequestRefused);
            e.payload["refusal"].clone()
        })
        .collect();
    assert_eq!(
        refusals,
        [
            serde_json::json!({ "reason": "role", "role": "factory" }),
            serde_json::json!({ "reason": "not_listed" }),
            serde_json::json!({ "reason": "role", "role": "record" }),
        ]
    );
}

#[test]
fn an_observation_matching_no_row_parks_with_the_observation_attached() {
    let mut store = MemoryStore::new();
    let mut world = world();
    ask(&mut store, "lumen", 4);
    tick_in(&mut store, &mut world, Mode::Act);
    tick_in(&mut store, &mut world, Mode::Act);
    // The attempt's unit is gone with its end not recorded: no row matches.
    world.attempts.units.clear();
    let report = tick_in(&mut store, &mut world, Mode::Act);
    assert_eq!(
        report.lines[0].decision.map(|d| d.situation),
        Some(Situation::Unrecognized)
    );
    let parked = events(&store).pop().unwrap();
    assert_eq!(parked.kind, EventKind::LoopParked);
    assert_eq!(parked.payload["reason"], "unrecognized_state");
    let observation = &parked.payload["observation"];
    assert_eq!(observation["observed"], "issue");
    assert_eq!(observation["repo"], "lumen");
    assert_eq!(observation["run"]["state"], "open");
    let run = &store.loops().unwrap()[0];
    assert_eq!(run.state, LoopState::Parked);
    assert_eq!(run.park, Some(ParkReason::UnrecognizedState));
    // Parked again by the next tick, it is the same park: no second event.
    let before = events(&store).len();
    tick_in(&mut store, &mut world, Mode::Act);
    assert_eq!(events(&store).len(), before);
}

#[test]
fn a_request_on_an_issue_whose_loop_is_open_is_refused_and_the_loop_left_alone() {
    let mut store = MemoryStore::new();
    ask(&mut store, "lumen", 6);
    tick(&mut store, Mode::Act);
    let before = store.loops().unwrap();
    assert_eq!(before[0].state, LoopState::Open);
    let second = ask(&mut store, "lumen", 6);
    let report = tick(&mut store, Mode::Act);
    assert_eq!(
        report.lines[0].decision.map(|d| d.action),
        Some(Action::Refuse {
            refusal: Refusal::LoopAlreadyOpen {}
        })
    );
    assert_eq!(store.loops().unwrap(), before);
    assert!(store.snapshot().unwrap().requests.is_empty());
    let refused = events(&store).pop().unwrap();
    assert_eq!(refused.kind, EventKind::RequestRefused);
    assert_eq!(
        refused.payload["refusal"],
        serde_json::json!({ "reason": "loop_already_open" })
    );
    assert_eq!(refused.payload["requests"], serde_json::json!([second.0]));
    assert_eq!(
        events(&store)
            .iter()
            .filter(|e| e.kind == EventKind::LoopParked)
            .count(),
        0
    );
}

#[test]
fn an_unreachable_store_takes_no_action_and_names_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite");
    let err = crate::tick_store(&path, Mode::Act).err().unwrap();
    assert!(matches!(
        err,
        crate::Error::Unreachable {
            situation: Situation::StoreUnreachable,
            ..
        }
    ));
    let text = crate::report(&err);
    assert!(
        text.starts_with("store_unreachable: the tick took no action: "),
        "{text}"
    );
    assert!(text.contains("does not exist"), "{text}");
    assert!(!path.exists(), "the tick created the store");
}

#[test]
fn the_tick_exits_non_zero_when_the_store_is_unreachable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.sqlite");
    // `run` maps the error to a failing exit in `main`.
    let result = crate::tick_store(&path, Mode::Observe);
    assert!(result.is_err());
}

#[test]
fn observe_decides_as_a_tick_would_and_writes_nothing() {
    let mut store = MemoryStore::new();
    ask(&mut store, "ref-rsb-serverless-stack", 1);
    ask(&mut store, "lumen", 2);
    tick(&mut store, Mode::Act);
    ask(&mut store, "gauge", 3);
    ask(&mut store, "lumen", 2);
    ask(&mut store, "lumen", 5);
    store
        .flag(
            Flag {
                kind: FlagKind::Pause,
                scope: Some(LoopId(1)),
            },
            NOW,
        )
        .unwrap();
    store
        .flag(
            Flag {
                kind: FlagKind::Drain,
                scope: None,
            },
            NOW,
        )
        .unwrap();

    let before = store.clone();
    let observed = tick(&mut store, Mode::Observe);
    assert_eq!(store, before, "observe wrote to the store");
    assert!(
        observed
            .lines
            .iter()
            .all(|l| l.outcome == Outcome::Observed)
    );

    let acted = tick(&mut store, Mode::Act);
    let decisions = |r: &Report| {
        r.lines
            .iter()
            .map(|l| (l.repo.clone(), l.issue, l.loop_id, l.decision))
            .collect::<Vec<_>>()
    };
    assert_eq!(decisions(&observed), decisions(&acted));
    assert_ne!(store, before, "the acting tick wrote nothing to compare");
}

#[test]
fn observe_on_sqlite_leaves_the_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dev.sqlite");
    let mut store = SqliteStore::create(&path, Path::new("/registry/repos.toml")).unwrap();
    ask(&mut store, "lumen", 2);
    ask(&mut store, "gauge", 3);
    let before = end_state(&store);
    let observed = tick(&mut store, Mode::Observe);
    assert_eq!(end_state(&store), before);
    assert_eq!(observed.lines.len(), 2);
}

#[test]
fn a_trigger_during_the_tick_asks_for_one_more() {
    /// Inserts a request between the snapshot and the end of the tick.
    struct Interrupting(MemoryStore, Cell<bool>);

    impl Store for Interrupting {
        type Error = rsb_gauge_reconcile::ReconcileError;
        fn snapshot(&self) -> Result<Snapshot, Self::Error> {
            self.0.snapshot()
        }
        fn trigger(&self) -> Result<u64, Self::Error> {
            let base = self.0.trigger()?;
            Ok(base + u64::from(self.1.get()))
        }
        fn write(
            &mut self,
            write: &Write,
            at: i64,
            payload: &serde_json::Value,
        ) -> Result<Applied, Self::Error> {
            self.1.set(true);
            self.0.write(write, at, payload)
        }
        fn request(&mut self, r: &NewRequest, at: i64) -> Result<RequestId, Self::Error> {
            self.0.request(r, at)
        }
        fn label_request(
            &mut self,
            key: &LabelKey,
            agent: Agent,
            at: i64,
        ) -> Result<Option<RequestId>, Self::Error> {
            self.0.label_request(key, agent, at)
        }
        fn label_requested(&self, key: &LabelKey) -> Result<bool, Self::Error> {
            self.0.label_requested(key)
        }
        fn flag(&mut self, f: Flag, at: i64) -> Result<bool, Self::Error> {
            self.0.flag(f, at)
        }
        fn loops(&self) -> Result<Vec<Loop>, Self::Error> {
            self.0.loops()
        }
        fn events(&self, since: u64, limit: usize) -> Result<Vec<Event>, Self::Error> {
            self.0.events(since, limit)
        }
    }

    let mut quiet = MemoryStore::new();
    ask(&mut quiet, "lumen", 1);
    assert!(!tick(&mut quiet, Mode::Act).again);

    let mut store = Interrupting(MemoryStore::new(), Cell::new(false));
    ask(&mut store.0, "lumen", 1);
    assert!(tick(&mut store, Mode::Act).again);
}

#[test]
fn an_unreadable_registry_fails_only_the_issues_that_need_it() {
    let mut store = MemoryStore::new();
    ask(&mut store, "lumen", 1);
    tick(&mut store, Mode::Act);
    ask(&mut store, "lumen", 2);
    let broken: Result<Registry, String> = Err("cannot read /registry/repos.toml".into());
    let mut w = world();
    let report = tick::run(
        &mut store,
        &mut w.github,
        &mut w.attempts,
        &broken,
        &w.settings,
        &FixedClock,
        Mode::Act,
    )
    .unwrap();
    // The poll, and the one issue that needs a role.
    assert_eq!(report.failed, 2);
    assert!(matches!(
        &report.polls[..],
        [Poll { repo: None, polled: Polled::Failed(m) }] if m.contains("repos.toml")
    ));
    let failed = report.lines.iter().find(|l| l.issue == 2).unwrap();
    assert!(matches!(&failed.outcome, Outcome::Failed(m) if m.contains("repos.toml")));
    // The open loop needs no role: its first attempt starts.
    let started = report.lines.iter().find(|l| l.issue == 1).unwrap();
    assert!(matches!(
        started.outcome,
        Outcome::Applied(Applied::Recorded(_))
    ));
    assert!(
        events(&store)
            .iter()
            .any(|e| e.kind == EventKind::AttemptStarted && e.issue == Some(1))
    );
    assert!(
        events(&store)
            .iter()
            .any(|e| e.kind == EventKind::ReconcileFailed && e.issue == Some(2))
    );
}

// ---------------------------------------------------------------------------
// The criteria of ticket 05.

const REF: &str = "ref-rsb-serverless-stack";

fn opened(store: &impl Store) -> Vec<Event> {
    events(store)
        .into_iter()
        .filter(|e| e.kind == EventKind::LoopOpened)
        .collect()
}

#[test]
fn a_labelled_issue_on_the_reference_repo_opens_one_loop_on_the_next_tick() {
    let mut github = world();
    let mut store = MemoryStore::new();
    let event = github.label(REF, 5, LABEL);
    let report = tick_in(&mut store, &mut github, Mode::Act);
    assert!(report.polls.contains(&Poll {
        repo: Some(REF.into()),
        polled: Polled::Seen {
            labelled: 1,
            new: 1
        }
    }));
    let loops = store.loops().unwrap();
    assert_eq!(loops.len(), 1);
    let run = &loops[0];
    assert_eq!((run.repo.as_str(), run.issue), (REF, 5));
    let ticket = run.ticket.as_ref().unwrap();
    assert_eq!(ticket.id, "S-0001/1");
    assert_eq!(ticket.spec(), "S-0001");
    assert_eq!(ticket.path, TICKET);
    assert_eq!((ticket.branch.as_str(), ticket.sha.as_str()), ("main", SHA));
    assert_eq!(ticket.lockfile, world::sha256(b"lockfile"));
    let event_payload = &opened(&store)[0].payload;
    assert_eq!(event_payload["labels"], serde_json::json!([event]));
    assert_eq!(event_payload["world"]["lockfile"], ticket.lockfile);
    assert_eq!(event_payload["world"]["ticket"]["path"], TICKET);

    // A second tick opens nothing more: the label is still there, its event
    // already has a request.
    let again = tick_in(&mut store, &mut github, Mode::Act);
    assert!(again.polls.contains(&Poll {
        repo: Some(REF.into()),
        polled: Polled::Seen {
            labelled: 1,
            new: 0
        }
    }));
    assert_eq!(store.loops().unwrap().len(), 1);
    assert_eq!(opened(&store).len(), 1);
}

#[test]
fn a_tick_killed_after_the_label_request_is_inserted_inserts_no_second() {
    let mut github = world();
    let mut store = MemoryStore::new();
    github.label(REF, 5, LABEL);
    // Calls: 0 polls the reference repo, 1 inserts its request, 2 polls
    // lumen. Die there, after the insert and before the snapshot.
    let (result, killed, _) = gated(&mut store, &mut github, Some(2));
    assert!(killed && result.is_err());
    let pending = store.snapshot().unwrap().requests;
    assert_eq!(pending.len(), 1);
    assert!(store.loops().unwrap().is_empty());

    tick_in(&mut store, &mut github, Mode::Act);
    tick_in(&mut store, &mut github, Mode::Act);
    assert_eq!(store.loops().unwrap().len(), 1);
    let opened = opened(&store);
    assert_eq!(opened.len(), 1);
    assert_eq!(
        opened[0].payload["requests"],
        serde_json::json!([pending[0].id.0])
    );
    assert!(
        events(&store)
            .iter()
            .all(|e| e.kind != EventKind::RequestRefused),
        "a duplicate request was refused"
    );
}

#[test]
fn a_label_on_a_repo_without_the_lockfile_opens_nothing_and_records_why() {
    let mut github = world();
    let mut store = MemoryStore::new();
    github.label("libraw", 3, LABEL);
    tick_in(&mut store, &mut github, Mode::Act);
    tick_in(&mut store, &mut github, Mode::Act);
    assert!(store.loops().unwrap().is_empty());
    let all = events(&store);
    assert_eq!(all.len(), 1, "{all:?}");
    let refused = &all[0];
    assert_eq!(refused.kind, EventKind::RequestRefused);
    assert_eq!(refused.payload["situation"], "ineligible");
    assert_eq!(
        refused.payload["refusal"],
        serde_json::json!({ "reason": "no_lockfile" })
    );
    assert_eq!(
        refused.payload["world"]["lockfile"],
        serde_json::Value::Null
    );
    assert_eq!(refused.payload["world"]["sha"], SHA);
}

#[test]
fn an_issue_naming_no_ticket_is_refused_with_its_cause_not_parked() {
    let mut github = world();
    let mut store = MemoryStore::new();
    github.label(REF, 13, LABEL);
    ask(&mut store, REF, 40);
    tick_in(&mut store, &mut github, Mode::Act);
    assert!(store.loops().unwrap().is_empty());
    let refusals: Vec<serde_json::Value> = events(&store)
        .into_iter()
        .map(|e| {
            assert_eq!(e.kind, EventKind::RequestRefused);
            assert_eq!(e.payload["situation"], "ticket_unresolved");
            e.payload["refusal"].clone()
        })
        .collect();
    let refusal = |cause: TicketCause| serde_json::json!({ "reason": "no_ticket", "cause": cause });
    assert_eq!(
        refusals,
        [
            refusal(TicketCause::NoMarker),
            refusal(TicketCause::IssueMissing)
        ]
    );
}

#[test]
fn only_eligible_repos_the_driver_reaches_are_polled() {
    let mut github = world();
    let mut store = MemoryStore::new();
    let report = tick_in(&mut store, &mut github, Mode::Act);
    let polled: Vec<(&str, &Polled)> = report
        .polls
        .iter()
        .map(|p| (p.repo.as_deref().unwrap(), &p.polled))
        .collect();
    let none = Polled::Seen {
        labelled: 0,
        new: 0,
    };
    assert_eq!(
        polled,
        [
            (REF, &none),
            ("lumen", &none),
            ("libraw", &none),
            ("gordana-milan", &Polled::Unreached),
        ]
    );

    // A request on the repo the driver does not reach is refused, once,
    // rather than failing on every tick.
    ask(&mut store, "gordana-milan", 1);
    tick_in(&mut store, &mut github, Mode::Act);
    tick_in(&mut store, &mut github, Mode::Act);
    let all = events(&store);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(
        all[0].payload["refusal"],
        serde_json::json!({ "reason": "not_reached" })
    );
    assert!(all[0].payload.get("world").is_none());
}

#[test]
fn github_down_on_one_repo_fails_only_that_repo() {
    let mut github = world();
    let mut store = MemoryStore::new();
    github.label(REF, 2, LABEL);
    github.label("lumen", 2, LABEL);
    github.fail("lumen", true);
    let report = tick_in(&mut store, &mut github, Mode::Act);
    assert_eq!(report.failed, 1);
    assert_eq!(store.loops().unwrap().len(), 1);
    github.fail("lumen", false);
    tick_in(&mut store, &mut github, Mode::Act);
    assert_eq!(store.loops().unwrap().len(), 2);

    // A request whose world cannot be read fails at its boundary, stays
    // pending, and opens on the tick after GitHub is back.
    ask(&mut store, "lumen", 9);
    github.fail("lumen", true);
    let report = tick_in(&mut store, &mut github, Mode::Act);
    let line = report.lines.iter().find(|l| l.issue == 9).unwrap();
    assert!(matches!(&line.outcome, Outcome::Failed(m) if m.contains("observing GitHub")));
    assert_eq!(store.snapshot().unwrap().requests.len(), 1);
    github.fail("lumen", false);
    tick_in(&mut store, &mut github, Mode::Act);
    assert_eq!(store.loops().unwrap().len(), 3);
}

#[test]
fn observe_sees_a_new_label_and_records_nothing() {
    let mut github = world();
    let mut store = MemoryStore::new();
    github.label(REF, 6, LABEL);
    github.label("libraw", 6, LABEL);
    let before = store.clone();
    let observed = tick_in(&mut store, &mut github, Mode::Observe);
    assert_eq!(store, before, "observe wrote to the store");
    let acted = tick_in(&mut store, &mut github, Mode::Act);
    let decisions = |r: &Report| {
        r.lines
            .iter()
            .map(|l| (l.repo.clone(), l.issue, l.decision))
            .collect::<Vec<_>>()
    };
    assert_eq!(decisions(&observed), decisions(&acted));
    assert_eq!(observed.polls, acted.polls);
    assert_eq!(
        acted
            .lines
            .iter()
            .map(|l| l.decision.map(|d| d.action))
            .collect::<Vec<_>>(),
        [
            Some(Action::Refuse {
                refusal: Refusal::NoLockfile {}
            }),
            Some(Action::OpenLoop {}),
        ]
    );
}

/// The criterion that no installation token reaches an event, a log line or
/// a file. The fake has no token, so this pins the boundary's shape: the
/// tick's ports carry no token type, events are built from the observation,
/// and the one place a token meets disk, the clone, is tested in `git.rs`.
#[test]
fn no_event_payload_carries_anything_token_shaped() {
    let mut github = world();
    let mut store = MemoryStore::new();
    for (repo, issue) in [(REF, 1), (REF, 13), ("libraw", 1), ("lumen", 2)] {
        github.label(repo, issue, LABEL);
    }
    ask(&mut store, "gauge", 3);
    tick_in(&mut store, &mut github, Mode::Act);
    tick_in(&mut store, &mut github, Mode::Act);
    for event in events(&store) {
        let text = event.payload.to_string();
        for shape in [
            "ghs_",
            "ghp_",
            "gho_",
            "github_pat_",
            "Bearer",
            "x-access-token",
            "BEGIN",
        ] {
            assert!(!text.contains(shape), "{shape} in {text}");
        }
    }
}

// ---------------------------------------------------------------------------
// The criteria of ticket 06.

fn of_kind(store: &impl Store, kind: EventKind) -> Vec<Event> {
    events(store)
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

/// A loop opened on `REF#5`, with no attempt yet.
fn opened_loop() -> (MemoryStore, World) {
    let mut store = MemoryStore::new();
    let mut world = world();
    ask(&mut store, REF, 5);
    tick_in(&mut store, &mut world, Mode::Act);
    assert_eq!(store.loops().unwrap().len(), 1);
    (store, world)
}

/// The loop's attempt, started at `NOW`.
fn started_attempt() -> (MemoryStore, World) {
    let (mut store, mut world) = opened_loop();
    tick_in(&mut store, &mut world, Mode::Act);
    assert_eq!(store.snapshot().unwrap().attempts.len(), 1);
    (store, world)
}

const UNIT: &str = "gauge-run-1-1";

#[test]
fn the_next_tick_after_a_loop_opens_runs_one_unit_with_the_workspace_and_no_credential() {
    let (mut store, mut world) = opened_loop();
    let report = tick_in(&mut store, &mut world, Mode::Act);
    assert_eq!(
        report.lines[0].decision.map(|d| d.action),
        Some(Action::StartAttempt { attempt: 1 })
    );
    for _ in 0..3 {
        tick_in(&mut store, &mut world, Mode::Act);
    }
    let starts = &world.attempts.starts;
    assert_eq!(starts.len(), 1, "{starts:?}");
    let launch = &starts[0];
    assert_eq!(launch.unit, UNIT);
    assert_eq!(world.attempts.units[UNIT], UnitSeen::Running);

    // In agent-box, with the attempt directory as its workspace.
    let dir = Path::new("/fake/runs/1/1");
    let argv = launch.argv();
    let tail: Vec<&str> = argv
        .iter()
        .skip_while(|a| *a != "--")
        .map(String::as_str)
        .collect();
    assert_eq!(
        tail[..4],
        [
            "--",
            "/home/u/code/software/rsb/factory/agent-box/drive.sh",
            UNIT,
            "/fake/runs/1/1"
        ]
    );
    assert_eq!(launch.dir, dir);
    assert_eq!(
        world.attempts.branches[dir], "feat/5-first-ticket",
        "the branch from the ticket's slice and file name"
    );

    // Nothing in its environment but the bot identity, the skills, PATH and
    // HOME: no credential, and no --bare (Q17).
    let keys: Vec<&str> = launch.env.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        keys,
        ["AGENT_NAME", "AGENT_EMAIL", "RSB_SKILLS", "PATH", "HOME"]
    );
    for arg in &argv {
        assert!(!rsb_gauge_reconcile::brief::credential(arg), "{arg}");
        for shape in [
            "ghs_",
            "TOKEN",
            "GH_",
            "GITHUB",
            "GAUGE_STORE",
            ".pem",
            "sqlite",
        ] {
            assert!(!arg.contains(shape), "{shape} in {arg}");
        }
    }
    assert!(!argv.iter().any(|a| a == "--bare"));

    // The brief written is the one recorded, which validates against v1.
    let issued = of_kind(&store, EventKind::BriefIssued);
    assert_eq!(issued.len(), 1);
    assert_eq!(world.attempts.briefs[dir], issued[0].payload);
    rsb_gauge_reconcile::brief::validate(&issued[0].payload).unwrap();
    assert_eq!(
        issued[0].payload["issues"]["closes"],
        serde_json::json!([5])
    );

    // The record: number, unit, agent, model, start, bound and skills.
    let started = of_kind(&store, EventKind::AttemptStarted);
    assert_eq!(started.len(), 1);
    let record = &started[0].payload;
    assert_eq!(record["number"], 1);
    assert_eq!(record["unit"], UNIT);
    assert_eq!(record["agent"], "claude");
    assert_eq!(record["model"], "claude-opus-5-5");
    assert_eq!(record["started_at"], NOW);
    assert_eq!(record["runtime_bound"], 3600);
    assert_eq!(
        record["skills"],
        serde_json::to_value(crate::fake::fake_skills()).unwrap()
    );
    assert!(issued[0].seq < started[0].seq);
}

#[test]
fn kill_point_1_between_brief_issued_and_the_unit_start_starts_exactly_one_unit() {
    let (mut store, mut world) = opened_loop();
    let (_, _, log) = gated(&mut store.clone(), &mut world.clone(), None);
    let start = log.iter().position(|c| *c == "start").unwrap();
    let (result, killed, _) = gated(&mut store, &mut world, Some(start));
    assert!(killed && result.is_err());
    assert_eq!(of_kind(&store, EventKind::BriefIssued).len(), 1);
    assert!(world.attempts.units.is_empty(), "the unit started");
    assert!(store.snapshot().unwrap().attempts.is_empty());

    for _ in 0..3 {
        tick_in(&mut store, &mut world, Mode::Act);
    }
    assert_eq!(world.attempts.starts.len(), 1);
    assert_eq!(of_kind(&store, EventKind::BriefIssued).len(), 1);
    assert_eq!(of_kind(&store, EventKind::AttemptStarted).len(), 1);
}

#[test]
fn a_tick_killed_between_the_unit_start_and_the_record_starts_no_second_unit() {
    let (mut store, mut world) = opened_loop();
    let (_, _, log) = gated(&mut store.clone(), &mut world.clone(), None);
    let start = log.iter().position(|c| *c == "start").unwrap();
    assert_eq!(log[start + 1], "store", "the record follows the start");
    let (result, killed, _) = gated(&mut store, &mut world, Some(start + 1));
    assert!(killed && result.is_err());
    assert_eq!(world.attempts.units[UNIT], UnitSeen::Running);
    assert!(store.snapshot().unwrap().attempts.is_empty());

    let report = tick_in(&mut store, &mut world, Mode::Act);
    assert_eq!(
        report.lines[0].decision.map(|d| d.action),
        Some(Action::StartAttempt { attempt: 1 })
    );
    for _ in 0..2 {
        tick_in(&mut store, &mut world, Mode::Act);
    }
    assert_eq!(world.attempts.starts.len(), 1, "a second unit started");
    let attempts = store.snapshot().unwrap().attempts;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].started.unit, UNIT);
    assert_eq!(attempts[0].end, None);
}

#[test]
fn an_attempt_past_its_bound_is_stopped_and_recorded_timed_out() {
    let (mut store, mut world) = started_attempt();
    world.now = NOW + 3599;
    tick_in(&mut store, &mut world, Mode::Act);
    assert!(world.attempts.stops.is_empty(), "stopped within its bound");
    world.now = NOW + 3600;
    let report = tick_in(&mut store, &mut world, Mode::Act);
    assert_eq!(
        report.lines[0].decision.map(|d| d.action),
        Some(Action::StopAttempt {
            attempt: 1,
            cause: AttemptCause::TimedOut
        })
    );
    assert_eq!(world.attempts.stops, [UNIT]);
    let end = store.snapshot().unwrap().attempts[0].end.clone().unwrap();
    assert_eq!(end.cause, Some(AttemptCause::TimedOut));
    assert_eq!((end.at, end.exit), (NOW + 3600, None));
    let ended = of_kind(&store, EventKind::AttemptEnded);
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].payload["cause"], "timed_out");
}

#[test]
fn a_kill_between_recording_the_stop_and_stopping_stops_on_restart() {
    let (mut store, mut world) = started_attempt();
    world.now = NOW + 3600;
    let (_, _, log) = gated(&mut store.clone(), &mut world.clone(), None);
    let stop = log.iter().position(|c| *c == "stop").unwrap();
    let (_, killed, _) = gated(&mut store, &mut world, Some(stop));
    assert!(killed);
    assert_eq!(world.attempts.units[UNIT], UnitSeen::Running);
    tick_in(&mut store, &mut world, Mode::Act);
    assert_eq!(world.attempts.stops, [UNIT]);
    assert_eq!(of_kind(&store, EventKind::AttemptEnded).len(), 1);
}

#[test]
fn a_non_zero_exit_with_no_sealed_outbox_records_exited_nonzero() {
    let (mut store, mut world) = started_attempt();
    world.attempts.outputs.insert(
        PathBuf::from("/fake/runs/1/1"),
        RunOutput {
            exit: None,
            stdout: br#"{"type":"result","total_cost_usd":0.5,"usage":{"input_tokens":7,"output_tokens":3}}"#
                .to_vec(),
            commits: vec!["c0ffee".into()],
        },
    );
    world.attempts.set(UNIT, UnitSeen::Exited { code: Some(2) });
    tick_in(&mut store, &mut world, Mode::Act);
    let end = store.snapshot().unwrap().attempts[0].end.clone().unwrap();
    assert_eq!(end.cause, Some(AttemptCause::ExitedNonzero));
    assert_eq!(end.exit, Some(2));
    let ended = of_kind(&store, EventKind::AttemptEnded);
    assert_eq!(ended.len(), 1);
    let payload = &ended[0].payload;
    assert_eq!(payload["cause"], "exited_nonzero");
    assert_eq!(payload["exit"], 2);
    assert_eq!(payload["commits"], serde_json::json!(["c0ffee"]));
    assert_eq!(payload["usage"]["dollars"], 0.5);
    assert_eq!(payload["usage"]["input_tokens"], 7);
}

#[test]
fn a_non_zero_exit_with_a_sealed_outbox_records_no_cause() {
    let (mut store, mut world) = started_attempt();
    world.attempts.seal(Path::new("/fake/runs/1/1"));
    world.attempts.set(UNIT, UnitSeen::Exited { code: Some(1) });
    tick_in(&mut store, &mut world, Mode::Act);
    let end = store.snapshot().unwrap().attempts[0].end.clone().unwrap();
    assert_eq!((end.exit, end.cause), (Some(1), None));
    assert!(world.attempts.stops.is_empty());
}

#[test]
fn an_invalid_brief_parks_the_loop_and_writes_no_brief() {
    let (mut store, mut world) = opened_loop();
    if let Ok(settings) = &mut world.settings {
        // Brief v1 wants a runtime bound of at least 1.
        settings.config.runtime_bound = 0;
    }
    tick_in(&mut store, &mut world, Mode::Act);
    tick_in(&mut store, &mut world, Mode::Act);
    assert!(world.attempts.briefs.is_empty(), "a brief.json was written");
    assert!(world.attempts.starts.is_empty());
    assert!(of_kind(&store, EventKind::BriefIssued).is_empty());
    let run = &store.loops().unwrap()[0];
    assert_eq!(run.state, LoopState::Parked);
    assert_eq!(run.park, Some(ParkReason::UnrecognizedState));
    let parked = of_kind(&store, EventKind::LoopParked);
    assert_eq!(parked.len(), 1);
    assert_eq!(
        parked[0].payload["brief_errors"],
        serde_json::json!(["runtime_bound: not an integer of at least 1"])
    );
}

#[test]
fn a_ticket_with_no_branch_slice_parks_and_writes_no_brief() {
    let (mut store, mut world) = opened_loop();
    world.attempts.ticket = Some("---\nslice: human\n---\n".into());
    tick_in(&mut store, &mut world, Mode::Act);
    assert!(world.attempts.briefs.is_empty() && world.attempts.starts.is_empty());
    assert_eq!(store.loops().unwrap()[0].state, LoopState::Parked);
}

#[test]
fn drain_holds_a_start() {
    let (mut store, mut world) = opened_loop();
    store
        .flag(
            Flag {
                kind: FlagKind::Drain,
                scope: None,
            },
            NOW,
        )
        .unwrap();
    let report = tick_in(&mut store, &mut world, Mode::Act);
    assert_eq!(
        report.lines[0].decision.map(|d| d.situation),
        Some(Situation::Draining)
    );
    tick_in(&mut store, &mut world, Mode::Act);
    assert!(world.attempts.starts.is_empty());
    assert!(of_kind(&store, EventKind::BriefIssued).is_empty());
}

#[test]
fn a_codex_request_is_refused_with_a_reason_and_opens_no_loop() {
    let mut store = MemoryStore::new();
    store
        .request(
            &NewRequest {
                repo: REF.into(),
                issue: 5,
                agent: Agent::Codex,
            },
            NOW,
        )
        .unwrap();
    tick(&mut store, Mode::Act);
    assert!(store.loops().unwrap().is_empty());
    let refused = of_kind(&store, EventKind::RequestRefused);
    assert_eq!(
        refused[0].payload["refusal"],
        serde_json::json!({ "reason": "no_adapter", "agent": "codex" })
    );
}

#[test]
fn without_the_bot_identity_a_start_fails_before_any_brief() {
    let (mut store, mut world) = opened_loop();
    world.settings = Err("no config".into());
    let report = tick_in(&mut store, &mut world, Mode::Act);
    assert!(matches!(&report.lines[0].outcome, Outcome::Failed(m) if m.contains("no config")));
    world.settings = Ok(crate::attempt::tests::settings());
    if let Ok(settings) = &mut world.settings {
        settings.config.agent_name = None;
    }
    let report = tick_in(&mut store, &mut world, Mode::Act);
    assert!(matches!(&report.lines[0].outcome, Outcome::Failed(m) if m.contains("agent_name")));
    assert!(of_kind(&store, EventKind::BriefIssued).is_empty());
    assert!(world.attempts.starts.is_empty());
}
