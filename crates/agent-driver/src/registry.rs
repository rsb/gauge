//! The `repos.toml` a store is tied to, and the commit of the checkout it was
//! read from, recorded on the events that rest on it.

use std::path::{Path, PathBuf};

use rsb_gauge_schema::{Repos, Role};
use serde::Serialize;

use crate::error::Error;

/// Where the roles came from: the file and its checkout's commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Source {
    pub path: PathBuf,
    /// `None` outside a git checkout, or where HEAD cannot be read.
    pub sha: Option<String>,
}

/// The repos a store's ticks may open loops on, with their roles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registry {
    pub source: Source,
    pub repos: Repos,
}

impl Registry {
    pub fn load(path: &Path) -> Result<Self, Error> {
        let repos = Repos::load(path)?;
        let sha = path.parent().and_then(head);
        Ok(Self {
            source: Source {
                path: path.to_path_buf(),
                sha,
            },
            repos,
        })
    }

    /// The role of `repo`, or `None` when the file does not list it.
    pub fn role(&self, repo: &str) -> Option<Role> {
        self.repos.get(repo).map(|r| r.role)
    }
}

/// The commit HEAD names in the checkout holding `dir`, read from the files
/// under `.git` so no process is started. Uncommitted edits are not seen.
fn head(dir: &Path) -> Option<String> {
    let git = dir
        .ancestors()
        .map(|d| d.join(".git"))
        .find(|g| g.exists())?;
    let git = if git.is_file() {
        // A worktree: `.git` is a file naming the real directory.
        let text = std::fs::read_to_string(&git).ok()?;
        let target = PathBuf::from(text.strip_prefix("gitdir:")?.trim());
        git.parent()?.join(target)
    } else {
        git
    };
    let head = std::fs::read_to_string(git.join("HEAD")).ok()?;
    let head = head.trim();
    let Some(name) = head.strip_prefix("ref:") else {
        return is_sha(head).then(|| head.to_owned());
    };
    let name = name.trim();
    // A worktree keeps its refs in the common directory.
    let common = std::fs::read_to_string(git.join("commondir"))
        .ok()
        .map(|c| git.join(c.trim()))
        .unwrap_or_else(|| git.clone());
    for base in [&git, &common] {
        if let Ok(text) = std::fs::read_to_string(base.join(name)) {
            let sha = text.trim();
            if is_sha(sha) {
                return Some(sha.to_owned());
            }
        }
    }
    let packed = std::fs::read_to_string(common.join("packed-refs")).ok()?;
    packed.lines().find_map(|line| {
        let (sha, refname) = line.split_once(' ')?;
        (refname == name && is_sha(sha)).then(|| sha.to_owned())
    })
}

fn is_sha(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn checkout(head: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git/refs/heads")).unwrap();
        std::fs::write(dir.path().join(".git/HEAD"), head).unwrap();
        std::fs::write(
            dir.path().join("repos.toml"),
            "schema = 1\n[[repo]]\nname = \"lumen\"\nrole = \"project\"\npath = \"p\"\nstatus = \"active\"\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn reads_the_branch_head_loose_or_packed() {
        let dir = checkout("ref: refs/heads/main\n");
        std::fs::write(dir.path().join(".git/refs/heads/main"), format!("{SHA}\n")).unwrap();
        let registry = Registry::load(&dir.path().join("repos.toml")).unwrap();
        assert_eq!(registry.source.sha.as_deref(), Some(SHA));
        assert_eq!(registry.role("lumen"), Some(Role::Project));
        assert_eq!(registry.role("gauge"), None);

        let dir = checkout("ref: refs/heads/main\n");
        std::fs::write(
            dir.path().join(".git/packed-refs"),
            format!("# pack-refs with: peeled\n{SHA} refs/heads/main\n"),
        )
        .unwrap();
        let registry = Registry::load(&dir.path().join("repos.toml")).unwrap();
        assert_eq!(registry.source.sha.as_deref(), Some(SHA));
    }

    #[test]
    fn a_detached_head_is_its_own_sha_and_no_checkout_is_none() {
        let dir = checkout(SHA);
        let registry = Registry::load(&dir.path().join("repos.toml")).unwrap();
        assert_eq!(registry.source.sha.as_deref(), Some(SHA));

        let bare = tempfile::tempdir().unwrap();
        std::fs::write(bare.path().join("repos.toml"), "schema = 1\n").unwrap();
        let registry = Registry::load(&bare.path().join("repos.toml")).unwrap();
        assert_eq!(registry.source.sha, None);
    }
}
