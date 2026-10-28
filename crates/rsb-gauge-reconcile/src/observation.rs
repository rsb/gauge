//! What the tick sees, per issue. The store's rows are grouped by issue, and
//! each group becomes one [`Observation`] for [`decide`](crate::decide).

use std::collections::BTreeMap;

use rsb_gauge_schema::Role;
use serde::Serialize;

use crate::ParkReason;
use crate::store::{
    Agent, Attempt, AttemptEnd, Flag, FlagKind, IssuedBrief, Loop, LoopId, LoopState, Request,
    RequestId, Snapshot,
};
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
    /// A unit `gauge-run-*` other than this loop's current attempt's is
    /// running.
    pub another_running: bool,
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
    /// The loop's current attempt: the highest number with a brief issued
    /// or an attempt recorded. `None` before the first brief.
    pub attempt: Option<AttemptSeen>,
}

/// An attempt's transient unit, `gauge-run-<loop>-<attempt>`, as systemd
/// shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "unit", rename_all = "snake_case")]
pub enum UnitSeen {
    /// No unit of that name is loaded.
    Absent,
    /// Starting, running or stopping.
    Running,
    /// Exited with `code`; `None` when a signal ended it.
    Exited { code: Option<i32> },
}

/// One attempt: its brief, its record, its unit and its outbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AttemptSeen {
    pub number: u32,
    /// `brief_issued` is recorded for it.
    pub brief: bool,
    /// The attempt record, once its unit has started.
    pub recorded: Option<RecordedSeen>,
    pub unit: UnitSeen,
    /// `run/outbox/manifest.json` is on disk: the outbox is sealed.
    pub sealed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RecordedSeen {
    /// The clock is at or past `started_at + runtime_bound`.
    pub past_bound: bool,
    pub end: Option<AttemptEnd>,
}

/// What the driver saw of a loop's current attempt outside the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttemptWorld {
    pub unit: UnitSeen,
    pub sealed: bool,
}

/// The rows of one issue in a snapshot, before its role is looked up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueRows {
    pub repo: String,
    pub issue: u64,
    /// Pending, oldest first.
    pub requests: Vec<Request>,
    pub run: Option<Loop>,
    /// The loop's attempts and briefs, by number.
    pub attempts: Vec<Attempt>,
    pub briefs: Vec<IssuedBrief>,
}

impl IssueRows {
    /// Whether `decide` needs the repo's role: only a request asks for it.
    pub fn needs_role(&self) -> bool {
        !self.requests.is_empty()
    }

    /// The loop's current attempt number, if a brief or an attempt exists.
    pub fn current(&self) -> Option<u32> {
        let attempts = self.attempts.iter().map(|a| a.started.number);
        let briefs = self.briefs.iter().map(|b| b.attempt);
        attempts.chain(briefs).max()
    }

    /// The recorded attempt numbered `number`.
    pub fn attempt(&self, number: u32) -> Option<&Attempt> {
        self.attempts.iter().find(|a| a.started.number == number)
    }

    /// The brief issued for attempt `number`.
    pub fn brief(&self, number: u32) -> Option<&IssuedBrief> {
        self.briefs.iter().find(|b| b.attempt == number)
    }

    /// The observation of this issue under `flags`, with `role` from
    /// `repos.toml`, `world` from GitHub, and `attempt` what the driver saw
    /// of the current attempt's unit and outbox, and `another_running`
    /// whether a unit of any other attempt runs, at `now`.
    pub fn observe(
        &self,
        flags: &[Flag],
        role: Option<Role>,
        world: Option<WorldSeen>,
        attempt: Option<AttemptWorld>,
        another_running: bool,
        now: i64,
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
            another_running,
            request,
            run: self.run.as_ref().map(|run| LoopSeen {
                id: run.id,
                state: run.state,
                park: run.park,
                attempt: self.current().map(|number| {
                    let seen = attempt.unwrap_or(AttemptWorld {
                        unit: UnitSeen::Absent,
                        sealed: false,
                    });
                    AttemptSeen {
                        number,
                        brief: self.brief(number).is_some(),
                        recorded: self.attempt(number).map(|a| RecordedSeen {
                            past_bound: now
                                >= a.started.started_at.saturating_add(
                                    i64::try_from(a.started.runtime_bound).unwrap_or(i64::MAX),
                                ),
                            end: a.end.clone(),
                        }),
                        unit: seen.unit,
                        sealed: seen.sealed,
                    }
                }),
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
                attempts: Vec::new(),
                briefs: Vec::new(),
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
        if let Some(id) = rows.run.as_ref().map(|r| r.id) {
            rows.attempts = snapshot
                .attempts
                .iter()
                .filter(|a| a.loop_id == id)
                .cloned()
                .collect();
            rows.briefs = snapshot
                .briefs
                .iter()
                .filter(|b| b.loop_id == id)
                .cloned()
                .collect();
        }
    }
    out
}
