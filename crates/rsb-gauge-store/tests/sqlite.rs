//! The SQLite adapter against temporary files, never the live store.

#![cfg(test)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use rsb_gauge_reconcile::store::{
    Agent, Applied, EventKind, Flag, FlagKind, LoopId, LoopState, NewRequest, Store, Write,
};
use rsb_gauge_reconcile::{MemoryStore, ParkReason, Situation};
use rsb_gauge_store::{SqliteStore, StoreError};
use serde_json::json;

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn create(dir: &Path) -> (PathBuf, SqliteStore) {
    let path = dir.join("dev.sqlite");
    let store = SqliteStore::create(&path, Path::new("/registry/repos.toml")).unwrap();
    (path, store)
}

fn request(repo: &str, issue: u64) -> NewRequest {
    NewRequest {
        repo: repo.into(),
        issue,
        agent: Agent::Claude,
    }
}

fn open_loop(repo: &str, issue: u64, ids: Vec<rsb_gauge_reconcile::store::RequestId>) -> Write {
    Write::OpenLoop {
        repo: repo.into(),
        issue,
        agent: Agent::Claude,
        requests: ids,
        situation: Situation::RunRequested,
    }
}

#[test]
fn create_applies_v1_and_ties_the_store_to_its_repos() {
    let dir = dir();
    let (path, store) = create(dir.path());
    assert_eq!(store.version().unwrap(), 1);
    assert_eq!(
        store.repos().unwrap(),
        PathBuf::from("/registry/repos.toml")
    );
    drop(store);
    let again = SqliteStore::open(&path).unwrap();
    assert_eq!(again.version().unwrap(), 1);
    assert!(matches!(
        SqliteStore::create(&path, Path::new("x")),
        Err(StoreError::Exists(_))
    ));
}

#[test]
fn a_missing_file_is_unreachable_and_is_not_created() {
    let dir = dir();
    let path = dir.path().join("absent.sqlite");
    let err = SqliteStore::open(&path).err().unwrap();
    assert!(matches!(err, StoreError::Missing(_)));
    assert!(err.is_unreachable());
    assert!(!path.exists());
}

#[test]
fn a_file_that_is_not_a_store_is_unreachable() {
    let dir = dir();
    let path = dir.path().join("junk.sqlite");
    std::fs::write(&path, vec![0x5a; 8192]).unwrap();
    let err = SqliteStore::open(&path).err().unwrap();
    assert!(err.is_unreachable(), "{err:?}");
}

#[test]
fn a_lock_held_past_the_timeout_is_unreachable() {
    let dir = dir();
    let (path, _store) = create(dir.path());
    let holder = rusqlite::Connection::open(&path).unwrap();
    holder.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let err = SqliteStore::open_with(&path, Duration::from_millis(50))
        .err()
        .unwrap();
    assert!(err.is_unreachable(), "{err:?}");
    holder.execute_batch("ROLLBACK").unwrap();
    assert!(SqliteStore::open(&path).is_ok());
}

#[test]
fn a_newer_schema_is_refused() {
    let dir = dir();
    let (path, store) = create(dir.path());
    drop(store);
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute(
        "INSERT INTO migration (version, applied_at) VALUES (99, 0)",
        [],
    )
    .unwrap();
    drop(raw);
    assert!(matches!(
        SqliteStore::open(&path),
        Err(StoreError::Newer {
            found: 99,
            known: 1
        })
    ));
}

#[test]
fn the_store_runs_in_wal_mode() {
    let dir = dir();
    let (path, store) = create(dir.path());
    drop(store);
    let raw = rusqlite::Connection::open(&path).unwrap();
    let mode: String = raw
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
}

#[test]
fn an_unknown_stored_value_is_an_error_not_a_default() {
    let dir = dir();
    let (path, mut store) = create(dir.path());
    let id = store.request(&request("lumen", 3), 10).unwrap();
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute("UPDATE request SET agent = 'gemini' WHERE id = ?1", [id.0])
        .unwrap();
    assert!(matches!(store.snapshot(), Err(StoreError::Value(_))));
}

#[test]
fn a_flag_on_a_loop_that_does_not_exist_is_refused() {
    let dir = dir();
    let (_path, mut store) = create(dir.path());
    let flag = Flag {
        kind: FlagKind::Pause,
        scope: Some(LoopId(42)),
    };
    assert!(store.flag(flag, 1).is_err());
}

