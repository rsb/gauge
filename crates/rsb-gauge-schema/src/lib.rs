//! The rule registry: one TOML file per rule or heuristic, read into typed
//! records.
//!
//! The types in [`rule`] are the schema. `toml` reads a file into them,
//! `schemars` derives the JSON Schema from them for the editor and CI, and
//! an unknown field or value is a parse error. The registry content lives in
//! `rsb/agent-registry`; this crate only reads it. The agent never reads a rule file,
//! only the projections `render` makes from it. [`repos`] reads `repos.toml`,
//! the portfolio and each repo's role.

mod error;
pub mod lint;
pub mod repos;
pub mod rule;

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

pub use error::RegistryError;
pub use lint::{Finding, Problem, lint};
pub use repos::{Repo, Repos, Role};
pub use rule::{Heuristic, Rule};

/// One rule or heuristic and the file it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry<T> {
    pub path: PathBuf,
    pub item: T,
}

/// Every rule and every heuristic under a registry root, each in path order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Registry {
    pub rules: Vec<Entry<Rule>>,
    pub heuristics: Vec<Entry<Heuristic>>,
}

/// The two kinds of registry file, each with its own directory, id prefix
/// and JSON Schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rule,
    Heuristic,
}

impl Kind {
    /// The directory under the registry root that holds this kind.
    pub fn dir(self) -> &'static str {
        match self {
            Self::Rule => "rules",
            Self::Heuristic => "heuristics",
        }
    }

    /// The id prefix: an id is `<prefix>-NNNN`.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Rule => "rl",
            Self::Heuristic => "hr",
        }
    }
}

impl Registry {
    /// Reads `<root>/rules/*.toml` and `<root>/heuristics/*.toml`. Both
    /// directories are flat; `heuristics/` may be absent, `rules/` may not.
    /// A file is refused unless it is named `<prefix>-NNNN.toml` for its
    /// kind and holds that id. The first file that cannot be read, parsed or
    /// named stops the load: a registry is whole or it is not loaded.
    pub fn load(root: &Path) -> Result<Self, RegistryError> {
        if !root.is_dir() {
            return Err(RegistryError::NotDirectory(root.to_path_buf()));
        }
        let rules_dir = root.join(Kind::Rule.dir());
        if !rules_dir.is_dir() {
            return Err(RegistryError::NotDirectory(rules_dir));
        }
        let rules = read_kind(&rules_dir, Kind::Rule, |r: &Rule| &r.id)?;
        let heuristics_dir = root.join(Kind::Heuristic.dir());
        let heuristics = if heuristics_dir.exists() {
            read_kind(&heuristics_dir, Kind::Heuristic, |h: &Heuristic| &h.id)?
        } else {
            Vec::new()
        };
        Ok(Self { rules, heuristics })
    }

    /// The rule with `id`, if the registry has one.
    pub fn rule(&self, id: &str) -> Option<&Entry<Rule>> {
        self.rules.iter().find(|e| e.item.id == id)
    }

    /// The heuristic with `id`, if the registry has one.
    pub fn heuristic(&self, id: &str) -> Option<&Entry<Heuristic>> {
        self.heuristics.iter().find(|e| e.item.id == id)
    }
}

fn read_kind<T: DeserializeOwned>(
    dir: &Path,
    kind: Kind,
    id: impl Fn(&T) -> &String,
) -> Result<Vec<Entry<T>>, RegistryError> {
    let mut files = list(dir)?;
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if !is_id(stem, kind.prefix()) {
                return Err(RegistryError::FileName {
                    path,
                    prefix: kind.prefix(),
                });
            }
            let text = std::fs::read_to_string(&path).map_err(|source| RegistryError::Read {
                path: path.clone(),
                source,
            })?;
            let item: T = toml::from_str(&text).map_err(|source| RegistryError::Parse {
                path: path.clone(),
                source,
            })?;
            if id(&item) != stem {
                let id = id(&item).clone();
                return Err(RegistryError::IdMismatch { path, id });
            }
            Ok(Entry { path, item })
        })
        .collect()
}

/// The `.toml` files directly in `dir`, skipping hidden entries. A
/// subdirectory is refused rather than skipped, so a file in one is never
/// silently left out.
fn list(dir: &Path) -> Result<Vec<PathBuf>, RegistryError> {
    let read = |source| RegistryError::Read {
        path: dir.to_path_buf(),
        source,
    };
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(read)? {
        let path = entry.map_err(read)?.path();
        let hidden = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'));
        if hidden {
            continue;
        }
        if path.is_dir() {
            return Err(RegistryError::Nested(path));
        }
        if path.extension().is_some_and(|e| e == "toml") {
            out.push(path);
        }
    }
    Ok(out)
}

/// `<prefix>-NNNN`: the prefix, a dash, four digits.
fn is_id(text: &str, prefix: &str) -> bool {
    text.strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('-'))
        .is_some_and(|n| n.len() == 4 && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The JSON Schema for one rule or heuristic file, for taplo in the editor
/// and the validation step in CI.
pub fn schema(kind: Kind) -> schemars::Schema {
    match kind {
        Kind::Rule => schemars::schema_for!(Rule),
        Kind::Heuristic => schemars::schema_for!(Heuristic),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_the_prefix_and_four_digits() {
        assert!(is_id("rl-0001", "rl"));
        assert!(is_id("hr-0420", "hr"));
        assert!(!is_id("hr-0001", "rl"), "the other kind's prefix");
        assert!(!is_id("rl-1", "rl"), "four digits");
        assert!(!is_id("rl-00001", "rl"), "four digits, not five");
        assert!(!is_id("ERR-1", "rl"), "the old topic ids");
        assert!(!is_id("rl0001", "rl"), "a dash");
    }
}
