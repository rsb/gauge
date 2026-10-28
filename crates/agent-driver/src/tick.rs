//! The tick: one run-to-completion pass. It first polls the eligible repos
//! for the run label and records a request per new label event, then makes a
//! pass over every issue with a pending request or an open loop. Per issue it
//! observes, decides, performs the one action and records its event, then
//! moves on. Nothing is held between ticks: every start is a resume.
//! `--observe` is the same pass without the act step.

use rsb_gauge_reconcile::adapter::{AgentAdapter, AttemptSpec, ClaudeAdapter};
use rsb_gauge_reconcile::brief::{self, Brief, FreshBrief};
use rsb_gauge_reconcile::store::{
    Agent, Applied, AttemptCause, AttemptEnd, LabelKey, Loop, LoopId, LoopState, LoopTicket,
    NewAttempt, Request, RequestId, Store, Write, unit_name,
};
use rsb_gauge_reconcile::{
    Action, AttemptWorld, Decision, IssueRows, Observation, ParkReason, Refusal, Situation,
    TicketSeen, UnitSeen, WorldSeen, decide, issues,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::attempt::{Attempts, Launch, Settings, Workspace};
use crate::error::{Error, report as report_error};
use crate::registry::{Registry, Source};
use crate::world::{self, GitHub, LABEL};

/// The time, as Unix seconds. A port so tests fix it.
pub trait Clock {
    fn now(&self) -> i64;
}

/// The wall clock. A clock before 1970 reads as 0.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Observe, decide and act.
    Act,
    /// Observe and decide; write nothing.
    Observe,
}

/// What became of one issue.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// `--observe`: decided, not acted on.
    Observed,
    /// The action wrote nothing: wait, or a wait with its situation already
    /// recorded.
    Waited,
    Applied(Applied),
    /// The reconcile failed at this issue's boundary; other issues went on.
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub repo: String,
    pub issue: u64,
    pub loop_id: Option<LoopId>,
    /// `None` when the issue failed before a decision.
    pub decision: Option<Decision>,
    pub outcome: Outcome,
}

/// What polling one repo for the run label found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Polled {
    /// `labelled` open issues carry the label; `new` of them by an event no
    /// request has yet, each recorded as one (in `--observe`, would be).
    Seen {
        labelled: usize,
        new: usize,
    },
    /// The driver's config does not list the repo, so it is not polled.
    Unreached,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Poll {
    /// `None` when no repo could be polled: `repos.toml` is unavailable.
    pub repo: Option<String>,
    pub polled: Polled,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub polls: Vec<Poll>,
    pub lines: Vec<Line>,
    pub failed: usize,
    /// A trigger arrived during the tick, so one more tick follows.
    pub again: bool,
}