/// The same calls against SQLite and the in-memory fake give the same
/// results: the fake is a faithful stand-in for the tick tests.
#[test]
fn sqlite_and_the_fake_agree() {
    let dir = dir();
    let (_path, mut sqlite) = create(dir.path());
    let mut memory = MemoryStore::new();
    let payload = json!({ "note": "same" });

    fn script<S: Store>(store: &mut S, payload: &serde_json::Value) -> Vec<String> {
        let mut out = Vec::new();
        let mut log = |label: &str, value: String| out.push(format!("{label}: {value}"));
        let a = store.request(&request("ref", 1), 10).unwrap();
        let b = store.request(&request("ref", 1), 11).unwrap();
        let c = store.request(&request("gauge", 2), 12).unwrap();
        log("trigger", store.trigger().unwrap().to_string());
        let snap = store.snapshot().unwrap();
        log("pending", snap.requests.len().to_string());
        let applied = |a: Applied| match a {
            Applied::Recorded(e) => format!("event {} {} {:?}", e.seq, e.kind, e.loop_id),
            other => format!("{other:?}"),
        };
        let open = open_loop("ref", 1, vec![a, b]);
        log("open", applied(store.write(&open, 20, payload).unwrap()));
        log(
            "open again",
            applied(store.write(&open, 21, payload).unwrap()),
        );
        let refuse = Write::Refuse {
            repo: "gauge".into(),
            issue: 2,
            requests: vec![c],
        };
        log(
            "refuse",
            applied(store.write(&refuse, 22, payload).unwrap()),
        );
        log(
            "refuse again",
            applied(store.write(&refuse, 23, payload).unwrap()),
        );
        let situation = Write::Situation {
            loop_id: LoopId(1),
            situation: Situation::Paused,
        };
        log(
            "note",
            applied(store.write(&situation, 24, payload).unwrap()),
        );
        log(
            "note again",
            applied(store.write(&situation, 25, payload).unwrap()),
        );
        let park = Write::Park {
            loop_id: LoopId(1),
            reason: ParkReason::UnrecognizedState,
            situation: Situation::Unrecognized,
        };
        log("park", applied(store.write(&park, 26, payload).unwrap()));
        log(
            "park again",
            applied(store.write(&park, 27, payload).unwrap()),
        );
        let d = store.request(&request("ref", 1), 28).unwrap();
        log(
            "second loop while one is parked",
            applied(
                store
                    .write(&open_loop("ref", 1, vec![d]), 29, payload)
                    .unwrap(),
            ),
        );
        let pause = Flag {
            kind: FlagKind::Pause,
            scope: None,
        };
        log("pause", store.flag(pause, 30).unwrap().to_string());
        log("pause again", store.flag(pause, 31).unwrap().to_string());
        let drain = Flag {
            kind: FlagKind::Drain,
            scope: Some(LoopId(1)),
        };
        log("drain loop", store.flag(drain, 32).unwrap().to_string());
        log(
            "drain loop again",
            store.flag(drain, 33).unwrap().to_string(),
        );
        log("trigger", store.trigger().unwrap().to_string());
        let snap = store.snapshot().unwrap();
        log(
            "snapshot",
            format!("{:?}", (snap.requests, snap.loops, snap.flags)),
        );
        let events = store.events(0, 100).unwrap();
        log("events", format!("{events:?}"));
        log(
            "events since 1",
            format!("{:?}", store.events(1, 100).unwrap()),
        );
        log(
            "events limited",
            format!("{:?}", store.events(0, 1).unwrap()),
        );
        log("loops", format!("{:?}", store.loops().unwrap()));
        out
    }

    let from_sqlite = script(&mut sqlite, &payload);
    let from_memory = script(&mut memory, &payload);
    assert_eq!(from_sqlite, from_memory);
    let loops = sqlite.loops().unwrap();
    assert_eq!(loops.len(), 1);
    assert_eq!(loops[0].state, LoopState::Parked);
    let kinds: Vec<EventKind> = sqlite
        .events(0, 100)
        .unwrap()
        .iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            EventKind::LoopOpened,
            EventKind::RequestRefused,
            EventKind::LoopParked
        ]
    );
}

#[test]
fn events_come_back_in_sequence_from_any_cursor() {
    let dir = dir();
    let (_path, mut store) = create(dir.path());
    for issue in 1..=5 {
        let write = Write::Failure {
            repo: "ref".into(),
            issue,
            loop_id: None,
        };
        store
            .write(&write, issue as i64, &json!({ "n": issue }))
            .unwrap();
    }
    let all: Vec<u64> = store
        .events(0, 100)
        .unwrap()
        .iter()
        .map(|e| e.seq)
        .collect();
    assert_eq!(all, [1, 2, 3, 4, 5]);
    let after: Vec<u64> = store
        .events(3, 100)
        .unwrap()
        .iter()
        .map(|e| e.seq)
        .collect();
    assert_eq!(after, [4, 5]);
    assert!(store.events(5, 100).unwrap().is_empty());
}
