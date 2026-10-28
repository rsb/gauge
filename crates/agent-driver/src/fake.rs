//! The GitHub port in memory, scripted: repos with a default branch, files
//! at its one commit, issues and the label events on them. It clones from a
//! local bare repository when one is given.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rsb_gauge_reconcile::UnitSeen;
use rsb_gauge_reconcile::adapter::RunOutput;
use rsb_gauge_reconcile::store::{LoopId, SkillsSource};

use crate::attempt::{Attempts, Launch, Workspace};
use crate::error::Error;
use crate::git;
use crate::world::{Branch, Entry, EntryKind, GitHub, IssueSeen, LOCKFILE, Labelled};

/// The commit every fake repo's default branch names, unless a bare
/// repository gives a real one.
pub const SHA: &str = "feedfacefeedfacefeedfacefeedfacefeedface";

#[derive(Clone, Debug, PartialEq, Eq)]
struct FakeIssue {
    open: bool,
    pull_request: bool,
    body: String,
    /// Labels on the issue now, each with the event that applied it.
    labels: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FakeRepo {
    branch: String,
    sha: String,
    files: BTreeMap<String, Vec<u8>>,
    issues: BTreeMap<u64, FakeIssue>,
    bare: Option<PathBuf>,
    /// Every call on this repo fails, as GitHub down would.
    failing: bool,
}

impl FakeRepo {
    fn new() -> Self {
        Self {
            branch: "main".to_owned(),
            sha: SHA.to_owned(),
            files: BTreeMap::from([(LOCKFILE.to_owned(), b"lockfile".to_vec())]),
            issues: BTreeMap::new(),
            bare: None,
            failing: false,
        }
    }

    pub fn file(mut self, path: &str) -> Self {
        self.files.insert(path.to_owned(), b"# ticket\n".to_vec());
        self
    }

    pub fn lockfile(mut self, content: &[u8]) -> Self {
        self.files.insert(LOCKFILE.to_owned(), content.to_vec());
        self
    }

    pub fn without_lockfile(mut self) -> Self {
        self.files.remove(LOCKFILE);
        self
    }

    fn with_issue(mut self, number: u64, body: &str, open: bool, pull_request: bool) -> Self {
        self.issues.insert(
            number,
            FakeIssue {
                open,
                pull_request,
                body: body.to_owned(),
                labels: BTreeMap::new(),
            },
        );
        self
    }

    pub fn issue(self, number: u64, body: &str) -> Self {
        self.with_issue(number, body, true, false)
    }

    pub fn closed_issue(self, number: u64, body: &str) -> Self {
        self.with_issue(number, body, false, false)
    }

    pub fn pull_request(self, number: u64, body: &str) -> Self {
        self.with_issue(number, body, true, true)
    }

    /// Clones come from the bare repository at `path`, whose commit `sha`
    /// the default branch then names.
    pub fn bare(mut self, path: &Path, sha: &str) -> Self {
        self.bare = Some(path.to_path_buf());
        self.sha = sha.to_owned();
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FakeGitHub {
    repos: BTreeMap<String, FakeRepo>,
    /// The last label event id handed out; events rise across the world.
    events: u64,
}

impl FakeGitHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a repo the driver reaches, built by `build`.
    pub fn repo(mut self, name: &str, build: impl FnOnce(FakeRepo) -> FakeRepo) -> Self {
        self.repos.insert(name.to_owned(), build(FakeRepo::new()));
        self
    }

    fn issue_mut(&mut self, repo: &str, number: u64) -> &mut FakeIssue {
        let repo = self
            .repos
            .get_mut(repo)
            .unwrap_or_else(|| panic!("no fake repo {repo}"));
        repo.issues.entry(number).or_insert_with(|| FakeIssue {
            open: true,
            pull_request: false,
            body: String::new(),
            labels: BTreeMap::new(),
        })
    }

    /// Applies `label` to the issue, as a new event, creating the issue
    /// with an empty body if the script has none. Returns the event id.
    pub fn label(&mut self, repo: &str, number: u64, label: &str) -> u64 {
        self.events += 1;
        let event = self.events;
        self.issue_mut(repo, number)
            .labels
            .insert(label.to_owned(), event);
        event
    }

    pub fn unlabel(&mut self, repo: &str, number: u64, label: &str) {
        self.issue_mut(repo, number).labels.remove(label);
    }

    pub fn close(&mut self, repo: &str, number: u64) {
        self.issue_mut(repo, number).open = false;
    }

    /// Every later call on `repo` fails, or works again.
    pub fn fail(&mut self, repo: &str, failing: bool) {
        if let Some(r) = self.repos.get_mut(repo) {
            r.failing = failing;
        }
    }

    fn get(&self, repo: &str) -> Result<&FakeRepo, Error> {
        match self.repos.get(repo) {
            Some(r) if r.failing => Err(Error::Status {
                what: "fake",
                status: 503,
            }),
            Some(r) => Ok(r),
            None => Err(Error::NotInstalled(repo.to_owned())),
        }
    }

    fn at<'a>(&'a self, repo: &str, sha: &str) -> Result<Option<&'a FakeRepo>, Error> {
        let r = self.get(repo)?;
        Ok((r.sha == sha).then_some(r))
    }
}