/// Why one issue's reconcile failed.
#[derive(Debug, thiserror::Error)]
enum UnitError<E: std::error::Error + 'static> {
    #[error("repos.toml is unavailable: {0}")]
    Registry(String),
    #[error("observing GitHub: {0}")]
    GitHub(String),
    #[error("internal error: {0}")]
    Internal(&'static str),
    #[error("payload")]
    Payload(#[from] serde_json::Error),
    #[error("the driver's config is unavailable: {0}")]
    Settings(String),
    #[error("attempt")]
    Attempt(#[source] Error),
    #[error("store")]
    Store(#[source] E),
}

/// Runs one tick. Only a failure to read the snapshot or the trigger fails
/// the tick; a failure polling one repo, or inside one issue, is caught at
/// its boundary, recorded where the store allows, and counted.
pub fn run<S: Store, G: GitHub, A: Attempts>(
    store: &mut S,
    github: &mut G,
    attempts: &mut A,
    registry: &Result<Registry, String>,
    settings: &Result<Settings, String>,
    clock: &dyn Clock,
    mode: Mode,
) -> Result<Report, S::Error> {
    let mut report = Report::default();
    let unwritten = poll(store, github, registry, clock, mode, &mut report);
    let mut snapshot = store.snapshot()?;
    // In `--observe`, the requests the poll would have recorded join the
    // pass as they would after it.
    snapshot.requests.extend(unwritten);
    for rows in issues(&snapshot) {
        let loop_id = rows.run.as_ref().map(|r| r.id);
        let mut decided = None;
        let mut ports = Ports {
            store: &mut *store,
            github: &mut *github,
            attempts: &mut *attempts,
        };
        let result = reconcile(
            &mut ports,
            &rows,
            &snapshot.flags,
            registry,
            settings,
            clock,
            mode,
            &mut decided,
        );
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                report.failed += 1;
                let mut message = report_error(&error);
                if mode == Mode::Act {
                    let failure = Write::Failure {
                        repo: rows.repo.clone(),
                        issue: rows.issue,
                        loop_id,
                    };
                    let payload = serde_json::Value::Object(serde_json::Map::from_iter([(
                        "error".to_owned(),
                        serde_json::Value::String(message.clone()),
                    )]));
                    if let Err(e) = store.write(&failure, clock.now(), &payload) {
                        let e = UnitError::<S::Error>::Store(e);
                        message = format!("{message}; recording the failure: {}", report_error(&e));
                    }
                }
                Outcome::Failed(message)
            }
        };
        report.lines.push(Line {
            repo: rows.repo,
            issue: rows.issue,
            loop_id,
            decision: decided,
            outcome,
        });
    }
    report.again = store.trigger()? != snapshot.trigger;
    Ok(report)
}

/// Polls every repo `repos.toml` gives role `project` or `reference`, and
/// that the driver reaches, for open issues carrying the run label, and
/// records a request for each label event that has none. A factory or record
/// repo is never polled. Returns, in `--observe`, the requests it would have
/// recorded, with id 0.
fn poll<S: Store, G: GitHub>(
    store: &mut S,
    github: &mut G,
    registry: &Result<Registry, String>,
    clock: &dyn Clock,
    mode: Mode,
    report: &mut Report,
) -> Vec<Request> {
    let registry = match registry {
        Ok(registry) => registry,
        Err(e) => {
            report.failed += 1;
            report.polls.push(Poll {
                repo: None,
                polled: Polled::Failed(format!("repos.toml is unavailable: {e}")),
            });
            return Vec::new();
        }
    };
    let mut unwritten = Vec::new();
    for repo in registry.repos.repos.iter().filter(|r| r.role.is_eligible()) {
        let polled = if github.reaches(&repo.name) {
            poll_repo(store, github, &repo.name, clock, mode, &mut unwritten)
        } else {
            Polled::Unreached
        };
        if matches!(polled, Polled::Failed(_)) {
            report.failed += 1;
        }
        report.polls.push(Poll {
            repo: Some(repo.name.clone()),
            polled,
        });
    }
    unwritten
}

fn poll_repo<S: Store, G: GitHub>(
    store: &mut S,
    github: &mut G,
    repo: &str,
    clock: &dyn Clock,
    mode: Mode,
    unwritten: &mut Vec<Request>,
) -> Polled {
    let labelled = match github.labelled(repo, LABEL) {
        Ok(labelled) => labelled,
        Err(e) => return Polled::Failed(report_error(&e)),
    };
    let mut new = 0;
    for seen in &labelled {
        let key = LabelKey {
            repo: repo.to_owned(),
            issue: seen.issue,
            event: seen.event,
        };
        let inserted = match mode {
            Mode::Act => store
                .label_request(&key, Agent::Claude, clock.now())
                .map(|id| id.is_some()),
            Mode::Observe => store.label_requested(&key).map(|known| {
                if !known {
                    unwritten.push(Request {
                        id: RequestId(0),
                        repo: key.repo.clone(),
                        issue: key.issue,
                        agent: Agent::Claude,
                        at: clock.now(),
                        label: Some(key.event),
                    });
                }
                !known
            }),
        };
        match inserted {
            Ok(true) => new += 1,
            Ok(false) => {}
            Err(e) => {
                let e = UnitError::<S::Error>::Store(e);
                return Polled::Failed(format!("recording a label request: {}", report_error(&e)));
            }
        }
    }
    Polled::Seen {
        labelled: labelled.len(),
        new,
    }
}

/// The tick's ports, for one issue.
struct Ports<'a, S, G, A> {
    store: &'a mut S,
    github: &'a mut G,
    attempts: &'a mut A,
}

