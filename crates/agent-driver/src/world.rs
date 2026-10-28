//! The GitHub port: what the tick observes on GitHub, and the clone ticket 06
//! starts an attempt from. The live adapter is in `live.rs`; the fake, for
//! the tick's tests and the kill harness, in `fake.rs`. Repos are named as
//! `repos.toml` names them, bare, under [`OWNER`].

use std::path::Path;

use rsb_gauge_reconcile::{TicketCause, TicketSeen, WorldSeen};
use sha2::{Digest, Sha256};

use crate::error::Error;

/// The account every repo in `repos.toml` lives under.
pub const OWNER: &str = "rsb";

/// The label that asks for a run (record 1 §2).
pub const LABEL: &str = "rsb-workflow:run";

/// The lockfile whose presence on the default branch makes a repo eligible
/// (record 1, 1a). Its content is hashed, not validated, at M1.
pub const LOCKFILE: &str = ".rsb/governance.lock";

/// Where the spec folders live, from the repo's root.
pub const SPECS: &str = "specs";

/// An open issue carrying the run label, and the event that last applied it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Labelled {
    pub issue: u64,
    pub event: u64,
}

/// An issue, cut to what the tick reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueSeen {
    pub open: bool,
    pub pull_request: bool,
    pub body: String,
}

/// A branch and the commit it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub sha: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    /// A symlink or submodule: never a spec folder or a ticket file.
    Other,
}

/// One entry of a directory listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
}

/// GitHub, as the tick sees it. Every call is a read except
/// [`clone_at`](Self::clone_at), which writes only under the path it is
/// given; none writes to GitHub (that is ticket 07).
pub trait GitHub {
    /// Whether the driver is set up to reach `repo`. A repo it does not
    /// reach is not polled.
    fn reaches(&self, repo: &str) -> bool;

    /// The open issues on `repo` carrying `label`, pull requests left out.
    fn labelled(&mut self, repo: &str, label: &str) -> Result<Vec<Labelled>, Error>;

    /// The issue numbered `number`, or `None` when there is none.
    fn issue(&mut self, repo: &str, number: u64) -> Result<Option<IssueSeen>, Error>;

    /// The default branch and the commit it names now.
    fn default_branch(&mut self, repo: &str) -> Result<Branch, Error>;

    /// The entries of the directory `dir` at `sha`, or `None` when there is
    /// no such directory.
    fn list(&mut self, repo: &str, sha: &str, dir: &str) -> Result<Option<Vec<Entry>>, Error>;

    /// The bytes of the file `path` at `sha`, or `None` when there is none.
    fn read(&mut self, repo: &str, sha: &str, path: &str) -> Result<Option<Vec<u8>>, Error>;

    /// Fetches `repo` at `sha` into `into`, which must be absent or empty,
    /// with `sha` checked out. The credential is passed per command and is
    /// written nowhere: not to `.git/config`, not to disk.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "ticket 06 clones a repo for its first attempt")
    )]
    fn clone_at(&mut self, repo: &str, sha: &str, into: &Path) -> Result<(), Error>;
}

/// Reads the world for one issue: the default branch, the lockfile on it and
/// the ticket the issue names, all at one commit.
pub fn observe(github: &mut impl GitHub, repo: &str, issue: u64) -> Result<WorldSeen, Error> {
    let branch = github.default_branch(repo)?;
    let lockfile = github
        .read(repo, &branch.sha, LOCKFILE)?
        .map(|bytes| sha256(&bytes));
    let ticket = match github.issue(repo, issue)? {
        None => unresolved(TicketCause::IssueMissing),
        Some(seen) if seen.pull_request => unresolved(TicketCause::PullRequest),
        Some(seen) if !seen.open => unresolved(TicketCause::IssueClosed),
        Some(seen) => match marker(&seen.body) {
            Ok(id) => resolve(github, repo, &branch.sha, &id)?,
            Err(cause) => unresolved(cause),
        },
    };
    Ok(WorldSeen {
        branch: branch.name,
        sha: branch.sha,
        lockfile,
        ticket,
    })
}

fn unresolved(cause: TicketCause) -> TicketSeen {
    TicketSeen::Unresolved { cause }
}

/// The SHA-256 of `bytes`, in lowercase hex.
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A ticket id as `rsb-skills`' `scripts/spec` writes it: `S-NNNN/N`, where
/// `N` is the ticket file's two-digit number read as an integer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketId {
    /// `S-NNNN`.
    pub spec: String,
    pub number: u32,
}

impl std::fmt::Display for TicketId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.spec, self.number)
    }
}

