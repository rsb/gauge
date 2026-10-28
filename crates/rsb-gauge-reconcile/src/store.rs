//! The store port: the rows the tick observes and the writes it makes, and
//! the rows the CLI writes for the tick to act on. Every write is one atomic
//! step that looks up its natural key first, so a repeat is a no-op.

use std::fmt;

use serde::Serialize;

use crate::vocabulary::vocabulary;
use crate::{ParkReason, Situation};

vocabulary! {
    /// The agent a loop's attempts run. Claude only until Codex lands at M4.
    pub enum Agent as "agent" {
        Claude = "claude",
        Codex = "codex",
    }
}

vocabulary! {
    /// An owner flag, global or on one loop.
    pub enum FlagKind as "flag" {
        /// Act on nothing.
        Pause = "pause",
        /// Start nothing new; let what runs finish.
        Drain = "drain",
    }
}

vocabulary! {
    /// Where a loop is in its life. A parked loop is still open: at most one
    /// loop that is not closed exists per issue.
    pub enum LoopState as "loop state" {
        Open = "open",
        Parked = "parked",
        Closed = "closed",
    }
}

vocabulary! {
    /// Where a request is. A pending request is unhandled.
    pub enum RequestState as "request state" {
        Pending = "pending",
        /// A loop opened for it.
        Handled = "handled",
        Refused = "refused",
    }
}

vocabulary! {
    /// What an event records: one per action performed.
    pub enum EventKind as "event kind" {
        LoopOpened = "loop_opened",
        RequestRefused = "request_refused",
        LoopParked = "loop_parked",
        /// A reconcile of one issue failed at its boundary.
        ReconcileFailed = "reconcile_failed",
        /// The brief for an attempt, recorded before its unit starts; the
        /// payload is the brief.
        BriefIssued = "brief_issued",
        /// An attempt's unit was started, or found started, and recorded.
        AttemptStarted = "attempt_started",
        /// An attempt's end: its unit exited or was stopped.
        AttemptEnded = "attempt_ended",
    }
}

vocabulary! {
    /// Why an attempt ended without handing back. Closed (spec S-0001,
    /// attempt causes); `stopped` is not counted against the budget.
    pub enum AttemptCause as "attempt cause" {
        /// The unit exited non-zero with no sealed outbox.
        ExitedNonzero = "exited_nonzero",
        /// The attempt ran past its runtime bound and was stopped.
        TimedOut = "timed_out",
        NoOutbox = "no_outbox",
        IntentLost = "intent_lost",
        WorkLost = "work_lost",
        /// Stopped by the owner or by the loop closing.
        Stopped = "stopped",
    }
}

/// A loop's id, the `<loop>` in the unit name `gauge-run-<loop>-<attempt>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct LoopId(pub i64);

impl fmt::Display for LoopId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RequestId(pub i64);

/// An owner's ask to run a ticket issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewRequest {
    pub repo: String,
    pub issue: u64,
    pub agent: Agent,
}

/// A pending run request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: RequestId,
    pub repo: String,
    pub issue: u64,
    pub agent: Agent,
    pub at: i64,
    /// The GitHub event that applied the run label, for a request from a
    /// label; `None` for one from `gauge run`.
    pub label: Option<u64>,
}

/// A request's natural key when it comes from a label: (repo, issue, label
/// event id). The label stays on the issue at M1, so every tick sees it again
/// and only a new event is a new request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelKey {
    pub repo: String,
    pub issue: u64,
    pub event: u64,
}

/// What a loop opened on: the ticket, and the default branch at the commit
/// the ticket and the lockfile were read from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LoopTicket {
    /// `S-NNNN/N`.
    pub id: String,
    /// `specs/S-NNNN-<slug>/NN-<slug>.md`, from the repo's root.
    pub path: String,
    pub branch: String,
    pub sha: String,
    /// The lockfile's SHA-256, in hex.
    pub lockfile: String,
}

