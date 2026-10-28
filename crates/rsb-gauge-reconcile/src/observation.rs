//! What the tick sees, per issue. The store's rows are grouped by issue, and
//! each group becomes one [`Observation`] for [`decide`](crate::decide).

use std::collections::BTreeMap;

use rsb_gauge_schema::Role;
use serde::Serialize;

use crate::ParkReason;
use crate::store::{Agent, Flag, FlagKind, Loop, LoopId, LoopState, Request, RequestId, Snapshot};
use crate::vocabulary::vocabulary;

/// One input to `decide`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "observed", rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "one observation per issue per tick, built and dropped in place"
)]
pub enum Observation {
    /// The store could not be opened: nothing else can be seen.
    StoreUnreachable,
    Issue(IssueObservation),
}

/// One issue: its pending run requests, its loop, and the flags over it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IssueObservation {
    pub repo: String,
    pub issue: u64,
    /// A pause flag, global or on this loop.
    pub paused: bool,
    /// A drain flag, global or on this loop.
    pub draining: bool,
    pub request: Option<RequestSeen>,
    /// The loop that is not closed, if any.
    pub run: Option<LoopSeen>,
}

/// The issue's pending run requests, taken together.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RequestSeen {
    /// Oldest first.
    pub ids: Vec<RequestId>,
    /// The oldest request's agent.
    pub agent: Agent,
    /// The repo's role in `repos.toml`; `None` when it is not listed.
    pub role: Option<Role>,
    /// What GitHub shows for the issue and its repo's default branch. Read
    /// only for a repo whose role is eligible and that the driver reaches;
    /// `None` otherwise.
    pub world: Option<WorldSeen>,
}

/// The repo's default branch at one commit, and the issue read against it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorldSeen {
    /// The default branch's name.
    pub branch: String,
    /// The commit the branch named when it was read. The lockfile and the
    /// ticket file are read at this commit.
    pub sha: String,
    /// The SHA-256 of `.rsb/governance.lock` at `sha`, in hex; `None` when
    /// the file is absent. Recorded, not validated (M3).
    pub lockfile: Option<String>,
    pub ticket: TicketSeen,
}

/// The ticket the issue's body names, resolved to its file at the commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "ticket", rename_all = "snake_case")]
pub enum TicketSeen {
    /// `id` as `S-NNNN/N`; `path` as `specs/S-NNNN-<slug>/NN-<slug>.md`.
    Resolved {
        id: String,
        path: String,
    },
    Unresolved {
        cause: TicketCause,
    },
}

vocabulary! {
    /// Why an issue names no ticket a loop could run.
    pub enum TicketCause as "ticket cause" {
        /// No issue has the number.
        IssueMissing = "issue_missing",
        IssueClosed = "issue_closed",
        /// The number is a pull request's.
        PullRequest = "pull_request",
        /// The body carries no `<!-- rsb-ticket S-NNNN/N -->` marker.
        NoMarker = "no_marker",
        /// The body carries only the spec marker: an epic, whose `run` is M5.
        Epic = "epic",
        /// The body names more than one ticket.
        TwoTickets = "two_tickets",
        /// The default branch has no `specs/S-NNNN-<slug>/` for the spec, or
        /// more than one.
        NoSpecFolder = "no_spec_folder",
        /// The spec folder has no `NN-<slug>.md` for the ticket, or more
        /// than one.
        NoTicketFile = "no_ticket_file",
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LoopSeen {
    pub id: LoopId,
    pub state: LoopState,
    pub park: Option<ParkReason>,
}

/// The rows of one issue in a snapshot, before its role is looked up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueRows {
    pub repo: String,
    pub issue: u64,
    /// Pending, oldest first.
    pub requests: Vec<Request>,
    pub run: Option<Loop>,
}

impl IssueRows {
    /// Whether `decide` needs the repo's role: only a request asks for it.
    pub fn needs_role(&self) -> bool {
        !self.requests.is_empty()
    }

    /// The observation of this issue under `flags`, with `role` from
    /// `repos.toml` and `world` from GitHub.
    pub fn observe(
        &self,
        flags: &[Flag],
        role: Option<Role>,
        world: Option<WorldSeen>,
    ) -> Observation {
        let flagged = |kind| {
            flags.iter().any(|f| {
                f.kind == kind
                    && match f.scope {
                        None => true,
                        Some(id) => self.run.as_ref().is_some_and(|run| run.id == id),
                    }
            })
        };
        let request = self.requests.first().map(|oldest| RequestSeen {
            ids: self.requests.iter().map(|r| r.id).collect(),
            agent: oldest.agent,
            role,
            world,
        });
        Observation::Issue(IssueObservation {
            repo: self.repo.clone(),
            issue: self.issue,
            paused: flagged(FlagKind::Pause),
            draining: flagged(FlagKind::Drain),
            request,
            run: self.run.as_ref().map(|run| LoopSeen {
                id: run.id,
                state: run.state,
                park: run.park,
            }),
        })
    }
}

/// Groups a snapshot's pending requests and open loops by issue, in (repo,
/// issue) order. Closed loops are not in a snapshot.
pub fn issues(snapshot: &Snapshot) -> Vec<IssueRows> {
    let mut by_issue: BTreeMap<(String, u64), IssueRows> = BTreeMap::new();
    fn entry<'a>(
        by_issue: &'a mut BTreeMap<(String, u64), IssueRows>,
        repo: &str,
        issue: u64,
    ) -> &'a mut IssueRows {
        by_issue
            .entry((repo.to_owned(), issue))
            .or_insert_with(|| IssueRows {
                repo: repo.to_owned(),
                issue,
                requests: Vec::new(),
                run: None,
            })
    }
    for run in snapshot
        .loops
        .iter()
        .filter(|l| l.state != LoopState::Closed)
    {
        entry(&mut by_issue, &run.repo, run.issue).run = Some(run.clone());
    }
    for request in &snapshot.requests {
        entry(&mut by_issue, &request.repo, request.issue)
            .requests
            .push(request.clone());
    }
    let mut out: Vec<IssueRows> = by_issue.into_values().collect();
    for rows in &mut out {
        rows.requests.sort_by_key(|r| r.id);
    }
    out
}
