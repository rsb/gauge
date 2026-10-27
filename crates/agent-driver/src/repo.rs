use std::fmt;
use std::str::FromStr;

use crate::error::Error;

/// A GitHub repository as `owner/name`, validated before any use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoName {
    owner: String,
    name: String,
}

impl RepoName {
    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for RepoName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

impl serde::Serialize for RepoName {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for RepoName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl FromStr for RepoName {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let invalid = || Error::RepoName(s.to_owned());
        let (owner, name) = s.split_once('/').ok_or_else(invalid)?;
        if !is_owner(owner) || !is_name(name) {
            return Err(invalid());
        }
        Ok(Self {
            owner: owner.to_owned(),
            name: name.to_owned(),
        })
    }
}

/// GitHub user and organisation names: alphanumerics and single hyphens, not
/// at the ends, at most 39 characters.
fn is_owner(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 39
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Repository names: alphanumerics, `-`, `_` and `.`, at most 100 characters,
/// and not a path step.
fn is_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s != "."
        && s != ".."
        && !s.ends_with(".git")
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<RepoName, Error> {
        s.parse()
    }

    #[test]
    fn accepts_owner_slash_name() {
        let repo = parse("rsb/lumen").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(repo.owner(), "rsb");
        assert_eq!(repo.name(), "lumen");
        assert_eq!(repo.to_string(), "rsb/lumen");
        assert!(parse("rsb/gm-serverless").is_ok());
        assert!(parse("my-org/some_repo.v2").is_ok());
    }

    #[test]
    fn refuses_anything_that_is_not_a_plain_pair() {
        for bad in [
            "",
            "lumen",
            "rsb/",
            "/lumen",
            "rsb/lumen/extra",
            "rsb/..",
            "rsb/.",
            "rsb/lumen.git",
            "-rsb/lumen",
            "rsb--x/lumen",
            "rsb/lu men",
            "rsb/lumen?x=1",
            "rsb/lumen#3",
            "rsb/lu\nmen",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} should be refused");
        }
    }
}