/// Observes and decides one issue, then acts unless observing. The decision
/// is left in `decided` even when acting fails. GitHub is read only for a
/// request on a repo whose role is eligible and that the driver reaches; the
/// current attempt's unit and outbox only for a loop with an attempt.
#[expect(clippy::too_many_arguments, reason = "the tick's ports and its inputs")]
fn reconcile<S: Store, G: GitHub, A: Attempts>(
    ports: &mut Ports<'_, S, G, A>,
    rows: &IssueRows,
    flags: &[rsb_gauge_reconcile::store::Flag],
    registry: &Result<Registry, String>,
    settings: &Result<Settings, String>,
    clock: &dyn Clock,
    mode: Mode,
    decided: &mut Option<Decision>,
) -> Result<Outcome, UnitError<S::Error>> {
    let (role, source) = if rows.needs_role() {
        let registry = registry
            .as_ref()
            .map_err(|e| UnitError::Registry(e.clone()))?;
        (registry.role(&rows.repo), Some(&registry.source))
    } else {
        (None, None)
    };
    let world = match role {
        Some(role) if role.is_eligible() && ports.github.reaches(&rows.repo) => Some(
            world::observe(ports.github, &rows.repo, rows.issue)
                .map_err(|e| UnitError::GitHub(report_error(&e)))?,
        ),
        _ => None,
    };
    let attempt = match rows.run.as_ref().zip(rows.current()) {
        Some((run, number)) => {
            let unit = ports
                .attempts
                .unit(&unit_name(run.id, number))
                .map_err(UnitError::Attempt)?;
            let sealed = ports
                .attempts
                .sealed(&ports.attempts.dir(run.id, number))
                .map_err(UnitError::Attempt)?;
            Some(AttemptWorld { unit, sealed })
        }
        None => None,
    };
    // One attempt at a time (row N14): any other attempt's running unit,
    // looked up per issue so a start earlier in this tick counts.
    let another_running = match rows.run.as_ref().filter(|r| r.state == LoopState::Open) {
        Some(run) => {
            let own = rows.current().map(|number| unit_name(run.id, number));
            let running = ports.attempts.running().map_err(UnitError::Attempt)?;
            running.iter().any(|name| Some(name) != own.as_ref())
        }
        None => false,
    };
    let now = clock.now();
    let observation = rows.observe(flags, role, world.clone(), attempt, another_running, now);
    let decision = decide(&observation);
    *decided = Some(decision);
    if mode == Mode::Observe {
        return Ok(Outcome::Observed);
    }
    if let Some(run) = &rows.run {
        let act = Act {
            rows,
            run,
            decision: &decision,
            observation: &observation,
            now,
        };
        match decision.action {
            Action::StartAttempt { attempt } => {
                let settings = settings
                    .as_ref()
                    .map_err(|e| UnitError::Settings(e.clone()))?;
                return start_attempt(ports, &act, settings, attempt);
            }
            Action::StopAttempt { attempt, cause } => {
                return stop_attempt(ports, &act, attempt, cause);
            }
            Action::EndAttempt {
                attempt,
                exit,
                cause,
            } => return end_attempt(ports, &act, attempt, exit, cause),
            _ => {}
        }
    }
    let Some(write) = plan(rows, &decision, world.as_ref())? else {
        return Ok(Outcome::Waited);
    };
    let payload = payload(rows, &decision, &observation, source, world.as_ref())?;
    match ports
        .store
        .write(&write, now, &payload)
        .map_err(UnitError::Store)?
    {
        Applied::Noop if matches!(decision.action, Action::Wait {}) => Ok(Outcome::Waited),
        applied => Ok(Outcome::Applied(applied)),
    }
}

/// What an attempt action acts on.
struct Act<'a> {
    rows: &'a IssueRows,
    run: &'a Loop,
    decision: &'a Decision,
    observation: &'a Observation,
    now: i64,
}

impl Act<'_> {
    fn write<S: Store>(
        &self,
        store: &mut S,
        write: &Write,
        payload: &Value,
    ) -> Result<Applied, UnitError<S::Error>> {
        store
            .write(write, self.now, payload)
            .map_err(UnitError::Store)
    }

    /// Records the derived situation after an attempt action's writes.
    fn situation<S: Store>(&self, store: &mut S) -> Result<(), UnitError<S::Error>> {
        let write = Write::Situation {
            loop_id: self.run.id,
            situation: self.decision.situation,
        };
        self.write(store, &write, &Value::Null).map(|_| ())
    }
}

