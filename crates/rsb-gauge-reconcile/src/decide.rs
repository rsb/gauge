//! The situation table and the pure `decide`. Rows are tried in order and
//! the first match wins; the order is part of the contract (spec S-0001, the
//! M1 situation table). Each row carries its order and number from that table
//! so later tickets insert theirs in place.

use std::fmt;

use rsb_gauge_schema::Role;
use serde::{Deserialize, Serialize};

use crate::observation::{
    AttemptSeen, IssueObservation, LoopSeen, Observation, TicketCause, TicketSeen, UnitSeen,
};
use crate::store::{Agent, AttemptCause, LoopState};
use crate::vocabulary::vocabulary;

vocabulary! {
    /// What the observation says is going on.
    pub enum Situation as "situation" {
        /// The store is missing, locked past its timeout, or fails its
        /// integrity check.
        StoreUnreachable = "store_unreachable",
        /// A pause flag, global or on this loop.
        Paused = "paused",
        /// A run request on a repo that may not host a loop: by its role, or
        /// for want of a lockfile on its default branch.
        Ineligible = "ineligible",
        /// A run request on an issue that names no ticket a loop could run.
        TicketUnresolved = "ticket_unresolved",
        /// A run request on an eligible repo: it opens a loop, or is refused
        /// when the issue already has one that is not closed.
        RunRequested = "run_requested",
        /// An attempt's unit runs within its runtime bound.
        AttemptRunning = "attempt_running",
        /// An attempt's unit runs past its runtime bound, or after its end
        /// was recorded.
        AttemptOverBound = "attempt_over_bound",
        /// An attempt's unit exited and its end is not recorded.
        AttemptExited = "attempt_exited",
        /// An open loop whose current attempt has not started: none yet, or
        /// a brief issued whose attempt is not recorded.
        AttemptDue = "attempt_due",
        /// A drain flag holds an attempt that would start.
        Draining = "draining",
        /// Another attempt's unit runs: an attempt that would start waits
        /// for it.
        AttemptHeld = "attempt_held",
        /// No row matched.
        Unrecognized = "unrecognized",
    }
}

vocabulary! {
    /// Why a loop is parked. Closed; a park is terminal until resume or abort.
    pub enum ParkReason as "park reason" {
        NoProgress = "no_progress",
        AttemptLimit = "attempt_limit",
        UnrecognizedState = "unrecognized_state",
        OutboxRejected = "outbox_rejected",
        EffectRefused = "effect_refused",
        EffectUnverified = "effect_unverified",
        AgentFlagged = "agent_flagged",
        AgentBlocked = "agent_blocked",
    }
}

/// Why a run request is refused. The fieldless reasons are written `{}`, not
/// as unit variants: serde lets an internally tagged unit variant carry
/// unknown fields, and these must refuse them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum Refusal {
    /// The repo is not in the `repos.toml` tied to the store.
    NotListed {},
    /// The repo's role is not `project` or `reference`.
    Role { role: Role },
    /// The driver does not reach the repo on GitHub: its config does not
    /// list it, so nothing of the repo can be seen.
    NotReached {},
    /// The repo's default branch has no `.rsb/governance.lock`: it has not
    /// been minted or aligned.
    NoLockfile {},
    /// The issue names no ticket file on the default branch.
    NoTicket { cause: TicketCause },
    /// The issue already has a loop that is not closed, open or parked.
    LoopAlreadyOpen {},
    /// The request names an agent with no adapter yet: Codex until M4.
    NoAdapter { agent: Agent },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotListed {} => f.write_str("not listed in repos.toml"),
            Self::Role { role } => write!(f, "role {role} may not host a loop"),
            Self::NotReached {} => f.write_str("the driver's config does not list the repo"),
            Self::NoLockfile {} => f.write_str("the default branch has no .rsb/governance.lock"),
            Self::NoTicket { cause } => write!(f, "the issue names no ticket file ({cause})"),
            Self::LoopAlreadyOpen {} => f.write_str("the issue already has an open loop"),
            Self::NoAdapter { agent } => write!(f, "agent {agent} has no adapter yet"),
        }
    }
}

/// The one next step for an issue. The fieldless actions are written `{}`
/// for the reason given on [`Refusal`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Do nothing this tick.
    Wait {},
    OpenLoop {},
    Refuse {
        refusal: Refusal,
    },
    Park {
        reason: ParkReason,
    },
    /// Start attempt `attempt`: issue its brief, start its unit and record
    /// it, each looked up first.
    StartAttempt {
        attempt: u32,
    },
    /// Record the attempt's end with `cause`, then stop its unit.
    StopAttempt {
        attempt: u32,
        cause: AttemptCause,
    },
    /// Record the end of an attempt whose unit exited.
    EndAttempt {
        attempt: u32,
        exit: Option<i32>,
        cause: Option<AttemptCause>,
    },
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wait {} => f.write_str("wait"),
            Self::OpenLoop {} => f.write_str("open loop"),
            Self::Refuse { refusal } => write!(f, "refuse: {refusal}"),
            Self::Park { reason } => write!(f, "park {reason}"),
            Self::StartAttempt { attempt } => write!(f, "start attempt {attempt}"),
            Self::StopAttempt { attempt, cause } => write!(f, "stop attempt {attempt} ({cause})"),
            Self::EndAttempt { attempt, cause, .. } => match cause {
                Some(cause) => write!(f, "end attempt {attempt} ({cause})"),
                None => write!(f, "end attempt {attempt}"),
            },
        }
    }
}

