//! The in-memory store: the fake behind tick tests and the kill harness. Its
//! writes keep the same natural keys as the SQLite adapter.

use crate::ReconcileError;
use crate::store::{
    Agent, Applied, Attempt, Event, Flag, IssuedBrief, LabelKey, Loop, LoopId, LoopState,
    NewRequest, Request, RequestId, RequestState, Snapshot, Store, Write,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct RequestRow {
    request: Request,
    state: RequestState,
    loop_id: Option<LoopId>,
}

/// Every row, in memory. Clone it to compare one store's state with another.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemoryStore {
    requests: Vec<RequestRow>,
    loops: Vec<Loop>,
    flags: Vec<Flag>,
    attempts: Vec<Attempt>,
    briefs: Vec<IssuedBrief>,
    events: Vec<Event>,
    trigger: u64,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn pending(&self, ids: &[RequestId]) -> bool {
        self.requests
            .iter()
            .any(|r| ids.contains(&r.request.id) && r.state == RequestState::Pending)
    }

    fn settle(&mut self, ids: &[RequestId], state: RequestState, loop_id: Option<LoopId>) {
        for row in self.requests.iter_mut() {
            if ids.contains(&row.request.id) && row.state == RequestState::Pending {
                row.state = state;
                row.loop_id = loop_id;
            }
        }
    }

    fn record(
        &mut self,
        write: &Write,
        at: i64,
        repo: &str,
        issue: u64,
        loop_id: Option<LoopId>,
        payload: &serde_json::Value,
    ) -> Applied {
        let Some(kind) = write.event_kind() else {
            return Applied::Updated;
        };
        let event = Event {
            seq: self.events.last().map_or(1, |e| e.seq + 1),
            at,
            kind,
            repo: Some(repo.to_owned()),
            issue: Some(issue),
            loop_id,
            payload: payload.clone(),
        };
        self.events.push(event.clone());
        Applied::Recorded(event)
    }

    fn insert(&mut self, request: Request) -> RequestId {
        let id = request.id;
        self.requests.push(RequestRow {
            request,
            state: RequestState::Pending,
            loop_id: None,
        });
        id
    }

    fn next_request(&self) -> RequestId {
        RequestId(self.requests.last().map_or(1, |r| r.request.id.0 + 1))
    }

    fn loop_mut(&mut self, id: LoopId) -> Result<&mut Loop, ReconcileError> {
        self.loops
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or(ReconcileError::NoLoop(id))
    }

    fn loop_key(&self, id: LoopId) -> Result<(String, u64), ReconcileError> {
        self.loops
            .iter()
            .find(|l| l.id == id)
            .map(|l| (l.repo.clone(), l.issue))
            .ok_or(ReconcileError::NoLoop(id))
    }

    fn not_closed(&self, id: LoopId) -> bool {
        self.loops
            .iter()
            .any(|l| l.id == id && l.state != LoopState::Closed)
    }
}

impl Store for MemoryStore {
    type Error = ReconcileError;

    fn snapshot(&self) -> Result<Snapshot, Self::Error> {
        let mut attempts: Vec<Attempt> = self
            .attempts
            .iter()
            .filter(|a| self.not_closed(a.loop_id))
            .cloned()
            .collect();
        attempts.sort_by_key(|a| (a.loop_id, a.started.number));
        let mut briefs: Vec<IssuedBrief> = self
            .briefs
            .iter()
            .filter(|b| self.not_closed(b.loop_id))
            .cloned()
            .collect();
        briefs.sort_by_key(|b| (b.loop_id, b.attempt));
        Ok(Snapshot {
            requests: self
                .requests
                .iter()
                .filter(|r| r.state == RequestState::Pending)
                .map(|r| r.request.clone())
                .collect(),
            loops: self
                .loops
                .iter()
                .filter(|l| l.state != LoopState::Closed)
                .cloned()
                .collect(),
            flags: self.flags.clone(),
            attempts,
            briefs,
            trigger: self.trigger,
        })
    }

    fn trigger(&self) -> Result<u64, Self::Error> {
        Ok(self.trigger)
    }

