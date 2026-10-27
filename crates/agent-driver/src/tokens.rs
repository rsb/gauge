//! Minting, and the tokens a long-running driver holds: one per repo and
//! set of permissions, in memory only, reused well inside the hour GitHub
//! honours them for.

use std::time::{Duration, Instant};

use crate::config::Config;
use crate::error::Error;
use crate::github::{Client, Minted, Permissions, Token, TokenRequest};
use crate::jwt::{Claims, Jwt};
use crate::repo::RepoName;

/// How long a held token is reused. GitHub honours one for an hour; half
/// of that leaves room for a slow request and a skewed clock.
pub const REUSE: Duration = Duration::from_secs(30 * 60);

/// Checks `repo` is listed, signs the App JWT, finds the installation and
/// mints a token narrowed to `repo` and `permissions`.
pub fn mint(
    config: &Config,
    client: &Client,
    repo: &RepoName,
    permissions: Permissions,
) -> Result<Minted, Error> {
    let _policy = config.repo(repo)?;
    let jwt = Jwt::sign(&config.read_key()?, &Claims::now(config.app_id)?)?;
    let installation = client.installation_id(&jwt, repo)?;
    client.mint(
        &jwt,
        installation,
        &TokenRequest::narrowed(repo, permissions),
    )
}

struct Held {
    repo: RepoName,
    permissions: Permissions,
    minted_at: Instant,
    token: Token,
}

/// The tokens the driver holds, and the client that uses them.
pub struct Tokens<'a> {
    config: &'a Config,
    client: Client,
    held: Vec<Held>,
}

impl<'a> Tokens<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self {
            config,
            client: Client::new(),
            held: Vec::new(),
        }
    }

    pub fn config(&self) -> &Config {
        self.config
    }

    /// A token for `repo` with exactly `permissions`, minted on first need
    /// and again once the held one is older than `REUSE`.
    pub fn token(
        &mut self,
        repo: &RepoName,
        permissions: Permissions,
    ) -> Result<(&Client, &Token), Error> {
        self.held.retain(|held| held.minted_at.elapsed() < REUSE);
        let found = self
            .held
            .iter()
            .position(|held| held.repo == *repo && held.permissions == permissions);
        let index = match found {
            Some(index) => index,
            None => {
                let minted = mint(self.config, &self.client, repo, permissions.clone())?;
                self.held.push(Held {
                    repo: repo.clone(),
                    permissions,
                    minted_at: Instant::now(),
                    token: minted.token,
                });
                self.held.len() - 1
            }
        };
        match self.held.get(index) {
            Some(held) => Ok((&self.client, &held.token)),
            None => Err(Error::Failed("held token vanished".to_owned())),
        }
    }
}