impl LoopTicket {
    /// The spec id, `S-NNNN`.
    pub fn spec(&self) -> &str {
        self.id.split_once('/').map_or(&self.id, |(spec, _)| spec)
    }
}

/// One Work Stream Loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loop {
    pub id: LoopId,
    pub repo: String,
    pub issue: u64,
    /// 1 for the first loop on the issue, then 2 and on.
    pub ordinal: u32,
    pub agent: Agent,
    pub state: LoopState,
    pub park: Option<ParkReason>,
    /// The situation the last acting tick derived.
    pub situation: Option<Situation>,
    pub opened_at: i64,
    /// `None` only on a loop opened before tickets were resolved (S-0001
    /// ticket 03).
    pub ticket: Option<LoopTicket>,
}

/// The attempt's unit name, `gauge-run-<loop>-<attempt>`: the natural key
/// of the unit, looked up before one is started.
pub fn unit_name(loop_id: LoopId, attempt: u32) -> String {
    format!("gauge-run-{loop_id}-{attempt}")
}

/// The `rsb-skills` checkout the attempt's skills were mounted from. The
/// box mounts the working tree, so `clean` says whether the tree matched
/// `commit`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SkillsSource {
    pub commit: String,
    pub clean: bool,
}

/// An attempt as recorded when its unit has started.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NewAttempt {
    /// 1 for the loop's first attempt.
    pub number: u32,
    /// `gauge-run-<loop>-<attempt>`.
    pub unit: String,
    pub agent: Agent,
    pub model: String,
    pub started_at: i64,
    /// Seconds the attempt may run before it is stopped.
    pub runtime_bound: u64,
    pub skills: SkillsSource,
}

/// How an attempt ended. `cause` is set only for an attempt that does not
/// hand back; `exit` is the unit's status when it exited by itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AttemptEnd {
    pub at: i64,
    pub exit: Option<i32>,
    pub cause: Option<AttemptCause>,
}

/// A recorded attempt of a loop that is not closed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Attempt {
    pub loop_id: LoopId,
    #[serde(flatten)]
    pub started: NewAttempt,
    pub end: Option<AttemptEnd>,
}

/// A recorded `brief_issued`, keyed by (loop, attempt).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IssuedBrief {
    pub loop_id: LoopId,
    pub attempt: u32,
    pub brief: serde_json::Value,
}

/// A flag on one loop, or on every loop when `scope` is `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flag {
    pub kind: FlagKind,
    pub scope: Option<LoopId>,
}

/// What a tick observes in the store: pending requests, loops that are not
/// closed, every flag, and the trigger count.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub requests: Vec<Request>,
    pub loops: Vec<Loop>,
    pub flags: Vec<Flag>,
    /// Every attempt of the loops in `loops`, by (loop, number).
    pub attempts: Vec<Attempt>,
    /// Every brief issued for the loops in `loops`, by (loop, attempt).
    pub briefs: Vec<IssuedBrief>,
    /// Bumped by every CLI write. A tick that ends on a higher count than it
    /// began on was triggered during its run, so one more tick follows.
    pub trigger: u64,
}

/// A recorded event. `seq` rises with every event and is never reused.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub seq: u64,
    pub at: i64,
    pub kind: EventKind,
    pub repo: Option<String>,
    pub issue: Option<u64>,
    pub loop_id: Option<LoopId>,
    pub payload: serde_json::Value,
}

