use std::path::PathBuf;

use rsb_gauge_reconcile::Situation;
use rsb_gauge_schema::RegistryError;
use rsb_gauge_store::StoreError;

/// Every failure the program reports. None carries a token or a key.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HOME is not set, so the config directory is unknown")]
    NoHome,
    #[error("cannot read {path}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot write {path}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is readable by others (mode {mode:o}); run chmod 600")]
    KeyMode { path: PathBuf, mode: u32 },
    #[error("config {path}")]
    Config {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("repo {0:?} is not a valid owner/name")]
    RepoName(String),
    #[error("repo {0} is not listed in config")]
    RepoNotConfigured(String),
    #[error("request id {0:?} is not 32 lowercase hex digits")]
    RequestId(String),
    /// Our fault: what would have been a panic.
    #[error("internal error: {0}")]
    Failed(String),
    #[error("the clock is before 1970")]
    Clock,
    #[error("signing the App JWT")]
    Sign(#[from] jsonwebtoken::errors::Error),
    #[error("the App is not installed on {0}")]
    NotInstalled(String),
    #[error("GitHub {what}")]
    Http {
        what: &'static str,
        source: ureq::Error,
    },
    #[error("GitHub returned status {status} for {what}")]
    Status { what: &'static str, status: u16 },
    #[error("store")]
    Store(#[from] StoreError),
    /// The tick's first row: the store cannot be opened, so nothing is done.
    #[error("{situation}: the tick took no action")]
    Unreachable {
        situation: Situation,
        source: StoreError,
    },
    #[error("registry")]
    Registry(#[from] RegistryError),
    #[error("the driver's config is unavailable: {0}")]
    NoConfig(String),
    #[error("{repo}#{issue} carries the run label but no event applied it")]
    NoLabelEvent { repo: String, issue: u64 },
    #[error("{0:?} is not a full commit id")]
    Sha(String),
    #[error("{0} exists and is not empty")]
    CloneTarget(PathBuf),
    #[error("cannot start git")]
    GitStart(#[source] std::io::Error),
    /// git's own message, with the credential cut out.
    #[error("git {what} exited {}: {stderr}", exit(*.status))]
    Git {
        what: &'static str,
        status: Option<i32>,
        stderr: String,
    },
    #[error("cannot start {0}")]
    Start(&'static str, #[source] std::io::Error),
    /// A command the attempts port runs, with what it printed on stderr.
    #[error("{what} exited {}: {stderr}", exit(*.status))]
    Command {
        what: String,
        status: Option<i32>,
        stderr: String,
    },
    #[error("systemd shows unit {unit} in a state the driver does not know: {state}")]
    UnitState { unit: String, state: String },
    #[error("set agent_name and agent_email in the driver's [attempt] config to start an attempt")]
    NoAgentIdentity,
}

fn exit(status: Option<i32>) -> String {
    status.map_or_else(|| "on a signal".to_owned(), |code| code.to_string())
}

/// Renders an error for a person: each error in the `source()` chain, joined
/// by a colon (engineering/errors.md).
pub fn report(error: &dyn std::error::Error) -> String {
    let mut out = error.to_string();
    let mut source = error.source();
    while let Some(e) = source {
        out.push_str(&format!(": {e}"));
        source = e.source();
    }
    out
}
