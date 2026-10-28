//! The GitHub port over the REST API, as the App. One token per repo per
//! run, narrowed to that repo with read access to issues and contents, held
//! in memory by [`Tokens`] and never written down.

use std::path::Path;

use crate::config::Config;
use crate::error::Error;
use crate::git;
use crate::github::{Access, Client, ContentEntry, ListQuery, PER_PAGE, Permissions, State, Token};
use crate::repo::RepoName;
use crate::tokens::Tokens;
use crate::world::{Branch, Entry, EntryKind, GitHub, IssueSeen, Labelled, OWNER};

/// The most pages one listing reads: 3,000 issues or events.
const PAGES: u32 = 30;

/// What observing needs: read, nothing written.
fn read() -> Permissions {
    Permissions {
        contents: Some(Access::Read),
        issues: Some(Access::Read),
        pull_requests: None,
    }
}

pub struct Live<'a> {
    /// The driver's config failed to load: every call fails with why.
    tokens: Result<Tokens<'a>, String>,
}

impl<'a> Live<'a> {
    pub fn new(config: Result<&'a Config, String>) -> Self {
        Self {
            tokens: config.map(Tokens::new),
        }
    }

    /// A client and a read token for `repo`.
    fn token(&mut self, repo: &str) -> Result<(&Client, &Token, RepoName), Error> {
        let name = full(repo)?;
        let tokens = self
            .tokens
            .as_mut()
            .map_err(|e| Error::NoConfig(e.clone()))?;
        let (client, token) = tokens.token(&name, read())?;
        Ok((client, token, name))
    }
}

fn full(repo: &str) -> Result<RepoName, Error> {
    format!("{OWNER}/{repo}").parse()
}

impl GitHub for Live<'_> {
    fn reaches(&self, repo: &str) -> bool {
        match (&self.tokens, full(repo)) {
            (Ok(tokens), Ok(name)) => tokens.config().repo(&name).is_ok(),
            _ => false,
        }
    }

    fn labelled(&mut self, repo: &str, label: &str) -> Result<Vec<Labelled>, Error> {
        let (client, token, name) = self.token(repo)?;
        let mut numbers = Vec::new();
        for page in 1..=PAGES {
            let query = ListQuery {
                state: "open",
                label: Some(label.to_owned()),
                per_page: PER_PAGE,
                page,
            };
            let issues = client.issues(token, &name, &query)?;
            let last = issues.len() < usize::from(PER_PAGE);
            numbers.extend(
                issues
                    .into_iter()
                    .filter(|i| !i.is_pull_request())
                    .map(|i| i.number),
            );
            if last {
                break;
            }
        }
        let mut out = Vec::new();
        for issue in numbers {
            let mut event = None;
            for page in 1..=PAGES {
                let events = client.issue_events(token, &name, issue, page)?;
                let last = events.len() < usize::from(PER_PAGE);
                if let Some(found) = events.iter().rev().find(|e| {
                    e.event == "labeled" && e.label.as_ref().is_some_and(|l| l.name == label)
                }) {
                    event = Some(found.id);
                }
                if last {
                    break;
                }
            }
            let event = event.ok_or_else(|| Error::NoLabelEvent {
                repo: name.to_string(),
                issue,
            })?;
            out.push(Labelled { issue, event });
        }
        Ok(out)
    }

    fn issue(&mut self, repo: &str, number: u64) -> Result<Option<IssueSeen>, Error> {
        let (client, token, name) = self.token(repo)?;
        match client.issue(token, &name, number) {
            Ok(issue) => Ok(Some(IssueSeen {
                open: issue.state == State::Open,
                pull_request: issue.is_pull_request(),
                body: issue.body.clone().unwrap_or_default(),
            })),
            // 410 is an issue that was deleted.
            Err(Error::Status {
                status: 404 | 410, ..
            }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn default_branch(&mut self, repo: &str) -> Result<Branch, Error> {
        let (client, token, name) = self.token(repo)?;
        let branch = client.repository(token, &name)?.default_branch;
        let found = client.branch(token, &name, &branch)?;
        Ok(Branch {
            name: found.name,
            sha: found.commit.sha,
        })
    }

    fn list(&mut self, repo: &str, sha: &str, dir: &str) -> Result<Option<Vec<Entry>>, Error> {
        let (client, token, name) = self.token(repo)?;
        let entries = client.list(token, &name, sha, dir)?;
        Ok(entries.map(|entries| entries.into_iter().map(entry).collect()))
    }

    fn read(&mut self, repo: &str, sha: &str, path: &str) -> Result<Option<Vec<u8>>, Error> {
        let (client, token, name) = self.token(repo)?;
        client.read(token, &name, sha, path)
    }

    fn clone_at(&mut self, repo: &str, sha: &str, into: &Path) -> Result<(), Error> {
        let (_, token, name) = self.token(repo)?;
        let url = format!("https://github.com/{name}.git");
        git::fetch_at(&url, sha, into, Some(&token.git_auth()))
    }
}

fn entry(entry: ContentEntry) -> Entry {
    let kind = match entry.kind.as_str() {
        "file" => EntryKind::File,
        "dir" => EntryKind::Dir,
        _ => EntryKind::Other,
    };
    Entry {
        name: entry.name,
        kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reaches_only_what_the_config_lists() {
        let config = Config::parse(
            "[app]\nid = 1\n[repos.\"rsb/ref-rsb-serverless-stack\"]\n",
            Path::new("/cfg"),
        )
        .unwrap();
        let live = Live::new(Ok(&config));
        assert!(live.reaches("ref-rsb-serverless-stack"));
        assert!(!live.reaches("gordana-milan"));
        assert!(!live.reaches("not a name"));
    }

    #[test]
    fn without_a_config_nothing_is_reached_and_every_call_says_why() {
        let mut live = Live::new(Err("cannot read config.toml".to_owned()));
        assert!(!live.reaches("lumen"));
        let err = live.default_branch("lumen").err().unwrap();
        assert_eq!(
            err.to_string(),
            "the driver's config is unavailable: cannot read config.toml"
        );
    }

    #[test]
    fn listing_entries_keep_only_files_and_dirs() {
        let kinds: Vec<EntryKind> = ["file", "dir", "symlink", "submodule"]
            .into_iter()
            .map(|kind| {
                entry(ContentEntry {
                    name: "x".into(),
                    kind: kind.into(),
                })
                .kind
            })
            .collect();
        assert_eq!(
            kinds,
            [
                EntryKind::File,
                EntryKind::Dir,
                EntryKind::Other,
                EntryKind::Other
            ]
        );
    }
}