impl GitHub for FakeGitHub {
    fn reaches(&self, repo: &str) -> bool {
        self.repos.contains_key(repo)
    }

    fn labelled(&mut self, repo: &str, label: &str) -> Result<Vec<Labelled>, Error> {
        Ok(self
            .get(repo)?
            .issues
            .iter()
            .filter(|(_, i)| i.open && !i.pull_request)
            .filter_map(|(&issue, i)| i.labels.get(label).map(|&event| Labelled { issue, event }))
            .collect())
    }

    fn issue(&mut self, repo: &str, number: u64) -> Result<Option<IssueSeen>, Error> {
        Ok(self.get(repo)?.issues.get(&number).map(|i| IssueSeen {
            open: i.open,
            pull_request: i.pull_request,
            body: i.body.clone(),
        }))
    }

    fn default_branch(&mut self, repo: &str) -> Result<Branch, Error> {
        let r = self.get(repo)?;
        Ok(Branch {
            name: r.branch.clone(),
            sha: r.sha.clone(),
        })
    }

    fn list(&mut self, repo: &str, sha: &str, dir: &str) -> Result<Option<Vec<Entry>>, Error> {
        let Some(r) = self.at(repo, sha)? else {
            return Ok(None);
        };
        let prefix = format!("{dir}/");
        let mut dirs = BTreeSet::new();
        let mut files = BTreeSet::new();
        for path in r.files.keys() {
            let Some(rest) = path.strip_prefix(&prefix) else {
                continue;
            };
            match rest.split_once('/') {
                Some((sub, _)) => dirs.insert(sub.to_owned()),
                None => files.insert(rest.to_owned()),
            };
        }
        if dirs.is_empty() && files.is_empty() {
            return Ok(None);
        }
        let entry = |kind| move |name| Entry { name, kind };
        Ok(Some(
            dirs.into_iter()
                .map(entry(EntryKind::Dir))
                .chain(files.into_iter().map(entry(EntryKind::File)))
                .collect(),
        ))
    }

    fn read(&mut self, repo: &str, sha: &str, path: &str) -> Result<Option<Vec<u8>>, Error> {
        Ok(self.at(repo, sha)?.and_then(|r| r.files.get(path).cloned()))
    }

    fn clone_at(&mut self, repo: &str, sha: &str, into: &Path) -> Result<(), Error> {
        let r = self.get(repo)?;
        let bare = r
            .bare
            .as_ref()
            .ok_or_else(|| Error::Failed(format!("fake repo {repo} has no bare repository")))?;
        git::fetch_at(&format!("file://{}", bare.display()), sha, into, None)
    }
}

/// The ticket every fake workspace's clone holds: a feature, so its branch
/// is `feat/<issue>-<slug>`.
pub const FAKE_TICKET: &str = "---\nid: S-0001/1\nslice: feature\n---\n\n# The first ticket\n";

/// The skills source every fake attempt records.
pub fn fake_skills() -> SkillsSource {
    SkillsSource {
        commit: "5eed5eed5eed5eed5eed5eed5eed5eed5eed5eed".to_owned(),
        clean: true,
    }
}

/// The attempts port in memory: units by name, scripted as running, exited
/// or gone; workspaces by directory, holding the brief written and whether
/// the outbox is sealed. Every start is kept, refused ones too, so a test
/// can count them per unit name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FakeAttempts {
    pub units: BTreeMap<String, UnitSeen>,
    /// Every `start` call, in order.
    pub starts: Vec<Launch>,
    pub stops: Vec<String>,
    /// `run/brief.json` per attempt directory, as last written.
    pub briefs: BTreeMap<PathBuf, serde_json::Value>,
    /// The branch each workspace was prepared on.
    pub branches: BTreeMap<PathBuf, String>,
    pub sealed: BTreeSet<PathBuf>,
    /// What each run left; an empty output when none is scripted.
    pub outputs: BTreeMap<PathBuf, RunOutput>,
    pub ticket: Option<String>,
}

