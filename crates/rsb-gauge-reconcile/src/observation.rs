//! What the tick sees, per issue. The store's rows are grouped by issue, and
//! each group becomes one [`Observation`] for [`decide`](crate::decide).

use std::collections::BTreeMap;

use rsb_gauge_schema::Role;
use serde::Serialize;

use crate::ParkReason;
use crate::store::{Agent, Flag, FlagKind, Loop, LoopId, LoopState, Request, RequestId, Snapshot};

/// One input to `decide`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "observed", rename_all = "snake_case")]
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
    /// `repos.toml`.
    pub fn observe(&self, flags: &[Flag], role: Option<Role>) -> Observation {
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
