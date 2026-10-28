//! The store as a SQLite file: the one implementation of the
//! `rsb-gauge-reconcile` store port at M1. The file is named by `GAUGE_STORE`
//! and nothing defaults it. WAL mode and a busy timeout let the CLI write a
//! request while a tick runs. The schema is versioned migrations applied on
//! open, in SQL that Cloudflare's SQLite also runs.

mod error;

use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rsb_gauge_reconcile::store::{
    Agent, Applied, Event, EventKind, Flag, FlagKind, LabelKey, Loop, LoopId, LoopState,
    LoopTicket, NewRequest, Request, RequestId, RequestState, Snapshot, Store, Write,
};
use rsb_gauge_reconcile::{ParkReason, ReconcileError, Situation};
use rusqlite::{Connection, OpenFlags, OptionalExtension as _, Transaction, TransactionBehavior};

pub use error::StoreError;

/// The environment variable that names the store file.
pub const ENV: &str = "GAUGE_STORE";

/// How long a write waits on another writer before the store counts as
/// unreachable.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// The migrations, in version order. A version is never edited once shipped.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_loops.sql")),
    (2, include_str!("../migrations/0002_world.sql")),
];

/// The setting that ties the store to the `repos.toml` whose repos its ticks
/// may open loops on.
const REPOS: &str = "repos";

/// The store file named by `GAUGE_STORE`.
pub fn path_from_env() -> Result<PathBuf, StoreError> {
    match std::env::var_os(ENV) {
        Some(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        _ => Err(StoreError::NoPath),
    }
}

/// An open store file.
pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Creates the store at `path`, mode 600, applies every migration and ties
    /// it to the `repos.toml` at `repos`. Refuses a path that exists.
    pub fn create(path: &Path, repos: &Path) -> Result<Self, StoreError> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|source| match source.kind() {
                std::io::ErrorKind::AlreadyExists => StoreError::Exists(path.to_path_buf()),
                _ => StoreError::Create {
                    path: path.to_path_buf(),
                    source,
                },
            })?;
        let store = Self::open(path)?;
        store.conn.execute(
            "INSERT INTO setting (key, value) VALUES (?1, ?2)",
            (REPOS, repos.to_string_lossy()),
        )?;
        Ok(store)
    }

    /// Opens the store at `path` and applies any migration it lacks. A missing
    /// file, a lock held past [`BUSY_TIMEOUT`] or a failed integrity check is
    /// unreachable: see [`StoreError::is_unreachable`].
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::open_with(path, BUSY_TIMEOUT)
    }

    /// [`open`](Self::open) with another busy timeout.
    pub fn open_with(path: &Path, timeout: Duration) -> Result<Self, StoreError> {
        if !path.is_file() {
            return Err(StoreError::Missing(path.to_path_buf()));
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(timeout)?;
        let check: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if check != "ok" {
            return Err(StoreError::Corrupt(check));
        }
        conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        })?;
        conn.pragma_update(None, "foreign_keys", true)?;
        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    /// The schema version of the open file.
    pub fn version(&self) -> Result<i64, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT coalesce(max(version), 0) FROM migration", [], |r| {
                r.get(0)
            })?)
    }

    /// The `repos.toml` this store's ticks read roles from.
    pub fn repos(&self) -> Result<PathBuf, StoreError> {
        self.conn
            .query_row("SELECT value FROM setting WHERE key = ?1", [REPOS], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .map(PathBuf::from)
            .ok_or(StoreError::NoSetting(REPOS))
    }

    fn migrate(&mut self) -> Result<(), StoreError> {
        let known = MIGRATIONS.last().map_or(0, |(v, _)| *v);
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS migration (
                version INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
            )",
        )?;
        let found: i64 =
            tx.query_row("SELECT coalesce(max(version), 0) FROM migration", [], |r| {
                r.get(0)
            })?;
        if found > known {
            return Err(StoreError::Newer { found, known });
        }
        for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > found) {
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO migration (version, applied_at) VALUES (?1, CAST(strftime('%s', 'now') AS INTEGER))",
                [version],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn immediate(&mut self) -> Result<Transaction<'_>, StoreError> {
        Ok(self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?)
    }
}

fn int(what: &'static str, value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::Range {
        what,
        value: value.to_string(),
    })
}

fn uint(what: &'static str, value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::Range {
        what,
        value: value.to_string(),
    })
}