/// A situation and the action it takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Decision {
    pub situation: Situation,
    pub action: Action,
}

/// One row of the table: its place in the spec, and the test it applies.
pub struct Row {
    /// The order in the M1 table. A lettered row (7a) shares its number's
    /// order and comes before it.
    pub order: u8,
    /// The number in the PRD (§3.3) or the spec (`N`-rows).
    pub number: &'static str,
    pub rule: fn(&IssueObservation) -> Option<Decision>,
}

/// The rows this build implements, in precedence order. The store-unreachable
/// row comes before all of them and is not per issue: see [`decide`].
///
/// Row 25 (14, drain) is a gate on rows 22 to 24, which start attempts: row
/// N6 starts nothing while draining, and the drain row then waits. Row N14
/// is the same gate while another attempt's unit runs: M1 runs one attempt
/// at a time, because the attempts share one sign-in whose refresh two
/// boxes would race (agent-box PR A).
///
/// Rows 8, 9, N13 and 24 apply to an open loop only: rows 4 and 5 (parked,
/// ticket 09) come before them, and until then a parked loop falls through
/// to row 28 and stays parked.
pub const ROWS: &[Row] = &[
    Row {
        order: 2,
        number: "13",
        rule: paused,
    },
    Row {
        order: 6,
        number: "N1",
        rule: ineligible,
    },
    Row {
        order: 7,
        number: "N11",
        rule: loop_already_open,
    },
    Row {
        order: 7,
        number: "N12",
        rule: ticket_unresolved,
    },
    Row {
        order: 7,
        number: "11",
        rule: run_requested,
    },
    Row {
        order: 8,
        number: "1",
        rule: attempt_running,
    },
    Row {
        order: 9,
        number: "N2",
        rule: attempt_over_bound,
    },
    Row {
        order: 9,
        number: "N13",
        rule: attempt_exited,
    },
    Row {
        order: 24,
        number: "N6",
        rule: attempt_due,
    },
    Row {
        order: 25,
        number: "14",
        rule: draining,
    },
    Row {
        order: 25,
        number: "N14",
        rule: attempt_held,
    },
];

/// The row that matches when no other does: park `unrecognized_state`.
pub const UNRECOGNIZED: Decision = Decision {
    situation: Situation::Unrecognized,
    action: Action::Park {
        reason: ParkReason::UnrecognizedState,
    },
};

/// The one next action for an observation. Pure: the same observation always
/// gives the same decision.
pub fn decide(observation: &Observation) -> Decision {
    match observation {
        Observation::StoreUnreachable => Decision {
            situation: Situation::StoreUnreachable,
            action: Action::Wait {},
        },
        Observation::Issue(issue) => ROWS
            .iter()
            .find_map(|row| (row.rule)(issue))
            .unwrap_or(UNRECOGNIZED),
    }
}

fn paused(issue: &IssueObservation) -> Option<Decision> {
    issue.paused.then_some(Decision {
        situation: Situation::Paused,
        action: Action::Wait {},
    })
}

/// Row N1: by role from `repos.toml`, then by the lockfile on the default
/// branch, "the lockfile is the mechanical test" (record 1, 1a). A repo
/// eligible by role that the driver does not reach has no world seen, and
/// cannot host a loop either.
fn ineligible(issue: &IssueObservation) -> Option<Decision> {
    let request = issue.request.as_ref()?;
    let refusal = match request.role {
        None => Refusal::NotListed {},
        Some(role) if !role.is_eligible() => Refusal::Role { role },
        Some(_) => match &request.world {
            None => Refusal::NotReached {},
            Some(world) if world.lockfile.is_none() => Refusal::NoLockfile {},
            Some(_) => return None,
        },
    };
    Some(Decision {
        situation: Situation::Ineligible,
        action: Action::Refuse { refusal },
    })
}

/// Row N11: a request on an issue whose loop is not closed, open or parked,
/// is refused and the loop left as it is, so a second request can neither
/// open a second loop nor disturb the first.
fn loop_already_open(issue: &IssueObservation) -> Option<Decision> {
    issue.request.as_ref()?;
    issue.run.as_ref()?;
    Some(Decision {
        situation: Situation::RunRequested,
        action: Action::Refuse {
            refusal: Refusal::LoopAlreadyOpen {},
        },
    })
}

/// Row N12: a request on an issue that names no ticket file is refused, not
/// parked: no loop exists yet to park.
fn ticket_unresolved(issue: &IssueObservation) -> Option<Decision> {
    let world = issue.request.as_ref()?.world.as_ref()?;
    let TicketSeen::Unresolved { cause } = world.ticket else {
        return None;
    };
    Some(Decision {
        situation: Situation::TicketUnresolved,
        action: Action::Refuse {
            refusal: Refusal::NoTicket { cause },
        },
    })
}

