//! The one binary that holds the GitHub App key. No other crate in the
//! workspace may depend on this one or on a GitHub or HTTP client; see the
//! custody rule in engineering/layers.md. It also runs the tick, the one
//! process that acts on the store's requests.

// Kept from the old `agent-driver` for the tick: the token holder, the issue
// write operations and the audit log. `serve`, their only caller, was not
// moved.
mod attempt;
#[expect(dead_code, reason = "the audit log is the seed of the tick's events")]
mod audit;
mod config;
mod error;
mod git;
#[expect(dead_code, reason = "the issue writes wait for ticket 07")]
mod github;
mod jwt;
mod live;
mod registry;
mod repo;
mod tick;
mod tokens;
mod world;

#[cfg(test)]
mod contract;
#[cfg(test)]
mod fake;
#[cfg(test)]
mod harness;

use std::path::Path;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use rsb_gauge_reconcile::{Observation, decide};
use rsb_gauge_store::SqliteStore;

use crate::attempt::{LiveAttempts, Settings};
use crate::config::Config;
use crate::error::{Error, report};
use crate::github::{Access, Client, Permissions};
use crate::live::Live;
use crate::registry::Registry;
use crate::repo::RepoName;
use crate::tick::{Mode, Outcome, Polled, Report, SystemClock};

/// Holds the GitHub App key and talks to GitHub as the App.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Mint a read-only token for one repo and print an issue's title.
    Check {
        /// Repository as owner/name. Must be listed in config.
        #[arg(long)]
        repo: RepoName,
        /// Issue number to read.
        #[arg(long)]
        issue: u64,
    },
    /// One pass over every pending request and open loop in the store named
    /// by GAUGE_STORE: observe, decide, act, record. Exits non-zero when the
    /// store is unreachable or any issue failed.
    Tick {
        /// Observe and decide only: print what a tick would do and write
        /// nothing.
        #[arg(long)]
        observe: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let name = match &cli.command {
        Command::Check { .. } => "check",
        Command::Tick { .. } => "tick",
    };
    match run(cli.command) {
        Ok(passed) => {
            if passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("agent-driver {name}: {}", report(&error));
            ExitCode::FAILURE
        }
    }
}

/// Runs one command. `Ok(false)` is a run that finished and failed.
fn run(command: Command) -> Result<bool, Error> {
    match command {
        Command::Check { repo, issue } => check(&repo, issue).map(|()| true),
        Command::Tick { observe } => {
            let mode = if observe { Mode::Observe } else { Mode::Act };
            let path = rsb_gauge_store::path_from_env()?;
            let report = tick_store(&path, mode)?;
            for line in lines(&report, mode) {
                println!("{line}");
            }
            Ok(report.failed == 0)
        }
    }
}

/// Opens the store at `path` and runs one tick on it.
fn tick_store(path: &Path, mode: Mode) -> Result<Report, Error> {
    let mut store = match SqliteStore::open(path) {
        Ok(store) => store,
        Err(source) if source.is_unreachable() => {
            let decision = decide(&Observation::StoreUnreachable);
            return Err(Error::Unreachable {
                situation: decision.situation,
                source,
            });
        }
        Err(e) => return Err(e.into()),
    };
    let registry = store
        .repos()
        .map_err(Error::from)
        .and_then(|repos| Registry::load(&repos))
        .map_err(|e| report(&e));
    // Without its config the driver reaches no repo: the tick polls nothing
    // and fails only the requests that need GitHub.
    let config = Config::load().map_err(|e| report(&e));
    let mut github = Live::new(config.as_ref().map_err(Clone::clone));
    // Without its config no attempt starts; the rest of the tick runs.
    let settings = config.as_ref().map_err(Clone::clone).map(|c| Settings {
        config: c.attempt.clone(),
        path: std::env::var("PATH").unwrap_or_default(),
        home: std::env::var("HOME").unwrap_or_default(),
    });
    let mut attempts = LiveAttempts::new(attempt::runs_root(
        std::env::var_os("XDG_DATA_HOME"),
        std::env::var_os("HOME"),
    )?);
    Ok(tick::run(
        &mut store,
        &mut github,
        &mut attempts,
        &registry,
        &settings,
        &SystemClock,
        mode,
    )?)
}

/// The report: one line per repo polled, one per issue, then a summary.
fn lines(report: &Report, mode: Mode) -> Vec<String> {
    let new = match mode {
        Mode::Act => "new",
        Mode::Observe => "new, not recorded",
    };
    let mut out: Vec<String> = report
        .polls
        .iter()
        .map(|poll| {
            let repo = poll.repo.as_deref().unwrap_or("every repo");
            match &poll.polled {
                Polled::Seen { labelled, new: n } => {
                    format!("poll {repo}: {labelled} labelled, {n} {new}")
                }
                Polled::Unreached => {
                    format!("poll {repo}: not polled, the driver's config does not list it")
                }
                Polled::Failed(message) => format!("poll {repo}: failed: {message}"),
            }
        })
        .collect();
    out.extend(report.lines.iter().map(|line| {
        let subject = match line.loop_id {
            Some(id) => format!("{}#{} loop {id}", line.repo, line.issue),
            None => format!("{}#{}", line.repo, line.issue),
        };
        let decided = match &line.decision {
            Some(d) => format!("{}: {}", d.situation, d.action),
            None => "undecided".to_owned(),
        };
        let outcome = match &line.outcome {
            Outcome::Observed => "not acted on".to_owned(),
            Outcome::Waited => "nothing written".to_owned(),
            Outcome::Applied(rsb_gauge_reconcile::store::Applied::Recorded(event)) => {
                format!("event {} {}", event.seq, event.kind)
            }
            Outcome::Applied(rsb_gauge_reconcile::store::Applied::Updated) => {
                "situation recorded".to_owned()
            }
            Outcome::Applied(rsb_gauge_reconcile::store::Applied::Noop) => {
                "already done".to_owned()
            }
            Outcome::Failed(message) => format!("failed: {message}"),
        };
        format!("{subject}: {decided} ({outcome})")
    }));
    let verb = match mode {
        Mode::Act => "tick",
        Mode::Observe => "observe",
    };
    out.push(format!(
        "{verb}: {} issues, {} failed",
        report.lines.len(),
        report.failed
    ));
    if report.again && mode == Mode::Act {
        out.push("a trigger arrived during the tick: one more tick follows".to_owned());
    }
    out
}

fn check(repo: &RepoName, number: u64) -> Result<(), Error> {
    let config = Config::load()?;
    let client = Client::new();
    let minted = tokens::mint(
        &config,
        &client,
        repo,
        Permissions {
            issues: Some(Access::Read),
            ..Permissions::default()
        },
    )?;
    let issue = client.issue(&minted.token, repo, number)?;
    println!("#{} ({}): {}", issue.number, issue.state, issue.title);
    Ok(())
}