/// The ticket an issue's body names. `scripts/spec apply` writes
/// `<!-- rsb-ticket S-NNNN/N -->` on a ticket's issue and
/// `<!-- rsb-spec S-NNNN -->` on its epic, and finds issues by the same
/// markers, so they are the ids' one format.
pub fn marker(body: &str) -> Result<TicketId, TicketCause> {
    const OPEN: &str = "<!-- rsb-";
    const CLOSE: &str = " -->";
    let mut tickets: Vec<TicketId> = Vec::new();
    let mut epic = false;
    let mut rest = body;
    while let Some(start) = rest.find(OPEN) {
        rest = rest.get(start + OPEN.len()..).unwrap_or_default();
        let Some(end) = rest.find(CLOSE) else { break };
        let inner = rest.get(..end).unwrap_or_default();
        match inner.split_once(' ') {
            Some(("ticket", id)) => {
                if let Some(id) = ticket_id(id)
                    && !tickets.contains(&id)
                {
                    tickets.push(id);
                }
            }
            Some(("spec", id)) if spec_id(id) => epic = true,
            _ => {}
        }
    }
    match tickets.as_slice() {
        [one] => Ok(one.clone()),
        [] if epic => Err(TicketCause::Epic),
        [] => Err(TicketCause::NoMarker),
        _ => Err(TicketCause::TwoTickets),
    }
}

/// `S-` and four digits.
fn spec_id(s: &str) -> bool {
    s.strip_prefix("S-")
        .is_some_and(|n| n.len() == 4 && n.bytes().all(|b| b.is_ascii_digit()))
}

fn ticket_id(s: &str) -> Option<TicketId> {
    let (spec, number) = s.split_once('/')?;
    if !spec_id(spec) || number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(TicketId {
        spec: spec.to_owned(),
        number: number.parse().ok()?,
    })
}