/// The adapter for a loop's agent. Codex is refused when its request is
/// decided (row 11), so no Codex loop reaches here.
fn adapter<E: std::error::Error + 'static>(
    agent: Agent,
) -> Result<&'static dyn AgentAdapter, UnitError<E>> {
    match agent {
        Agent::Claude => Ok(&ClaudeAdapter),
        Agent::Codex => Err(UnitError::Internal(
            "an attempt for an agent with no adapter",
        )),
    }
}

/// Builds the fresh brief for attempt `number` from the loop's ticket, read
/// from the clone for its slice. Every reason it cannot be built or fails
/// brief v1 is returned.
fn build_brief(
    run: &Loop,
    ticket: &LoopTicket,
    text: Option<&str>,
    number: u32,
    runtime_bound: u64,
) -> Result<Value, Vec<String>> {
    let text = text.ok_or_else(|| vec![format!("{}: not in the clone", ticket.path)])?;
    let slice = brief::slice(text)
        .ok_or_else(|| vec![format!("{}: no slice in its front matter", ticket.path)])?;
    let branch = brief::branch(slice, run.issue, &ticket.path)
        .ok_or_else(|| vec![format!("slice {slice:?}: no branch is cut for it")])?;
    Brief::fresh(&FreshBrief {
        repo: &run.repo,
        issue: run.issue,
        base_sha: &ticket.sha,
        base_branch: &ticket.branch,
        branch: &branch,
        ticket_path: &ticket.path,
        spec: ticket.spec(),
        attempt: number,
        runtime_bound,
    })
    .to_checked()
}

/// Row N6. Each step looks its natural key up first, so a restart after a
/// kill anywhere starts exactly one unit: (a) `brief_issued` for (loop,
/// attempt), built and checked, or the loop parks and nothing is written;
/// (b) the unit, by name: only an absent one gets its workspace and is
/// started; (c) the attempt record.
fn start_attempt<S: Store, G: GitHub, A: Attempts>(
    ports: &mut Ports<'_, S, G, A>,
    act: &Act<'_>,
    settings: &Settings,
    number: u32,
) -> Result<Outcome, UnitError<S::Error>> {
    let run = act.run;
    let config = &settings.config;
    if config.agent_name.is_none() || config.agent_email.is_none() {
        return Err(UnitError::Attempt(Error::NoAgentIdentity));
    }
    let adapter = adapter(run.agent)?;
    let dir = ports.attempts.dir(run.id, number);
    let unit = unit_name(run.id, number);
    let Some(ticket) = &run.ticket else {
        return park_unbriefed(ports, act, &["the loop has no ticket".to_owned()]);
    };
    let ws = Workspace {
        repo: &run.repo,
        base_sha: &ticket.sha,
        ticket_path: &ticket.path,
    };

    // (a) The brief.
    let brief = match act.rows.brief(number) {
        Some(issued) => issued.brief.clone(),
        None => {
            let text = ports
                .attempts
                .ticket(ports.github, &dir, &ws)
                .map_err(UnitError::Attempt)?;
            match build_brief(run, ticket, text.as_deref(), number, config.runtime_bound) {
                Ok(brief) => {
                    let write = Write::IssueBrief {
                        loop_id: run.id,
                        attempt: number,
                        brief: brief.clone(),
                    };
                    act.write(ports.store, &write, &brief)?;
                    brief
                }
                Err(errors) => return park_unbriefed(ports, act, &errors),
            }
        }
    };
    let branch = brief
        .get("branch")
        .and_then(Value::as_str)
        .ok_or(UnitError::Internal("a recorded brief with no branch"))?;
    let runtime_bound =
        brief
            .get("runtime_bound")
            .and_then(Value::as_u64)
            .ok_or(UnitError::Internal(
                "a recorded brief with no runtime bound",
            ))?;
    let spec = AttemptSpec {
        unit: unit.clone(),
        attempt: number,
        model: config.model.clone(),
        runtime_bound,
    };

    // (b) The unit, looked up by name before it is started.
    let seen = ports.attempts.unit(&unit).map_err(UnitError::Attempt)?;
    let skills = ports
        .attempts
        .skills(&config.rsb_skills)
        .map_err(UnitError::Attempt)?;
    if seen == UnitSeen::Absent {
        ports
            .attempts
            .prepare(ports.github, &dir, &ws, branch, &brief)
            .map_err(UnitError::Attempt)?;
        let launch = Launch::new(settings, &unit, &dir, &adapter.invocation(&spec))
            .map_err(UnitError::Attempt)?;
        ports.attempts.start(&launch).map_err(UnitError::Attempt)?;
    }

    // (c) The record.
    let record = NewAttempt {
        number,
        unit,
        agent: run.agent,
        model: spec.model,
        started_at: act.now,
        runtime_bound,
        skills,
    };
    let payload = serde_json::to_value(&record)?;
    let write = Write::StartAttempt {
        loop_id: run.id,
        attempt: record,
    };
    let applied = act.write(ports.store, &write, &payload)?;
    act.situation(ports.store)?;
    Ok(Outcome::Applied(applied))
}