fn parse<T: std::str::FromStr<Err = ReconcileError>>(text: &str) -> Result<T, StoreError> {
    Ok(text.parse()?)
}

fn parse_opt<T: std::str::FromStr<Err = ReconcileError>>(
    text: Option<String>,
) -> Result<Option<T>, StoreError> {
    text.map(|t| parse(&t)).transpose()
}

/// Whether any of `ids` is still pending.
fn any_pending(tx: &Transaction<'_>, ids: &[RequestId]) -> Result<bool, StoreError> {
    for id in ids {
        let state: Option<String> = tx
            .query_row("SELECT state FROM request WHERE id = ?1", [id.0], |r| {
                r.get(0)
            })
            .optional()?;
        if state.as_deref() == Some(RequestState::Pending.as_str()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn settle(
    tx: &Transaction<'_>,
    ids: &[RequestId],
    state: RequestState,
    loop_id: Option<LoopId>,
    at: i64,
) -> Result<(), StoreError> {
    for id in ids {
        tx.execute(
            "UPDATE request SET state = ?1, loop_id = ?2, settled_at = ?3
             WHERE id = ?4 AND state = ?5",
            (
                state.as_str(),
                loop_id.map(|l| l.0),
                at,
                id.0,
                RequestState::Pending.as_str(),
            ),
        )?;
    }
    Ok(())
}

fn record(
    tx: &Transaction<'_>,
    kind: EventKind,
    at: i64,
    repo: &str,
    issue: u64,
    loop_id: Option<LoopId>,
    payload: &serde_json::Value,
) -> Result<Event, StoreError> {
    tx.execute(
        "INSERT INTO event (at, kind, repo, issue, loop_id, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        (
            at,
            kind.as_str(),
            repo,
            int("issue", issue)?,
            loop_id.map(|l| l.0),
            serde_json::to_string(payload)?,
        ),
    )?;
    Ok(Event {
        seq: uint("event seq", tx.last_insert_rowid())?,
        at,
        kind,
        repo: Some(repo.to_owned()),
        issue: Some(issue),
        loop_id,
        payload: payload.clone(),
    })
}

type LoopRow = (
    i64,
    String,
    i64,
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    [Option<String>; 5],
);

const LOOP_COLUMNS: &str = "id, repo, issue, ordinal, agent, state, park, situation, opened_at, \
     ticket, ticket_path, base_branch, base_sha, lockfile";

fn loop_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LoopRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        [
            row.get(9)?,
            row.get(10)?,
            row.get(11)?,
            row.get(12)?,
            row.get(13)?,
        ],
    ))
}

fn to_loop(row: LoopRow) -> Result<Loop, StoreError> {
    let (id, repo, issue, ordinal, agent, state, park, situation, opened_at, ticket) = row;
    let ticket = match ticket {
        [None, None, None, None, None] => None,
        [
            Some(id),
            Some(path),
            Some(branch),
            Some(sha),
            Some(lockfile),
        ] => Some(LoopTicket {
            id,
            path,
            branch,
            sha,
            lockfile,
        }),
        _ => return Err(StoreError::PartialTicket(id)),
    };
    Ok(Loop {
        id: LoopId(id),
        repo,
        issue: uint("issue", issue)?,
        ordinal: u32::try_from(ordinal).map_err(|_| StoreError::Range {
            what: "ordinal",
            value: ordinal.to_string(),
        })?,
        agent: parse(&agent)?,
        state: parse(&state)?,
        park: parse_opt::<ParkReason>(park)?,
        situation: parse_opt::<Situation>(situation)?,
        opened_at,
        ticket,
    })
}

fn select_loops(conn: &Connection, filter: &str) -> Result<Vec<Loop>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {LOOP_COLUMNS} FROM loop {filter} ORDER BY id"
    ))?;
    let rows = stmt
        .query_map([], loop_row)?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter().map(to_loop).collect()
}

impl Store for SqliteStore {
    type Error = StoreError;

