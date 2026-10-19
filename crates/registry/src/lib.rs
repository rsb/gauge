//! The rule registry: one TOML file per rule, read into typed records.
//!
//! The types in [`rule`] are the schema. `toml` reads a file into them,
//! `schemars` derives the JSON Schema from them for the editor and CI, and
//! an unknown field or value is a parse error. The registry content lives in
//! `rsb/rules`; this crate only reads it. The agent never reads a rule file,
//! only the projections `render` makes from it.

mod error;
pub mod lint;
pub mod rule;

use std::path::{Path, PathBuf};

pub use error::RegistryError;
pub use lint::{Finding, Problem, lint};
pub use rule::Rule;

/// One rule and where it came from. The topic is the rule's directory
/// relative to the registry root, such as `errors` or `web/cloudflare`.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub topic: String,
    pub rule: Rule,
}

/// Every rule under a root, in path order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Registry {
    pub entries: Vec<Entry>,
}

impl Registry {
    /// Reads every `.toml` file under `root`, recursively, skipping hidden
    /// directories. The first file that cannot be read or parsed stops the
    /// load: a registry is whole or it is not loaded.
    pub fn load(root: &Path) -> Result<Self, RegistryError> {
        if !root.is_dir() {
            return Err(RegistryError::NotDirectory(root.to_path_buf()));
        }
        let mut files = Vec::new();
        collect(root, &mut files)?;
        files.sort();
        let entries = files
            .into_iter()
            .map(|path| {
                let text =
                    std::fs::read_to_string(&path).map_err(|source| RegistryError::Read {
                        path: path.clone(),
                        source,
                    })?;
                let rule = toml::from_str(&text).map_err(|source| RegistryError::Parse {
                    path: path.clone(),
                    source,
                })?;
                let topic = path
                    .parent()
                    .and_then(|dir| dir.strip_prefix(root).ok())
                    .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                Ok(Entry { path, topic, rule })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { entries })
    }

    /// The rule with `id`, if the registry has one.
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.rule.id == id)
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), RegistryError> {
    let read = |source| RegistryError::Read {
        path: dir.to_path_buf(),
        source,
    };
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
            collect(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "toml") {
            out.push(path);
        }
    }
    Ok(())
}

/// The JSON Schema for one rule file, for taplo in the editor and the
/// validation step in CI.
pub fn schema() -> schemars::Schema {
    schemars::schema_for!(Rule)
}