/// Fail closed: a brief that cannot be built or fails brief v1 is never
/// recorded or written; the loop parks with the reasons.
fn park_unbriefed<S: Store, G: GitHub, A: Attempts>(
    ports: &mut Ports<'_, S, G, A>,
    act: &Act<'_>,
    errors: &[String],
) -> Result<Outcome, UnitError<S::Error>> {
    let reason = ParkReason::UnrecognizedState;
    let write = Write::Park {
        loop_id: act.run.id,
        reason,
        situation: act.decision.situation,
    };
    let payload = json!({
        "situation": act.decision.situation,
        "reason": reason,
        "brief_errors": errors,
        "observation": act.observation,
    });
    act.write(ports.store, &write, &payload)
        .map(Outcome::Applied)
}

/// Row N2: record the end, then stop the unit. A tick killed between the two
/// leaves the unit running with its end recorded, and row N2 stops it again.
fn stop_attempt<S: Store, G: GitHub, A: Attempts>(
    ports: &mut Ports<'_, S, G, A>,
    act: &Act<'_>,
    number: u32,
    cause: AttemptCause,
) -> Result<Outcome, UnitError<S::Error>> {
    let end = AttemptEnd {
        at: act.now,
        exit: None,
        cause: Some(cause),
    };
    let payload = json!({
        "situation": act.decision.situation,
        "attempt": number,
        "exit": end.exit,
        "cause": cause,
    });
    let write = Write::EndAttempt {
        loop_id: act.run.id,
        attempt: number,
        end,
    };
    let applied = act.write(ports.store, &write, &payload)?;
    ports
        .attempts
        .stop(&unit_name(act.run.id, number))
        .map_err(UnitError::Attempt)?;
    act.situation(ports.store)?;
    Ok(Outcome::Applied(applied))
}