    fn snapshot(&self) -> Result<Snapshot, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let mut stmt = tx.prepare(
            "SELECT id, repo, issue, agent, created_at, label_event FROM request
             WHERE state = ?1 ORDER BY id",
        )?;
        let raw = stmt
            .query_map([RequestState::Pending.as_str()], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        let requests = raw
            .into_iter()
            .map(|(id, repo, issue, agent, at, label)| {
                Ok(Request {
                    id: RequestId(id),
                    repo,
                    issue: uint("issue", issue)?,
                    agent: parse::<Agent>(&agent)?,
                    at,
                    label: label.map(|l| uint("label event", l)).transpose()?,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        let loops = select_loops(&tx, "WHERE state <> 'closed'")?;
        let mut stmt = tx.prepare("SELECT kind, loop_id FROM flag ORDER BY rowid")?;
        let raw = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        let flags = raw
            .into_iter()
            .map(|(kind, scope)| {
                Ok(Flag {
                    kind: parse::<FlagKind>(&kind)?,
                    scope: scope.map(LoopId),
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        let trigger = tx.query_row("SELECT count FROM tick_trigger WHERE id = 1", [], |r| {
            r.get::<_, i64>(0)
        })?;
        tx.commit()?;
        Ok(Snapshot {
            requests,
            loops,
            flags,
            trigger: uint("trigger", trigger)?,
        })
    }

    fn trigger(&self) -> Result<u64, StoreError> {
        let count =
            self.conn
                .query_row("SELECT count FROM tick_trigger WHERE id = 1", [], |r| {
                    r.get::<_, i64>(0)
                })?;
        uint("trigger", count)
    }

    fn write(
        &mut self,
        write: &Write,
        at: i64,
        payload: &serde_json::Value,
    ) -> Result<Applied, StoreError> {
        let tx = self.immediate()?;
        let applied = match write {
            Write::OpenLoop {
                repo,
                issue,
                agent,
                requests,
                situation,
                ticket,
            } => {
                let issue_i = int("issue", *issue)?;
                let open: bool = tx.query_row(
                    "SELECT EXISTS (SELECT 1 FROM loop
                     WHERE repo = ?1 AND issue = ?2 AND state <> 'closed')",
                    (repo, issue_i),
                    |r| r.get(0),
                )?;
                if open || !any_pending(&tx, requests)? {
                    return Ok(Applied::Noop);
                }
                let ordinal: i64 = tx.query_row(
                    "SELECT count(*) + 1 FROM loop WHERE repo = ?1 AND issue = ?2",
                    (repo, issue_i),
                    |r| r.get(0),
                )?;
                tx.execute(
                    "INSERT INTO loop (repo, issue, ordinal, agent, state, situation, opened_at,
                         ticket, ticket_path, base_branch, base_sha, lockfile)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    rusqlite::params![
                        repo,
                        issue_i,
                        ordinal,
                        agent.as_str(),
                        LoopState::Open.as_str(),
                        situation.as_str(),
                        at,
                        ticket.id,
                        ticket.path,
                        ticket.branch,
                        ticket.sha,
                        ticket.lockfile,
                    ],
                )?;
                let id = LoopId(tx.last_insert_rowid());
                settle(&tx, requests, RequestState::Handled, Some(id), at)?;
                Applied::Recorded(record(
                    &tx,
                    EventKind::LoopOpened,
                    at,
                    repo,
                    *issue,
                    Some(id),
                    payload,
                )?)
            }
            Write::Refuse {
                repo,
                issue,
                requests,
            } => {
                if !any_pending(&tx, requests)? {
                    return Ok(Applied::Noop);
                }
                settle(&tx, requests, RequestState::Refused, None, at)?;
                Applied::Recorded(record(
                    &tx,
                    EventKind::RequestRefused,
                    at,
                    repo,
                    *issue,
                    None,
                    payload,
                )?)
            }
            Write::Park {
                loop_id,
                reason,
                situation,
            } => {
                let run = select_loop(&tx, *loop_id)?;
                if run.state == LoopState::Parked && run.park == Some(*reason) {
                    return Ok(Applied::Noop);
                }
                tx.execute(
                    "UPDATE loop SET state = ?1, park = ?2, situation = ?3 WHERE id = ?4",
                    (
                        LoopState::Parked.as_str(),
                        reason.as_str(),
                        situation.as_str(),
                        loop_id.0,
                    ),
                )?;
                Applied::Recorded(record(
                    &tx,
                    EventKind::LoopParked,
                    at,
                    &run.repo,
                    run.issue,
                    Some(*loop_id),
                    payload,
                )?)
            }
            Write::Situation { loop_id, situation } => {
                let run = select_loop(&tx, *loop_id)?;
                if run.situation == Some(*situation) {
                    return Ok(Applied::Noop);
                }
                tx.execute(
                    "UPDATE loop SET situation = ?1 WHERE id = ?2",
                    (situation.as_str(), loop_id.0),
                )?;
                Applied::Updated
            }
            Write::Failure {
                repo,
                issue,
                loop_id,
            } => Applied::Recorded(record(
                &tx,
                EventKind::ReconcileFailed,
                at,
                repo,
                *issue,
                *loop_id,
                payload,
            )?),
        };
        tx.commit()?;
        Ok(applied)
    }

    fn request(&mut self, request: &NewRequest, at: i64) -> Result<RequestId, StoreError> {
        let tx = self.immediate()?;
        tx.execute(
            "INSERT INTO request (repo, issue, agent, state, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                &request.repo,
                int("issue", request.issue)?,
                request.agent.as_str(),
                RequestState::Pending.as_str(),
                at,
            ),
        )?;
        let id = RequestId(tx.last_insert_rowid());
        tx.execute("UPDATE tick_trigger SET count = count + 1 WHERE id = 1", [])?;
        tx.commit()?;
        Ok(id)
    }

    fn label_request(
        &mut self,
        key: &LabelKey,
        agent: Agent,
        at: i64,
    ) -> Result<Option<RequestId>, StoreError> {
        let tx = self.immediate()?;
        // Looked up first, not `INSERT OR IGNORE`: an ignored insert still
        // spends an AUTOINCREMENT id, and every tick sees the label again.
        if label_exists(&tx, key)? {
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO request (repo, issue, agent, state, created_at, label_event)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                &key.repo,
                int("issue", key.issue)?,
                agent.as_str(),
                RequestState::Pending.as_str(),
                at,
                int("label event", key.event)?,
            ),
        )?;
        let id = RequestId(tx.last_insert_rowid());
        tx.commit()?;
        Ok(Some(id))
    }

    fn label_requested(&self, key: &LabelKey) -> Result<bool, StoreError> {
        label_exists(&self.conn, key)
    }

    fn flag(&mut self, flag: Flag, at: i64) -> Result<bool, StoreError> {
        let tx = self.immediate()?;
        if let Some(id) = flag.scope {
            select_loop(&tx, id)?;
        }
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO flag (kind, loop_id, set_at) VALUES (?1, ?2, ?3)",
            (flag.kind.as_str(), flag.scope.map(|l| l.0), at),
        )? == 1;
        if inserted {
            tx.execute("UPDATE tick_trigger SET count = count + 1 WHERE id = 1", [])?;
        }
        tx.commit()?;
        Ok(inserted)
    }

    fn loops(&self) -> Result<Vec<Loop>, StoreError> {
        select_loops(&self.conn, "")
    }

    fn events(&self, since: u64, limit: usize) -> Result<Vec<Event>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, at, kind, repo, issue, loop_id, payload FROM event
             WHERE seq > ?1 ORDER BY seq LIMIT ?2",
        )?;
        let raw = stmt
            .query_map((int("since", since)?, int("limit", limit as u64)?), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        raw.into_iter()
            .map(|(seq, at, kind, repo, issue, loop_id, payload)| {
                Ok(Event {
                    seq: uint("event seq", seq)?,
                    at,
                    kind: parse(&kind)?,
                    repo,
                    issue: issue.map(|i| uint("issue", i)).transpose()?,
                    loop_id: loop_id.map(LoopId),
                    payload: serde_json::from_str(&payload)?,
                })
            })
            .collect()
    }
}

fn label_exists(conn: &Connection, key: &LabelKey) -> Result<bool, StoreError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM request
         WHERE repo = ?1 AND issue = ?2 AND label_event = ?3)",
        (
            &key.repo,
            int("issue", key.issue)?,
            int("label event", key.event)?,
        ),
        |r| r.get(0),
    )?)
}

fn select_loop(conn: &Connection, id: LoopId) -> Result<Loop, StoreError> {
    let row = conn
        .query_row(
            &format!("SELECT {LOOP_COLUMNS} FROM loop WHERE id = ?1"),
            [id.0],
            loop_row,
        )
        .optional()?
        .ok_or(ReconcileError::NoLoop(id))?;
    to_loop(row)
}
