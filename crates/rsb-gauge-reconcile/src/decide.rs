//! The situation table and the pure `decide`. Rows are tried in order and
//! the first match wins; the order is part of the contract (spec S-0001, the
//! M1 situation table). Each row carries its order and number from that table
//! so later tickets insert theirs in place.

use std::fmt;

use rsb_gauge_schema::Role;
use serde::{Deserialize, Serialize};

use crate::observation::{IssueObservation, Observation, TicketCause, TicketSeen};
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
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wait {} => f.write_str("wait"),
            Self::OpenLoop {} => f.write_str("open loop"),
            Self::Refuse { refusal } => write!(f, "refuse: {refusal}"),
            Self::Park { reason } => write!(f, "park {reason}"),
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
/// Row 25 (14, drain) is a gate on rows 22 to 24, which start attempts; none
/// exists before S-0001 ticket 06, so drain changes no decision yet.
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
/// once its repo has a lockfile and the issue resolves to a ticket file.
fn run_requested(issue: &IssueObservation) -> Option<Decision> {
    let world = issue.request.as_ref()?.world.as_ref()?;
    world.lockfile.as_ref()?;
    if !matches!(world.ticket, TicketSeen::Resolved { .. }) {
        return None;
    }
    Some(Decision {
        situation: Situation::RunRequested,
        action: Action::OpenLoop {},
    })
}
