//! The situation table and the pure `decide`. Rows are tried in order and
//! the first match wins; the order is part of the contract (spec S-0001, the
//! M1 situation table). Each row carries its order and number from that table
//! so later tickets insert theirs in place.

use std::fmt;

use rsb_gauge_schema::Role;
use serde::{Deserialize, Serialize};

use crate::observation::{IssueObservation, Observation};
use crate::vocabulary::vocabulary;

vocabulary! {
    /// What the observation says is going on.
    pub enum Situation as "situation" {
        /// The store is missing, locked past its timeout, or fails its
        /// integrity check.
        StoreUnreachable = "store_unreachable",
        /// A pause flag, global or on this loop.
        Paused = "paused",
        /// A run request on a repo that may not host a loop.
        Ineligible = "ineligible",
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
    /// The issue already has a loop that is not closed, open or parked.
    LoopAlreadyOpen {},
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotListed {} => f.write_str("not listed in repos.toml"),
            Self::Role { role } => write!(f, "role {role} may not host a loop"),
            Self::LoopAlreadyOpen {} => f.write_str("the issue already has an open loop"),
        }
    }
}

/// The one next step for an issue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Do nothing this tick.
    Wait,
    OpenLoop,
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
            Self::Wait => f.write_str("wait"),
            Self::OpenLoop => f.write_str("open loop"),
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
            action: Action::Wait,
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
        action: Action::Wait,
    })
}

fn ineligible(issue: &IssueObservation) -> Option<Decision> {
    let request = issue.request.as_ref()?;
    let refusal = match request.role {
        None => Refusal::NotListed {},
        Some(role) if !role.is_eligible() => Refusal::Role { role },
        Some(_) => return None,
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

/// Row 11: a request on an issue with no loop that is not closed opens one.
fn run_requested(issue: &IssueObservation) -> Option<Decision> {
    issue.request.as_ref()?;
    Some(Decision {
        situation: Situation::RunRequested,
        action: Action::OpenLoop,
    })
}
