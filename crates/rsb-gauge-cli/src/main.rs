//! The headless binary, `gauge`. Every capability Gauge has is a command here
//! before it has a window; the desktop app calls the same library and adds
//! nothing. The owner's loop commands write request and flag rows only; the
//! tick in `agent-driver` acts on them.

use std::any::Any;
use std::io::Write as _;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use rsb_gauge_reconcile::store::{Agent, Event, Flag, FlagKind, LoopId, NewRequest, Store};
use rsb_gauge_schema::{Registry, RegistryError, Repos};
use rsb_gauge_store::{SqliteStore, StoreError};

/// Control center for agent-driven development.
#[derive(Parser)]
#[command(name = "gauge", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// The rule registry: load, lint and describe it.
    Registry {
        #[command(subcommand)]
        command: RegistryCommand,
    },
    /// The store file named by GAUGE_STORE.
    Store {
        #[command(subcommand)]
        command: StoreCommand,
    },
    /// Ask for a ticket issue to run. Writes a request; the next tick acts.
    Run {
        /// The repo as named in repos.toml, then `#` and the issue number.
        target: Target,
        /// The agent the loop's attempts run.
        #[arg(long, default_value = "claude")]
        agent: Agent,
    },
    /// Act on nothing, for one loop or, with no loop, all of them.
    Pause {
        #[arg(value_name = "LOOP")]
        loop_id: Option<i64>,
    },
    /// Start nothing new, for one loop or, with no loop, all of them.
    Drain {
        #[arg(value_name = "LOOP")]
        loop_id: Option<i64>,
    },
    /// List every loop and the situation its last tick derived.
    Runs,
    /// Print the events after a sequence number, in order. Each line starts
    /// with its sequence: pass the last one printed as --since to resume.
    Events {
        #[arg(long, default_value_t = 0)]
        since: u64,
        /// Keep printing new events as they are recorded.
        #[arg(long)]
        follow: bool,
    },
}

#[derive(Subcommand)]
enum StoreCommand {
    /// Create the store file and tie it to the repos.toml whose repos its
    /// ticks may open loops on. Refuses a file that exists.
    Init {
        /// The live store uses agent-registry's repos.toml; a dev store, a
        /// file listing only a scratch repo.
        #[arg(long)]
        repos: PathBuf,
    },
}

/// `<repo>#<issue>`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    repo: String,
    issue: u64,
}

impl FromStr for Target {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let bad = || Error::Target(s.to_owned());
        let (repo, issue) = s.split_once('#').ok_or_else(bad)?;
        let named = !repo.is_empty()
            && repo
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        let issue: u64 = issue.parse().map_err(|_| bad())?;
        if !named || issue == 0 {
            return Err(bad());
        }
        Ok(Self {
            repo: repo.to_owned(),
            issue,
        })
    }
}

#[derive(Subcommand)]
enum RegistryCommand {
    /// Load every rule under a directory and report what is wrong with it.
    Lint {
        /// The registry root, such as `../agent-registry/rules`.
        root: PathBuf,
    },
    /// Print the JSON Schema for one rule file.
    Schema,
}