/// A slug as `scripts/spec` checks one: `[a-z0-9]+(-[a-z0-9]+)*`.
fn slug(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// Resolves `id` to `specs/S-NNNN-<slug>/NN-<slug>.md` at `sha`: exactly one
/// spec folder for the spec, and in it exactly one file for the number.
fn resolve(
    github: &mut impl GitHub,
    repo: &str,
    sha: &str,
    id: &TicketId,
) -> Result<TicketSeen, Error> {
    let folders = github.list(repo, sha, SPECS)?.unwrap_or_default();
    let prefix = format!("{}-", id.spec);
    let folder = match only(&folders, |e| {
        e.kind == EntryKind::Dir && e.name.strip_prefix(&prefix).is_some_and(slug)
    }) {
        Some(folder) => folder,
        None => return Ok(unresolved(TicketCause::NoSpecFolder)),
    };
    let dir = format!("{SPECS}/{folder}");
    let files = github.list(repo, sha, &dir)?.unwrap_or_default();
    let prefix = format!("{:02}-", id.number);
    let file = only(&files, |e| {
        e.kind == EntryKind::File
            && e.name
                .strip_prefix(&prefix)
                .and_then(|rest| rest.strip_suffix(".md"))
                .is_some_and(slug)
    });
    Ok(match file {
        Some(file) => TicketSeen::Resolved {
            id: id.to_string(),
            path: format!("{dir}/{file}"),
        },
        None => unresolved(TicketCause::NoTicketFile),
    })
}

/// The name of the one entry matching `test`, or `None` for none or several.
fn only(entries: &[Entry], test: impl Fn(&Entry) -> bool) -> Option<String> {
    let mut found = entries.iter().filter(|e| test(e));
    match (found.next(), found.next()) {
        (Some(one), None) => Some(one.name.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeGitHub;

    fn id(spec: &str, number: u32) -> TicketId {
        TicketId {
            spec: spec.into(),
            number,
        }
    }

    #[test]
    fn reads_the_marker_spec_apply_writes() {
        let body = "<!-- rsb-ticket S-0001/5 -->\n\n# The world observed\n";
        assert_eq!(marker(body), Ok(id("S-0001", 5)));
        // The same ticket named twice is still one.
        let twice = format!("{body}\n<!-- rsb-ticket S-0001/5 -->\n");
        assert_eq!(marker(&twice), Ok(id("S-0001", 5)));
        assert_eq!(
            marker("<!-- rsb-ticket S-0012/10 -->"),
            Ok(id("S-0012", 10))
        );
    }

    #[test]
    fn a_body_naming_no_one_ticket_says_why() {
        assert_eq!(marker(""), Err(TicketCause::NoMarker));
        assert_eq!(marker("Fix the thing."), Err(TicketCause::NoMarker));
        assert_eq!(
            marker("<!-- rsb-spec S-0001 -->\n\nSpec: ..."),
            Err(TicketCause::Epic)
        );
        assert_eq!(
            marker("<!-- rsb-ticket S-0001/5 -->\n<!-- rsb-ticket S-0001/6 -->"),
            Err(TicketCause::TwoTickets)
        );
        for bad in [
            "<!-- rsb-ticket S-0001 -->",
            "<!-- rsb-ticket S-001/5 -->",
            "<!-- rsb-ticket S-0001/ -->",
            "<!-- rsb-ticket S-0001/5a -->",
            "<!-- rsb-ticket s-0001/5 -->",
            "<!-- rsb-ticket  S-0001/5 -->",
            "<!--rsb-ticket S-0001/5 -->",
            "<!-- rsb-ticket S-0001/5-->",
            "<!-- rsb-tickets S-0001/5 -->",
        ] {
            assert_eq!(marker(bad), Err(TicketCause::NoMarker), "{bad:?}");
        }
    }

    #[test]
    fn resolves_the_one_folder_and_file_on_the_default_branch() {
        let mut github = FakeGitHub::new().repo("lumen", |r| {
            r.file("specs/S-0001-thin-loop/05-world-observed.md")
                .file("specs/S-0001-thin-loop/spec.md")
                .file("specs/S-0001-thin-loop/15-later.md")
                .file("specs/S-0002-other/05-elsewhere.md")
                .issue(3, "<!-- rsb-ticket S-0001/5 -->")
                .issue(4, "<!-- rsb-ticket S-0001/6 -->")
                .issue(5, "<!-- rsb-ticket S-0003/1 -->")
        });
        let world = observe(&mut github, "lumen", 3).unwrap();
        assert_eq!(
            world.ticket,
            TicketSeen::Resolved {
                id: "S-0001/5".into(),
                path: "specs/S-0001-thin-loop/05-world-observed.md".into()
            }
        );
        assert_eq!(world.branch, "main");
        assert_eq!(world.lockfile, Some(sha256(b"lockfile")));
        assert_eq!(
            observe(&mut github, "lumen", 4).unwrap().ticket,
            unresolved(TicketCause::NoTicketFile)
        );
        assert_eq!(
            observe(&mut github, "lumen", 5).unwrap().ticket,
            unresolved(TicketCause::NoSpecFolder)
        );
    }

    #[test]
    fn an_issue_that_is_missing_closed_or_a_pull_request_resolves_nothing() {
        let mut github = FakeGitHub::new().repo("lumen", |r| {
            r.file("specs/S-0001-thin-loop/01-a.md")
                .closed_issue(2, "<!-- rsb-ticket S-0001/1 -->")
                .pull_request(3, "<!-- rsb-ticket S-0001/1 -->")
        });
        for (number, cause) in [
            (1, TicketCause::IssueMissing),
            (2, TicketCause::IssueClosed),
            (3, TicketCause::PullRequest),
        ] {
            assert_eq!(
                observe(&mut github, "lumen", number).unwrap().ticket,
                unresolved(cause)
            );
        }
    }

    #[test]
    fn two_folders_for_one_spec_or_two_files_for_one_number_resolve_nothing() {
        let mut github = FakeGitHub::new()
            .repo("lumen", |r| {
                r.file("specs/S-0001-thin-loop/01-a.md")
                    .file("specs/S-0001-thick-loop/01-a.md")
                    .issue(1, "<!-- rsb-ticket S-0001/1 -->")
            })
            .repo("libraw", |r| {
                r.file("specs/S-0001-thin-loop/01-a.md")
                    .file("specs/S-0001-thin-loop/01-b.md")
                    .file("specs/S-0001-thin-loop/02-Bad_Name.md")
                    .file("specs/S-0001-thin-loop/02-c.txt")
                    .issue(1, "<!-- rsb-ticket S-0001/1 -->")
                    .issue(2, "<!-- rsb-ticket S-0001/2 -->")
            });
        assert_eq!(
            observe(&mut github, "lumen", 1).unwrap().ticket,
            unresolved(TicketCause::NoSpecFolder)
        );
        assert_eq!(
            observe(&mut github, "libraw", 1).unwrap().ticket,
            unresolved(TicketCause::NoTicketFile)
        );
        assert_eq!(
            observe(&mut github, "libraw", 2).unwrap().ticket,
            unresolved(TicketCause::NoTicketFile)
        );
    }

    #[test]
    fn no_lockfile_is_none_and_the_hash_is_of_the_content() {
        let mut github = FakeGitHub::new()
            .repo("lumen", |r| r.without_lockfile().issue(1, ""))
            .repo("libraw", |r| r.lockfile(b"[root]\n").issue(1, ""));
        assert_eq!(observe(&mut github, "lumen", 1).unwrap().lockfile, None);
        assert_eq!(
            observe(&mut github, "libraw", 1).unwrap().lockfile,
            Some(sha256(b"[root]\n"))
        );
        // The known SHA-256 of nothing, so the hex is the standard one.
        assert_eq!(
            sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