    fn write(
        &mut self,
        write: &Write,
        at: i64,
        payload: &serde_json::Value,
    ) -> Result<Applied, Self::Error> {
        match write {
            Write::OpenLoop {
                repo,
                issue,
                agent,
                requests,
                situation,
                ticket,
            } => {
                let open = self
                    .loops
                    .iter()
                    .any(|l| &l.repo == repo && l.issue == *issue && l.state != LoopState::Closed);
                if open || !self.pending(requests) {
                    return Ok(Applied::Noop);
                }
                let id = LoopId(self.loops.last().map_or(1, |l| l.id.0 + 1));
                let ordinal = self
                    .loops
                    .iter()
                    .filter(|l| &l.repo == repo && l.issue == *issue)
                    .count() as u32
                    + 1;
                self.loops.push(Loop {
                    id,
                    repo: repo.clone(),
                    issue: *issue,
                    ordinal,
                    agent: *agent,
                    state: LoopState::Open,
                    park: None,
                    situation: Some(*situation),
                    opened_at: at,
                    ticket: Some(ticket.clone()),
                });
                self.settle(requests, RequestState::Handled, Some(id));
                Ok(self.record(write, at, repo, *issue, Some(id), payload))
            }
            Write::Refuse {
                repo,
                issue,
                requests,
            } => {
                if !self.pending(requests) {
                    return Ok(Applied::Noop);
                }
                self.settle(requests, RequestState::Refused, None);
                Ok(self.record(write, at, repo, *issue, None, payload))
            }
            Write::Park {
                loop_id,
                reason,
                situation,
            } => {
                let run = self.loop_mut(*loop_id)?;
                if run.state == LoopState::Parked && run.park == Some(*reason) {
                    return Ok(Applied::Noop);
                }
                run.state = LoopState::Parked;
                run.park = Some(*reason);
                run.situation = Some(*situation);
                let (repo, issue) = (run.repo.clone(), run.issue);
                Ok(self.record(write, at, &repo, issue, Some(*loop_id), payload))
            }
            Write::Situation { loop_id, situation } => {
                let run = self.loop_mut(*loop_id)?;
                if run.situation == Some(*situation) {
                    return Ok(Applied::Noop);
                }
                run.situation = Some(*situation);
                Ok(Applied::Updated)
            }
            Write::IssueBrief {
                loop_id,
                attempt,
                brief,
            } => {
                let (repo, issue) = self.loop_key(*loop_id)?;
                if self
                    .briefs
                    .iter()
                    .any(|b| b.loop_id == *loop_id && b.attempt == *attempt)
                {
                    return Ok(Applied::Noop);
                }
                self.briefs.push(IssuedBrief {
                    loop_id: *loop_id,
                    attempt: *attempt,
                    brief: brief.clone(),
                });
                Ok(self.record(write, at, &repo, issue, Some(*loop_id), payload))
            }
            Write::StartAttempt { loop_id, attempt } => {
                let (repo, issue) = self.loop_key(*loop_id)?;
                if self
                    .attempts
                    .iter()
                    .any(|a| a.loop_id == *loop_id && a.started.number == attempt.number)
                {
                    return Ok(Applied::Noop);
                }
                self.attempts.push(Attempt {
                    loop_id: *loop_id,
                    started: attempt.clone(),
                    end: None,
                });
                Ok(self.record(write, at, &repo, issue, Some(*loop_id), payload))
            }
            Write::EndAttempt {
                loop_id,
                attempt,
                end,
            } => {
                let (repo, issue) = self.loop_key(*loop_id)?;
                let row = self
                    .attempts
                    .iter_mut()
                    .find(|a| a.loop_id == *loop_id && a.started.number == *attempt)
                    .ok_or(ReconcileError::NoAttempt(*loop_id, *attempt))?;
                if row.end.is_some() {
                    return Ok(Applied::Noop);
                }
                row.end = Some(end.clone());
                Ok(self.record(write, at, &repo, issue, Some(*loop_id), payload))
            }
            Write::Failure {
                repo,
                issue,
                loop_id,
            } => Ok(self.record(write, at, repo, *issue, *loop_id, payload)),
        }
    }

    fn request(&mut self, request: &NewRequest, at: i64) -> Result<RequestId, Self::Error> {
        let id = self.insert(Request {
            id: self.next_request(),
            repo: request.repo.clone(),
            issue: request.issue,
            agent: request.agent,
            at,
            label: None,
        });
        self.trigger += 1;
        Ok(id)
    }

    fn label_request(
        &mut self,
        key: &LabelKey,
        agent: Agent,
        at: i64,
    ) -> Result<Option<RequestId>, Self::Error> {
        if self.label_requested(key)? {
            return Ok(None);
        }
        Ok(Some(self.insert(Request {
            id: self.next_request(),
            repo: key.repo.clone(),
            issue: key.issue,
            agent,
            at,
            label: Some(key.event),
        })))
    }

    fn label_requested(&self, key: &LabelKey) -> Result<bool, Self::Error> {
        Ok(self.requests.iter().any(|r| {
            r.request.repo == key.repo
                && r.request.issue == key.issue
                && r.request.label == Some(key.event)
        }))
    }

    fn flag(&mut self, flag: Flag, _at: i64) -> Result<bool, Self::Error> {
        if let Some(id) = flag.scope {
            self.loop_mut(id)?;
        }
        if self.flags.contains(&flag) {
            return Ok(false);
        }
        self.flags.push(flag);
        self.trigger += 1;
        Ok(true)
    }

    fn loops(&self) -> Result<Vec<Loop>, Self::Error> {
        Ok(self.loops.clone())
    }

    fn events(&self, since: u64, limit: usize) -> Result<Vec<Event>, Self::Error> {
        Ok(self
            .events
            .iter()
            .filter(|e| e.seq > since)
            .take(limit)
            .cloned()
            .collect())
    }
}