/// Why a command failed. The command name is not part of the error; the
/// boundary renders it first, through [`report`].
#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("registry")]
    Registry(#[from] RegistryError),
    #[error("schema")]
    Schema(#[from] serde_json::Error),
    #[error("store")]
    Store(#[from] StoreError),
    #[error("{0:?} is not <repo>#<issue>")]
    Target(String),
    #[error("cannot write to stdout")]
    Stdout(#[source] std::io::Error),
    /// Our fault: what would have been a panic. No caller branches on it.
    #[error("internal error: {0}")]
    Internal(String),
}

/// What a command produced: the lines to print, and whether the run passed.
struct Outcome {
    lines: Vec<String>,
    passed: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let name = match &cli.command {
        Command::Registry { command } => match command {
            RegistryCommand::Lint { .. } => "registry lint",
            RegistryCommand::Schema => "registry schema",
        },
        Command::Store { command } => match command {
            StoreCommand::Init { .. } => "store init",
        },
        Command::Run { .. } => "run",
        Command::Pause { .. } => "pause",
        Command::Drain { .. } => "drain",
        Command::Runs => "runs",
        Command::Events { .. } => "events",
    };
    match dispatch(name, cli.command) {
        Ok(outcome) => {
            for line in &outcome.lines {
                println!("{line}");
            }
            if outcome.passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("{}", report(name, &e));
            ExitCode::FAILURE
        }
    }
}

/// Runs one command. The one catch in the codebase: a panic in a command is
/// our bug, and the caller gets it as an error rather than a crash.
fn dispatch(name: &str, command: Command) -> Result<Outcome, Error> {
    panic::catch_unwind(AssertUnwindSafe(|| run(command))).unwrap_or_else(|payload| {
        Err(Error::Internal(format!(
            "{name} panicked: {}",
            panic_message(&*payload)
        )))
    })
}

fn run(command: Command) -> Result<Outcome, Error> {
    match command {
        Command::Registry { command } => match command {
            RegistryCommand::Lint { root } => lint(&root),
            RegistryCommand::Schema => schema(),
        },
        Command::Store { command } => match command {
            StoreCommand::Init { repos } => init(&rsb_gauge_store::path_from_env()?, &repos),
        },
        Command::Run { target, agent } => request(&mut open()?, &target, agent, now()),
        Command::Pause { loop_id } => flag(&mut open()?, FlagKind::Pause, loop_id, now()),
        Command::Drain { loop_id } => flag(&mut open()?, FlagKind::Drain, loop_id, now()),
        Command::Runs => runs(&open()?),
        Command::Events { since, follow } => {
            let store = open()?;
            if follow {
                self::follow(&store, since)
            } else {
                Ok(Outcome {
                    lines: events(&store, since)?.0,
                    passed: true,
                })
            }
        }
    }
}

/// The store named by GAUGE_STORE. It must exist: only `store init` creates
/// one.
fn open() -> Result<SqliteStore, Error> {
    Ok(SqliteStore::open(&rsb_gauge_store::path_from_env()?)?)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn init(path: &Path, repos: &Path) -> Result<Outcome, Error> {
    let repos = std::fs::canonicalize(repos).map_err(|source| RegistryError::Read {
        path: repos.to_path_buf(),
        source,
    })?;
    let listed = Repos::load(&repos)?;
    SqliteStore::create(path, &repos)?;
    Ok(Outcome {
        lines: vec![format!(
            "created {} for {} ({} repos)",
            path.display(),
            repos.display(),
            listed.repos.len()
        )],
        passed: true,
    })
}

fn request(
    store: &mut impl Store<Error = StoreError>,
    target: &Target,
    agent: Agent,
    at: i64,
) -> Result<Outcome, Error> {
    let id = store.request(
        &NewRequest {
            repo: target.repo.clone(),
            issue: target.issue,
            agent,
        },
        at,
    )?;
    Ok(Outcome {
        lines: vec![format!(
            "request {} for {}#{} ({agent}); the next tick acts on it",
            id.0, target.repo, target.issue
        )],
        passed: true,
    })
}

fn flag(
    store: &mut impl Store<Error = StoreError>,
    kind: FlagKind,
    loop_id: Option<i64>,
    at: i64,
) -> Result<Outcome, Error> {
    let scope = loop_id.map(LoopId);
    let set = store.flag(Flag { kind, scope }, at)?;
    let on = match scope {
        Some(id) => format!("loop {id}"),
        None => "every loop".to_owned(),
    };
    let what = if set { "set" } else { "was already set" };
    Ok(Outcome {
        lines: vec![format!("{kind} {what} on {on}")],
        passed: true,
    })
}

fn runs(store: &impl Store<Error = StoreError>) -> Result<Outcome, Error> {
    let loops = store.loops()?;
    let mut lines: Vec<String> = loops
        .iter()
        .map(|l| {
            let state = match l.park {
                Some(reason) => format!("{} {reason}", l.state),
                None => l.state.to_string(),
            };
            let situation = l.situation.map_or("-", |s| s.as_str());
            let ticket = l
                .ticket
                .as_ref()
                .map_or_else(String::new, |t| format!(", ticket {}", t.id));
            format!(
                "loop {} {}#{} ({}) {state}, last {situation}, {}{ticket}, opened {}",
                l.id,
                l.repo,
                l.issue,
                l.ordinal,
                l.agent,
                rfc3339(l.opened_at)
            )
        })
        .collect();
    lines.push(format!("{} loops", loops.len()));
    Ok(Outcome {
        lines,
        passed: true,
    })
}

/// How many events one read returns.
const PAGE: usize = 500;

/// Every event after `since`, one line each, and the last sequence read.
fn events(store: &impl Store<Error = StoreError>, since: u64) -> Result<(Vec<String>, u64), Error> {
    let mut lines = Vec::new();
    let mut cursor = since;
    loop {
        let page = store.events(cursor, PAGE)?;
        let Some(last) = page.last() else {
            return Ok((lines, cursor));
        };
        cursor = last.seq;
        lines.extend(page.iter().map(event_line));
    }
}

/// Prints events as they are recorded, from `since`, until stopped.
fn follow(store: &impl Store<Error = StoreError>, since: u64) -> Result<Outcome, Error> {
    let mut cursor = since;
    let mut out = std::io::stdout();
    loop {
        let (lines, next) = events(store, cursor)?;
        for line in &lines {
            writeln!(out, "{line}").map_err(Error::Stdout)?;
        }
        out.flush().map_err(Error::Stdout)?;
        if next == cursor {
            std::thread::sleep(Duration::from_secs(1));
        }
        cursor = next;
    }
}

fn event_line(event: &Event) -> String {
    let mut line = format!("{} {} {}", event.seq, rfc3339(event.at), event.kind);
    if let (Some(repo), Some(issue)) = (&event.repo, event.issue) {
        line.push_str(&format!(" {repo}#{issue}"));
    }
    if let Some(id) = event.loop_id {
        line.push_str(&format!(" loop {id}"));
    }
    line.push_str(&format!(" {}", event.payload));
    line
}

/// Unix seconds as RFC 3339 UTC. The civil date is Howard Hinnant's
/// `civil_from_days`, as in agent-driver's audit log; a time before 1970
/// reads as the epoch.
fn rfc3339(at: i64) -> String {
    let secs = u64::try_from(at).unwrap_or(0);
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

fn lint(root: &std::path::Path) -> Result<Outcome, Error> {
    let registry = Registry::load(root)?;
    let findings = rsb_gauge_schema::lint(&registry);
    let mut lines: Vec<String> = findings.iter().map(ToString::to_string).collect();
    lines.push(format!(
        "{} rules, {} findings",
        registry.entries.len(),
        findings.len()
    ));
    Ok(Outcome {
        lines,
        passed: findings.is_empty(),
    })
}

fn schema() -> Result<Outcome, Error> {
    let text = serde_json::to_string_pretty(&rsb_gauge_schema::schema())?;
    Ok(Outcome {
        lines: vec![text],
        passed: true,
    })
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("no message")
}

/// Renders a command's error for a person: the command name, then each error
/// in the `source()` chain, joined by a colon.
fn report(command: &str, error: &dyn std::error::Error) -> String {
    let mut out = format!("{command}: {error}");
    let mut source = error.source();
    while let Some(e) = source {
        out.push_str(&format!(": {e}"));
        source = e.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_walks_the_source_chain_to_the_leaf() {
        let io = std::io::Error::from(std::io::ErrorKind::NotFound);
        let err = Error::Registry(RegistryError::Read {
            path: "rules/errors/ERR-1.toml".into(),
            source: io,
        });
        assert_eq!(
            report("registry lint", &err),
            "registry lint: registry: cannot read rules/errors/ERR-1.toml: entity not found"
        );
    }

    fn store(dir: &Path) -> SqliteStore {
        let repos = dir.join("repos.toml");
        std::fs::write(&repos, "schema = 1\n").unwrap();
        init(&dir.join("dev.sqlite"), &repos).unwrap();
        SqliteStore::open(&dir.join("dev.sqlite")).unwrap()
    }

    /// Records one event per call, as a tick would.
    fn record(store: &mut SqliteStore, issue: u64) {
        use rsb_gauge_reconcile::store::Write;
        let write = Write::Failure {
            repo: "lumen".into(),
            issue,
            loop_id: None,
        };
        store
            .write(&write, 1_793_000_000, &serde_json::Value::Null)
            .unwrap();
    }

    fn seqs(lines: &[String]) -> Vec<u64> {
        lines
            .iter()
            .map(|l| l.split(' ').next().unwrap().parse().unwrap())
            .collect()
    }

    #[test]
    fn a_target_is_repo_hash_issue() {
        assert_eq!(
            "ref-rsb-serverless-stack#12".parse::<Target>().unwrap(),
            Target {
                repo: "ref-rsb-serverless-stack".into(),
                issue: 12
            }
        );
        for bad in [
            "lumen",
            "#3",
            "lumen#",
            "lumen#0",
            "lumen#-1",
            "rsb/lumen#3",
            "lu men#3",
        ] {
            assert!(bad.parse::<Target>().is_err(), "{bad:?} parsed");
        }
    }

    #[test]
    fn run_and_flags_write_rows_and_act_on_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let target = "lumen#3".parse().unwrap();
        request(&mut store, &target, Agent::Codex, 10).unwrap();
        flag(&mut store, FlagKind::Pause, None, 11).unwrap();
        let again = flag(&mut store, FlagKind::Pause, None, 12).unwrap();
        assert_eq!(again.lines, ["pause was already set on every loop"]);
        assert!(flag(&mut store, FlagKind::Drain, Some(9), 13).is_err());
        let snapshot = store.snapshot().unwrap();
        assert_eq!(snapshot.requests.len(), 1);
        assert_eq!(snapshot.requests[0].agent, Agent::Codex);
        assert_eq!(snapshot.trigger, 2);
        assert!(store.loops().unwrap().is_empty());
        assert!(store.events(0, 10).unwrap().is_empty());
    }

    #[test]
    fn events_since_n_returns_every_later_event_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        for issue in 1..=(PAGE as u64 + 7) {
            record(&mut store, issue);
        }
        let (lines, last) = events(&store, 0).unwrap();
        assert_eq!(seqs(&lines), (1..=PAGE as u64 + 7).collect::<Vec<_>>());
        assert_eq!(last, PAGE as u64 + 7);
        let (lines, _) = events(&store, 5).unwrap();
        assert_eq!(seqs(&lines).first(), Some(&6));
        assert_eq!(lines.len(), PAGE + 2);
        let (lines, cursor) = events(&store, last).unwrap();
        assert!(lines.is_empty());
        assert_eq!(cursor, last);
    }

    #[test]
    fn a_follower_resumes_from_the_last_sequence_it_printed() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        record(&mut store, 1);
        record(&mut store, 2);
        let (printed, _) = events(&store, 0).unwrap();
        let last_printed = *seqs(&printed).last().unwrap();
        // The follower stops; events arrive; it restarts with --since.
        drop(store);
        let mut store = SqliteStore::open(&dir.path().join("dev.sqlite")).unwrap();
        record(&mut store, 3);
        record(&mut store, 4);
        let (resumed, _) = events(&store, last_printed).unwrap();
        assert_eq!(seqs(&resumed), [3, 4]);
        let mut all = seqs(&printed);
        all.extend(seqs(&resumed));
        assert_eq!(all, [1, 2, 3, 4]);
    }

    #[test]
    fn init_refuses_an_unreadable_repos_file_and_an_existing_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dev.sqlite");
        assert!(init(&path, &dir.path().join("absent.toml")).is_err());
        let bad = dir.path().join("bad.toml");
        std::fs::write(&bad, "schema = 2\n").unwrap();
        assert!(init(&path, &bad).is_err());
        assert!(!path.exists());
        drop(store(dir.path()));
        let good = dir.path().join("repos.toml");
        assert!(matches!(
            init(&path, &good),
            Err(Error::Store(StoreError::Exists(_)))
        ));
    }

    #[test]
    fn times_are_rfc3339_utc() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_793_000_000), "2026-10-26T07:33:20Z");
        assert_eq!(rfc3339(-5), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn a_panic_in_a_command_becomes_an_internal_error() {
        let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<Outcome, Error> {
            panic!("the registry walked off the end")
        }))
        .unwrap_or_else(|payload| {
            Err(Error::Internal(format!(
                "registry lint panicked: {}",
                panic_message(&*payload)
            )))
        });
        let err = result.err().expect("the panic is an error");
        assert_eq!(
            report("registry lint", &err),
            "registry lint: internal error: registry lint panicked: the registry walked off the end"
        );
    }
}