/// One write by the tick. Each is atomic, and each looks up its natural key
/// before it writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Write {
    /// Open the issue's loop and mark its pending requests handled. A no-op
    /// when none of `requests` is still pending.
    OpenLoop {
        repo: String,
        issue: u64,
        agent: Agent,
        requests: Vec<RequestId>,
        situation: Situation,
        ticket: LoopTicket,
    },
    /// Mark the requests refused. A no-op when none is still pending.
    Refuse {
        repo: String,
        issue: u64,
        requests: Vec<RequestId>,
    },
    /// Park the loop. A no-op when it is already parked for `reason`, the
    /// natural key (loop, reason, attempt) with no attempts yet.
    Park {
        loop_id: LoopId,
        reason: ParkReason,
        situation: Situation,
    },
    /// Record the loop's derived situation, when no other write does. No
    /// event.
    Situation {
        loop_id: LoopId,
        situation: Situation,
    },
    /// Record `brief_issued` for (loop, attempt), with the brief as the
    /// event's payload. A no-op when that brief is already recorded.
    IssueBrief {
        loop_id: LoopId,
        attempt: u32,
        brief: serde_json::Value,
    },
    /// Record the attempt whose unit has started. A no-op when (loop,
    /// number) is already recorded.
    StartAttempt {
        loop_id: LoopId,
        attempt: NewAttempt,
    },
    /// Record the attempt's end. A no-op when it has ended already.
    EndAttempt {
        loop_id: LoopId,
        attempt: u32,
        end: AttemptEnd,
    },
    /// Record that reconciling an issue failed.
    Failure {
        repo: String,
        issue: u64,
        loop_id: Option<LoopId>,
    },
}

impl Write {
    /// The event this write records when it is not a no-op.
    pub fn event_kind(&self) -> Option<EventKind> {
        match self {
            Self::OpenLoop { .. } => Some(EventKind::LoopOpened),
            Self::Refuse { .. } => Some(EventKind::RequestRefused),
            Self::Park { .. } => Some(EventKind::LoopParked),
            Self::Situation { .. } => None,
            Self::IssueBrief { .. } => Some(EventKind::BriefIssued),
            Self::StartAttempt { .. } => Some(EventKind::AttemptStarted),
            Self::EndAttempt { .. } => Some(EventKind::AttemptEnded),
            Self::Failure { .. } => Some(EventKind::ReconcileFailed),
        }
    }
}

/// What a write did.
#[derive(Clone, Debug, PartialEq)]
pub enum Applied {
    /// It changed the store and recorded this event.
    Recorded(Event),
    /// It changed the store and records no event.
    Updated,
    /// Its natural key was already satisfied; nothing changed.
    Noop,
}

/// The store, as the tick and the CLI see it. The SQLite adapter in
/// `rsb-gauge-store` and [`MemoryStore`](crate::MemoryStore) implement it.
pub trait Store {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Everything a tick observes, read at once.
    fn snapshot(&self) -> Result<Snapshot, Self::Error>;

    /// The trigger count alone, read at the end of a tick.
    fn trigger(&self) -> Result<u64, Self::Error>;

    /// Applies one tick write at `at`, with `payload` on its event.
    fn write(
        &mut self,
        write: &Write,
        at: i64,
        payload: &serde_json::Value,
    ) -> Result<Applied, Self::Error>;

    /// Inserts a run request and bumps the trigger.
    fn request(&mut self, request: &NewRequest, at: i64) -> Result<RequestId, Self::Error>;

    /// Inserts a run request from a label, keyed by `key`: `None`, and
    /// nothing written, when a request with that key exists in any state.
    /// The tick calls it, so it does not bump the trigger.
    fn label_request(
        &mut self,
        key: &LabelKey,
        agent: Agent,
        at: i64,
    ) -> Result<Option<RequestId>, Self::Error>;

    /// Whether a request with `key` exists, in any state.
    fn label_requested(&self, key: &LabelKey) -> Result<bool, Self::Error>;

    /// Sets a flag and bumps the trigger. False when it was already set.
    fn flag(&mut self, flag: Flag, at: i64) -> Result<bool, Self::Error>;

    /// Every loop, closed ones included, by id.
    fn loops(&self) -> Result<Vec<Loop>, Self::Error>;

    /// Up to `limit` events after `since`, in sequence order.
    fn events(&self, since: u64, limit: usize) -> Result<Vec<Event>, Self::Error>;
}