/// Row N13: the unit exited. Its output is read back through the adapter and
/// the end recorded with the commits, exit, usage and events.
fn end_attempt<S: Store, G: GitHub, A: Attempts>(
    ports: &mut Ports<'_, S, G, A>,
    act: &Act<'_>,
    number: u32,
    exit: Option<i32>,
    cause: Option<AttemptCause>,
) -> Result<Outcome, UnitError<S::Error>> {
    let run = act.run;
    let recorded = act
        .rows
        .attempt(number)
        .ok_or(UnitError::Internal("an exited attempt with no record"))?;
    let brief = act.rows.brief(number).map(|b| &b.brief);
    let field = |key: &str| {
        brief
            .and_then(|b| b.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    let spec = AttemptSpec {
        unit: recorded.started.unit.clone(),
        attempt: number,
        model: recorded.started.model.clone(),
        runtime_bound: recorded.started.runtime_bound,
    };
    let dir = ports.attempts.dir(run.id, number);
    let output = ports
        .attempts
        .output(&dir, field("base_sha"), field("branch"), exit)
        .map_err(UnitError::Attempt)?;
    let result = adapter(recorded.started.agent)?.result(&spec, &output);
    let payload = json!({
        "situation": act.decision.situation,
        "attempt": number,
        "exit": exit,
        "cause": cause,
        "commits": result.commits,
        "usage": result.usage,
        "events": result.events,
    });
    let write = Write::EndAttempt {
        loop_id: run.id,
        attempt: number,
        end: AttemptEnd {
            at: act.now,
            exit,
            cause,
        },
    };
    let applied = act.write(ports.store, &write, &payload)?;
    act.situation(ports.store)?;
    Ok(Outcome::Applied(applied))
}

/// The store write that performs a decision, if it writes anything.
fn plan<E: std::error::Error + 'static>(
    rows: &IssueRows,
    decision: &Decision,
    world: Option<&WorldSeen>,
) -> Result<Option<Write>, UnitError<E>> {
    let requests = || rows.requests.iter().map(|r| r.id).collect();
    Ok(match decision.action {
        Action::Wait {} => rows.run.as_ref().map(|run| Write::Situation {
            loop_id: run.id,
            situation: decision.situation,
        }),
        Action::OpenLoop {} => {
            let Some(oldest) = rows.requests.first() else {
                return Ok(None);
            };
            // Row 11 opens only on a world with a lockfile and a resolved
            // ticket; anything else is a bug in the table.
            let ticket = match world {
                Some(WorldSeen {
                    branch,
                    sha,
                    lockfile: Some(lockfile),
                    ticket: TicketSeen::Resolved { id, path },
                }) => LoopTicket {
                    id: id.clone(),
                    path: path.clone(),
                    branch: branch.clone(),
                    sha: sha.clone(),
                    lockfile: lockfile.clone(),
                },
                _ => return Err(UnitError::Internal("open loop without a resolved ticket")),
            };
            Some(Write::OpenLoop {
                repo: rows.repo.clone(),
                issue: rows.issue,
                agent: oldest.agent,
                requests: requests(),
                situation: decision.situation,
                ticket,
            })
        }
        Action::Refuse { .. } => Some(Write::Refuse {
            repo: rows.repo.clone(),
            issue: rows.issue,
            requests: requests(),
        }),
        // An issue with neither a request nor a loop is never grouped, so a
        // park always has a loop.
        Action::Park { reason } => rows.run.as_ref().map(|run| Write::Park {
            loop_id: run.id,
            reason,
            situation: decision.situation,
        }),
        // Performed by their own steps, on a loop; an issue with none has
        // no attempt to act on.
        Action::StartAttempt { .. } | Action::StopAttempt { .. } | Action::EndAttempt { .. } => {
            return Err(UnitError::Internal(
                "an attempt action on an issue with no loop",
            ));
        }
    })
}

/// What an event records beside its kind.
#[derive(Serialize)]
struct Payload<'a> {
    situation: Situation,
    #[serde(skip_serializing_if = "Option::is_none")]
    refusal: Option<Refusal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<ParkReason>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    requests: Vec<i64>,
    /// The label events behind the requests that came from a label.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    labels: Vec<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<rsb_gauge_reconcile::store::Agent>,
    /// The `repos.toml` the role came from, with its commit.
    #[serde(skip_serializing_if = "Option::is_none")]
    registry: Option<&'a Source>,
    /// What GitHub showed: the default branch at its commit, the lockfile's
    /// hash, the ticket.
    #[serde(skip_serializing_if = "Option::is_none")]
    world: Option<&'a WorldSeen>,
    /// The whole observation, on a park.
    #[serde(skip_serializing_if = "Option::is_none")]
    observation: Option<&'a Observation>,
}

fn payload(
    rows: &IssueRows,
    decision: &Decision,
    observation: &Observation,
    source: Option<&Source>,
    world: Option<&WorldSeen>,
) -> Result<serde_json::Value, serde_json::Error> {
    let (refusal, reason) = match decision.action {
        Action::Refuse { refusal } => (Some(refusal), None),
        Action::Park { reason } => (None, Some(reason)),
        Action::Wait {}
        | Action::OpenLoop {}
        | Action::StartAttempt { .. }
        | Action::StopAttempt { .. }
        | Action::EndAttempt { .. } => (None, None),
    };
    // A park carries the whole observation; the request ids and the
    // registry are on it already.
    let parked = matches!(decision.action, Action::Park { .. });
    serde_json::to_value(Payload {
        situation: decision.situation,
        refusal,
        reason,
        requests: if parked {
            Vec::new()
        } else {
            rows.requests.iter().map(|r| r.id.0).collect()
        },
        labels: if parked {
            Vec::new()
        } else {
            rows.requests.iter().filter_map(|r| r.label).collect()
        },
        agent: matches!(decision.action, Action::OpenLoop {})
            .then(|| rows.requests.first().map(|r| r.agent))
            .flatten(),
        registry: source.filter(|_| !parked),
        world: world.filter(|_| !parked),
        observation: parked.then_some(observation),
    })
}