/// Row 11: a request on an issue with no loop that is not closed opens one,
/// once its repo has a lockfile and the issue resolves to a ticket file. A
/// request for an agent with no adapter (Codex, until M4) is refused here,
/// so no loop opens that no attempt could run.
fn run_requested(issue: &IssueObservation) -> Option<Decision> {
    let request = issue.request.as_ref()?;
    let world = request.world.as_ref()?;
    world.lockfile.as_ref()?;
    if !matches!(world.ticket, TicketSeen::Resolved { .. }) {
        return None;
    }
    let action = match request.agent {
        Agent::Claude => Action::OpenLoop {},
        agent @ Agent::Codex => Action::Refuse {
            refusal: Refusal::NoAdapter { agent },
        },
    };
    Some(Decision {
        situation: Situation::RunRequested,
        action,
    })
}

/// The open loop and its current attempt, if any.
fn open_loop(issue: &IssueObservation) -> Option<&LoopSeen> {
    issue
        .run
        .as_ref()
        .filter(|run| run.state == LoopState::Open)
}

/// The current attempt of an open loop, recorded and not ended.
fn unended(issue: &IssueObservation) -> Option<&AttemptSeen> {
    let attempt = open_loop(issue)?.attempt.as_ref()?;
    attempt.recorded.as_ref()?.end.is_none().then_some(attempt)
}

/// Row 8 (1): the unit runs within its bound: wait.
fn attempt_running(issue: &IssueObservation) -> Option<Decision> {
    let attempt = unended(issue)?;
    let within = attempt.recorded.as_ref().is_some_and(|r| !r.past_bound);
    (attempt.unit == UnitSeen::Running && within).then_some(Decision {
        situation: Situation::AttemptRunning,
        action: Action::Wait {},
    })
}

/// Row N2: the unit runs past its bound: record `timed_out` and stop it. A
/// unit still running after its end was recorded, by a tick killed between
/// the record and the stop, is stopped with the recorded cause.
fn attempt_over_bound(issue: &IssueObservation) -> Option<Decision> {
    let attempt = open_loop(issue)?.attempt.as_ref()?;
    let recorded = attempt.recorded.as_ref()?;
    if attempt.unit != UnitSeen::Running {
        return None;
    }
    let cause = match &recorded.end {
        None if recorded.past_bound => AttemptCause::TimedOut,
        None => return None,
        Some(end) => end.cause.unwrap_or(AttemptCause::Stopped),
    };
    Some(Decision {
        situation: Situation::AttemptOverBound,
        action: Action::StopAttempt {
            attempt: attempt.number,
            cause,
        },
    })
}

/// Row N13: the unit exited and its end is not recorded: record it, with
/// `exited_nonzero` for a non-zero exit (or a signal) and no sealed outbox.
/// An attempt that exited zero, or left a sealed outbox, has no cause: what
/// it handed back is ticket 07's.
fn attempt_exited(issue: &IssueObservation) -> Option<Decision> {
    let attempt = unended(issue)?;
    let UnitSeen::Exited { code } = attempt.unit else {
        return None;
    };
    let cause = (code != Some(0) && !attempt.sealed).then_some(AttemptCause::ExitedNonzero);
    Some(Decision {
        situation: Situation::AttemptExited,
        action: Action::EndAttempt {
            attempt: attempt.number,
            exit: code,
            cause,
        },
    })
}

/// Row N6: an open loop with no attempt starts attempt 1, and one whose
/// current brief is issued but whose attempt is not recorded (a tick killed
/// between the two) starts it again: the start looks the unit up first.
/// Gated by drain and by another attempt's running unit, unless this
/// attempt's unit is already there to be recorded.
fn attempt_due(issue: &IssueObservation) -> Option<Decision> {
    let run = open_loop(issue)?;
    let gated = issue.draining || issue.another_running;
    let attempt = match &run.attempt {
        None => 1,
        Some(a) if a.recorded.is_none() => {
            if gated && a.unit == UnitSeen::Absent {
                return None;
            }
            a.number
        }
        Some(_) => return None,
    };
    if gated && run.attempt.is_none() {
        return None;
    }
    Some(Decision {
        situation: Situation::AttemptDue,
        action: Action::StartAttempt { attempt },
    })
}

/// Row 25 (14): a drain flag over a loop whose attempt would start: wait.
fn draining(issue: &IssueObservation) -> Option<Decision> {
    open_loop(issue)?;
    issue.draining.then_some(Decision {
        situation: Situation::Draining,
        action: Action::Wait {},
    })
}

/// Row N14: another attempt's unit runs over a loop whose attempt would
/// start: wait. One attempt at a time at M1 (see [`ROWS`]).
fn attempt_held(issue: &IssueObservation) -> Option<Decision> {
    open_loop(issue)?;
    issue.another_running.then_some(Decision {
        situation: Situation::AttemptHeld,
        action: Action::Wait {},
    })
}
