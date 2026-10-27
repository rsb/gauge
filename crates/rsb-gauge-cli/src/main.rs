//! The headless binary, `gauge`. Every capability Gauge has is a command here
//! before it has a window; the desktop app calls the same library and adds
//! nothing.

use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use rsb_gauge_schema::{Registry, RegistryError};

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
    }
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
