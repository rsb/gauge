//! `repos.toml` in `agent-registry`: every repository in the portfolio and
//! its role. The roles are a closed set; a loop may open only on a `project`
//! or a `reference`.

use std::fmt;
use std::path::Path;

use serde::Deserialize;

use crate::RegistryError;

/// The one schema version this crate reads.
pub const SCHEMA: u32 = 1;

/// A repository's role in the portfolio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// What the loop runs on. Never hosts an attempt.
    Factory,
    /// What the loop produces.
    Project,
    /// A profile's reference app. Governed and hosts attempts.
    Reference,
    /// The record.
    Record,
}

impl Role {
    /// Whether a loop may open on a repo with this role.
    pub fn is_eligible(self) -> bool {
        matches!(self, Self::Project | Self::Reference)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Factory => "factory",
            Self::Project => "project",
            Self::Reference => "reference",
            Self::Record => "record",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether a repo exists yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    Planned,
    Archived,
}

/// What a project or reference builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Service,
    Desktop,
    Lib,
}

/// One `[[repo]]` entry.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repo {
    /// The GitHub repository under `rsb/`.
    pub name: String,
    pub role: Role,
    /// The local checkout, relative to `~/code/software/rsb`.
    pub path: String,
    pub status: Status,
    pub kind: Option<Kind>,
    /// The profile a reference proves.
    pub profile: Option<String>,
}

/// The whole file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repos {
    pub repos: Vec<Repo>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema: u32,
    #[serde(default)]
    repo: Vec<Repo>,
}

impl Repos {
    pub fn load(path: &Path) -> Result<Self, RegistryError> {
        let text = std::fs::read_to_string(path).map_err(|source| RegistryError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&text, path)
    }

    /// Parses the text of the file at `path`. An unknown field, role, status
    /// or kind is an error, as is a schema other than [`SCHEMA`] or a name
    /// listed twice.
    pub fn parse(text: &str, path: &Path) -> Result<Self, RegistryError> {
        let file: File = toml::from_str(text).map_err(|source| RegistryError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        if file.schema != SCHEMA {
            return Err(RegistryError::ReposSchema {
                path: path.to_path_buf(),
                found: file.schema,
            });
        }
        let mut names = std::collections::BTreeSet::new();
        for repo in &file.repo {
            if !names.insert(repo.name.as_str()) {
                return Err(RegistryError::ReposDuplicate {
                    path: path.to_path_buf(),
                    name: repo.name.clone(),
                });
            }
        }
        Ok(Self { repos: file.repo })
    }

    /// The entry named `name`, if the file lists one.
    pub fn get(&self, name: &str) -> Option<&Repo> {
        self.repos.iter().find(|r| r.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Repos, RegistryError> {
        Repos::parse(text, Path::new("repos.toml"))
    }

    const SAMPLE: &str = r#"
schema = 1

[[repo]]
name = "gauge"
role = "factory"
path = "factory/gauge"
status = "active"

[[repo]]
name = "ref-rsb-serverless-stack"
role = "reference"
kind = "service"
profile = "rsb-serverless-stack"
path = "ref/ref-rsb-serverless-stack"
status = "planned"
"#;

    #[test]
    fn parses_roles_and_finds_by_name() {
        let repos = parse(SAMPLE).unwrap();
        assert_eq!(repos.repos.len(), 2);
        let reference = repos.get("ref-rsb-serverless-stack").unwrap();
        assert_eq!(reference.role, Role::Reference);
        assert_eq!(reference.kind, Some(Kind::Service));
        assert_eq!(reference.status, Status::Planned);
        assert_eq!(repos.get("gauge").unwrap().role, Role::Factory);
        assert!(repos.get("lumen").is_none());
    }

    #[test]
    fn only_project_and_reference_are_eligible() {
        assert!(Role::Project.is_eligible());
        assert!(Role::Reference.is_eligible());
        assert!(!Role::Factory.is_eligible());
        assert!(!Role::Record.is_eligible());
    }

    #[test]
    fn an_unknown_role_status_or_kind_is_an_error_not_a_default() {
        for (field, bad) in [
            ("role", "role = \"vendor\"\nstatus = \"active\""),
            ("status", "role = \"project\"\nstatus = \"paused\""),
            (
                "kind",
                "role = \"project\"\nstatus = \"active\"\nkind = \"mobile\"",
            ),
        ] {
            let text = format!("schema = 1\n[[repo]]\nname = \"x\"\npath = \"x\"\n{bad}\n");
            assert!(
                matches!(parse(&text), Err(RegistryError::Parse { .. })),
                "unknown {field} parsed"
            );
        }
    }

    #[test]
    fn refuses_another_schema_unknown_fields_and_duplicates() {
        assert!(matches!(
            parse("schema = 2\n"),
            Err(RegistryError::ReposSchema { found: 2, .. })
        ));
        assert!(matches!(
            parse("schema = 1\nowner = \"rsb\"\n"),
            Err(RegistryError::Parse { .. })
        ));
        let twice = "schema = 1\n[[repo]]\nname = \"a\"\nrole = \"project\"\npath = \"a\"\nstatus = \"active\"\n[[repo]]\nname = \"a\"\nrole = \"record\"\npath = \"b\"\nstatus = \"active\"\n";
        assert!(
            matches!(parse(twice), Err(RegistryError::ReposDuplicate { name, .. }) if name == "a")
        );
    }
}
