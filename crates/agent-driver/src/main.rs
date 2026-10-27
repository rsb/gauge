//! The one binary that holds the GitHub App key. No other crate in the
//! workspace may depend on this one or on a GitHub or HTTP client; see the
//! custody rule in engineering/layers.md.

// Kept from the old `agent-driver` for the tick: the token holder, the issue
// write operations and the audit log. `serve`, their only caller, was not
// moved.
#[expect(dead_code, reason = "the audit log is the seed of the tick's events")]
mod audit;
mod config;
mod error;
#[expect(dead_code, reason = "the issue list and writes wait for the tick")]
mod github;
mod jwt;
mod repo;
#[expect(dead_code, reason = "the token holder waits for the tick")]
mod tokens;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::config::Config;
use crate::error::Error;
use crate::github::{Access, Client, Permissions};
use crate::repo::RepoName;

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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("agent-driver: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<(), Error> {
    match command {
        Command::Check { repo, issue } => check(&repo, issue),
    }
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
