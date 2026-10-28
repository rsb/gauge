//! The tick: one run-to-completion pass over every issue with a pending
//! request or an open loop. Per issue it observes, decides, performs the one
//! action and records its event, then moves on. Nothing is held between
//! ticks: every start is a resume. `--observe` is the same pass without the
//! act step.

use rsb_gauge_reconcile::store::{Applied, LoopId, Store, Write};
use rsb_gauge_reconcile::{
    Action, Decision, IssueRows, Observation, ParkReason, Refusal, Situation, decide, issues,
};
use serde::Serialize;

use crate::error::report as report_error;
use crate::registry::{Registry, Source};

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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
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
    #[error("payload")]
    Payload(#[from] serde_json::Error),
    #[error("store")]
    Store(#[source] E),
}

/// Runs one tick. Only a failure to read the snapshot or the trigger fails
/// the tick; a failure inside one issue is caught at its boundary, recorded
/// where the store allows, and counted.
pub fn run<S: Store>(
    store: &mut S,
    registry: &Result<Registry, String>,
    clock: &dyn Clock,
    mode: Mode,
) -> Result<Report, S::Error> {
    let snapshot = store.snapshot()?;
    let mut report = Report::default();
    for rows in issues(&snapshot) {
        let loop_id = rows.run.as_ref().map(|r| r.id);
        let mut decided = None;
        let result = reconcile(
            store,
            &rows,
            &snapshot.flags,
            registry,
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

/// Observes and decides one issue, then acts unless observing. The decision
/// is left in `decided` even when acting fails.
fn reconcile<S: Store>(
    store: &mut S,
    rows: &IssueRows,
    flags: &[rsb_gauge_reconcile::store::Flag],
    registry: &Result<Registry, String>,
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
    let observation = rows.observe(flags, role);
    let decision = decide(&observation);
    *decided = Some(decision);
    if mode == Mode::Observe {
        return Ok(Outcome::Observed);
    }
    let Some(write) = plan(rows, &decision) else {
        return Ok(Outcome::Waited);
    };
    let payload = payload(rows, &decision, &observation, source)?;
    match store
        .write(&write, clock.now(), &payload)
        .map_err(UnitError::Store)?
    {
        Applied::Noop if matches!(decision.action, Action::Wait {}) => Ok(Outcome::Waited),
        applied => Ok(Outcome::Applied(applied)),
    }
}

/// The store write that performs a decision, if it writes anything.
fn plan(rows: &IssueRows, decision: &Decision) -> Option<Write> {
    let requests = || rows.requests.iter().map(|r| r.id).collect();
    match decision.action {
        Action::Wait {} => rows.run.as_ref().map(|run| Write::Situation {
            loop_id: run.id,
            situation: decision.situation,
        }),
        Action::OpenLoop {} => {
            let oldest = rows.requests.first()?;
            Some(Write::OpenLoop {
                repo: rows.repo.clone(),
                issue: rows.issue,
                agent: oldest.agent,
                requests: requests(),
                situation: decision.situation,
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
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<rsb_gauge_reconcile::store::Agent>,
    /// The `repos.toml` the role came from, with its commit.
    #[serde(skip_serializing_if = "Option::is_none")]
    registry: Option<&'a Source>,
    /// The whole observation, on a park.
    #[serde(skip_serializing_if = "Option::is_none")]
    observation: Option<&'a Observation>,
}

fn payload(
    rows: &IssueRows,
    decision: &Decision,
    observation: &Observation,
    source: Option<&Source>,
) -> Result<serde_json::Value, serde_json::Error> {
    let (refusal, reason) = match decision.action {
        Action::Refuse { refusal } => (Some(refusal), None),
        Action::Park { reason } => (None, Some(reason)),
        Action::Wait {} | Action::OpenLoop {} => (None, None),
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
        agent: matches!(decision.action, Action::OpenLoop {})
            .then(|| rows.requests.first().map(|r| r.agent))
            .flatten(),
        registry: source.filter(|_| !parked),
        observation: parked.then_some(observation),
    })
}