impl Default for FakeAttempts {
    fn default() -> Self {
        Self {
            units: BTreeMap::new(),
            starts: Vec::new(),
            stops: Vec::new(),
            briefs: BTreeMap::new(),
            branches: BTreeMap::new(),
            sealed: BTreeSet::new(),
            outputs: BTreeMap::new(),
            ticket: Some(FAKE_TICKET.to_owned()),
        }
    }
}

impl FakeAttempts {
    pub fn new() -> Self {
        Self::default()
    }

    /// The unit `name` now shows as `seen`.
    pub fn set(&mut self, name: &str, seen: UnitSeen) {
        self.units.insert(name.to_owned(), seen);
    }

    /// Seals the outbox of the attempt in `dir`.
    pub fn seal(&mut self, dir: &Path) {
        self.sealed.insert(dir.to_path_buf());
    }
}

impl Attempts for FakeAttempts {
    fn dir(&self, loop_id: LoopId, attempt: u32) -> PathBuf {
        PathBuf::from(format!("/fake/runs/{loop_id}/{attempt}"))
    }

    fn unit(&mut self, name: &str) -> Result<UnitSeen, Error> {
        Ok(self.units.get(name).copied().unwrap_or(UnitSeen::Absent))
    }

    fn running(&mut self) -> Result<BTreeSet<String>, Error> {
        Ok(self
            .units
            .iter()
            .filter(|(name, seen)| name.starts_with("gauge-run-") && **seen == UnitSeen::Running)
            .map(|(name, _)| name.clone())
            .collect())
    }

    fn start(&mut self, launch: &Launch) -> Result<(), Error> {
        self.starts.push(launch.clone());
        if self.unit(&launch.unit)? != UnitSeen::Absent {
            return Err(Error::Command {
                what: "systemd-run".to_owned(),
                status: Some(1),
                stderr: format!("Unit {}.service was already loaded", launch.unit),
            });
        }
        self.set(&launch.unit, UnitSeen::Running);
        Ok(())
    }

    fn stop(&mut self, name: &str) -> Result<(), Error> {
        self.stops.push(name.to_owned());
        self.units.remove(name);
        Ok(())
    }

    fn ticket(
        &mut self,
        _github: &mut dyn GitHub,
        _dir: &Path,
        _ws: &Workspace<'_>,
    ) -> Result<Option<String>, Error> {
        Ok(self.ticket.clone())
    }

    fn prepare(
        &mut self,
        _github: &mut dyn GitHub,
        dir: &Path,
        _ws: &Workspace<'_>,
        branch: &str,
        brief: &serde_json::Value,
    ) -> Result<(), Error> {
        self.branches.insert(dir.to_path_buf(), branch.to_owned());
        self.briefs.insert(dir.to_path_buf(), brief.clone());
        Ok(())
    }

    fn sealed(&mut self, dir: &Path) -> Result<bool, Error> {
        Ok(self.sealed.contains(dir))
    }

    fn output(
        &mut self,
        dir: &Path,
        _base_sha: &str,
        _branch: &str,
        exit: Option<i32>,
    ) -> Result<RunOutput, Error> {
        let mut output = self.outputs.get(dir).cloned().unwrap_or_default();
        output.exit = exit;
        Ok(output)
    }

    fn skills(&mut self, _checkout: &Path) -> Result<SkillsSource, Error> {
        Ok(fake_skills())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::LABEL;

    #[test]
    fn a_label_is_seen_until_it_is_removed_or_the_issue_closes() {
        let mut github = FakeGitHub::new().repo("lumen", |r| r.issue(1, "").issue(2, ""));
        assert!(github.labelled("lumen", LABEL).unwrap().is_empty());
        let first = github.label("lumen", 1, LABEL);
        github.label("lumen", 2, "bug");
        assert_eq!(
            github.labelled("lumen", LABEL).unwrap(),
            [Labelled {
                issue: 1,
                event: first
            }]
        );
        github.unlabel("lumen", 1, LABEL);
        assert!(github.labelled("lumen", LABEL).unwrap().is_empty());
        let again = github.label("lumen", 1, LABEL);
        assert!(again > first);
        github.close("lumen", 1);
        assert!(github.labelled("lumen", LABEL).unwrap().is_empty());
    }

    #[test]
    fn clones_from_the_bare_repository_at_its_sha() {
        let dir = tempfile::tempdir().unwrap();
        let (bare, sha) = crate::git::tests::bare(dir.path());
        let mut github = FakeGitHub::new().repo("lumen", |r| r.bare(&bare, &sha));
        assert_eq!(github.default_branch("lumen").unwrap().sha, sha);
        let into = dir.path().join("attempt/repo");
        github.clone_at("lumen", &sha, &into).unwrap();
        assert!(into.join(".rsb/governance.lock").is_file());
    }
}
