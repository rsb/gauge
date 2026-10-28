//! The kill harness and the tick's tests. A scripted world runs through
//! ticks against the fakes; a seeded kill stops a tick between any two port
//! calls; the tick restarts from the store alone; and the end state must equal
//! the run with no kill, with no event repeated. Later tickets add their
//! rows' scenarios to [`scenarios`].

use std::cell::Cell;
use std::path::Path;

use rsb_gauge_reconcile::store::{
    Agent, Applied, Event, EventKind, Flag, FlagKind, Loop, LoopId, LoopState, NewRequest,
    RequestId, Snapshot, Store, Write,
};
use rsb_gauge_reconcile::{Action, MemoryStore, ParkReason, Refusal, Situation};
use rsb_gauge_schema::Repos;
use rsb_gauge_store::SqliteStore;

use crate::registry::{Registry, Source};
use crate::tick::{self, Clock, Mode, Outcome, Report};

const NOW: i64 = 1_793_000_000;

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> i64 {
        NOW
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

fn tick(store: &mut impl Store, mode: Mode) -> Report {
    tick::run(store, &registry(), &FixedClock, mode).unwrap()
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

/// A store that dies at one port call: that call and every later one fail
/// before reaching the store, as if the process had been killed between the
/// previous call and this one.
struct Killable<'a, S> {
    inner: &'a mut S,
    calls: Cell<usize>,
    kill_at: Option<usize>,
}

#[derive(Debug, thiserror::Error)]
enum Killed<E: std::error::Error + 'static> {
    #[error("killed")]
    Killed,
    #[error("store")]
    Store(#[source] E),
}

impl<'a, S: Store> Killable<'a, S> {
    fn new(inner: &'a mut S, kill_at: Option<usize>) -> Self {
        Self {
            inner,
            calls: Cell::new(0),
            kill_at,
        }
    }

    fn gate(&self) -> Result<(), Killed<S::Error>> {
        let n = self.calls.get();
        self.calls.set(n + 1);
        if self.kill_at.is_some_and(|k| n >= k) {
            return Err(Killed::Killed);
        }
        Ok(())
    }

    fn killed(&self) -> bool {
        self.kill_at.is_some_and(|k| self.calls.get() > k)
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
    Pause(Option<i64>),
    Tick,
}

/// The scenarios, each a script of owner writes and ticks.
fn scenarios() -> Vec<(&'static str, Vec<Step>)> {
    use Step::*;
    vec![
        (
            "an eligible request opens one loop, which then parks unrecognized",
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
    ]
}

/// Runs a script. At each tick, `kills` picks the port call to die at, or
/// none; a killed tick is restarted until one completes, as systemd would.
/// Settling ticks after the script let both runs reach rest. Returns how
/// many ticks were killed.
fn play<S: Store>(
    store: &mut S,
    script: &[Step],
    mut kills: impl FnMut() -> Option<usize>,
) -> usize {
    let mut killed = 0;
    for step in script {
        match *step {
            Step::Ask(repo, issue) => {
                ask(store, repo, issue);
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
            Step::Tick => loop {
                let mut killable = Killable::new(store, kills());
                let result = tick::run(&mut killable, &registry(), &FixedClock, Mode::Act);
                if !killable.killed() {
                    let report = result.unwrap();
                    assert_eq!(report.failed, 0, "{report:?}");
                    break;
                }
                killed += 1;
            },
        }
    }
    for _ in 0..3 {
        tick(store, Mode::Act);
    }
    killed
}

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
        play(&mut clean, &script, || None);
        let expected = end_state(&clean);
        no_event_repeats(&expected.1);
        let mut killed = 0;
        for seed in 1..=200u64 {
            let mut rng = Seeded(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
            let mut store = MemoryStore::new();
            killed += play(&mut store, &script, || {
                // Die in about two ticks of three, at any of the first eight
                // port calls; more calls than a tick makes is no kill.
                (rng.below(3) != 0).then(|| rng.below(8))
            });
            assert_eq!(end_state(&store), expected, "{name}, seed {seed}");
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
        play(&mut clean, &script, || None);
        let expected = end_state(&clean);
        no_event_repeats(&expected.1);
        let ticks = script.iter().filter(|s| matches!(s, Step::Tick)).count();
        for tick_index in 0..ticks {
            for point in 0..8 {
                let path = dir
                    .path()
                    .join(format!("kill-{n}-{tick_index}-{point}.sqlite"));
                let mut store =
                    SqliteStore::create(&path, Path::new("/registry/repos.toml")).unwrap();
                let mut seen = 0;
                let killed = play(&mut store, &script, || {
                    seen += 1;
                    (seen == tick_index + 1).then_some(point)
                });
                assert_eq!(
                    end_state(&store),
                    expected,
                    "{name}, tick {tick_index}, killed at call {point}"
                );
                if point < 2 {
                    // Every tick makes at least two port calls.
                    assert_eq!(killed, 1, "{name}, tick {tick_index}, call {point}");
                }
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
    ask(&mut store, "lumen", 4);
    tick(&mut store, Mode::Act);
    let report = tick(&mut store, Mode::Act);
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
    tick(&mut store, Mode::Act);
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
    let report = tick::run(&mut store, &broken, &FixedClock, Mode::Act).unwrap();
    assert_eq!(report.failed, 1);
    let failed = report.lines.iter().find(|l| l.issue == 2).unwrap();
    assert!(matches!(&failed.outcome, Outcome::Failed(m) if m.contains("repos.toml")));
    let parked = report.lines.iter().find(|l| l.issue == 1).unwrap();
    assert!(matches!(
        parked.outcome,
        Outcome::Applied(Applied::Recorded(_))
    ));
    assert!(
        events(&store)
            .iter()
            .any(|e| e.kind == EventKind::LoopParked && e.issue == Some(1))
    );
    assert!(
        events(&store)
            .iter()
            .any(|e| e.kind == EventKind::ReconcileFailed && e.issue == Some(2))
    );
}
