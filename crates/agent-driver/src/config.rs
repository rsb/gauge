use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::Error;
use crate::repo::RepoName;

/// `~/.config/rsb/agent-driver/config.toml`, resolved.
#[derive(Debug)]
pub struct Config {
    pub app_id: u64,
    pub key_path: PathBuf,
    repos: BTreeMap<String, RepoConfig>,
}

/// Per-repo settings. Empty until the tick needs them.
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RepoConfig {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    app: AppSection,
    #[serde(default)]
    repos: BTreeMap<String, RepoConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AppSection {
    id: u64,
    /// Relative to the config directory. Defaults to `app.pem` beside it.
    key_path: Option<PathBuf>,
}

impl Config {
    /// Where the config lives: `$XDG_CONFIG_HOME` or `~/.config`, then
    /// `rsb/agent-driver`, per `../rules` naming.
    pub fn dir() -> Result<PathBuf, Error> {
        let base = match std::env::var_os("XDG_CONFIG_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => PathBuf::from(std::env::var_os("HOME").ok_or(Error::NoHome)?).join(".config"),
        };
        Ok(base.join("rsb").join("agent-driver"))
    }

    /// Where the driver keeps its own files: `$XDG_DATA_HOME` or
    /// `~/.local/share`, then `rsb/agent-driver`.
    #[expect(dead_code, reason = "the audit log's home, used by the tick")]
    pub fn data_dir() -> Result<PathBuf, Error> {
        let base = match std::env::var_os("XDG_DATA_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => PathBuf::from(std::env::var_os("HOME").ok_or(Error::NoHome)?)
                .join(".local")
                .join("share"),
        };
        Ok(base.join("rsb").join("agent-driver"))
    }

    pub fn load() -> Result<Self, Error> {
        let dir = Self::dir()?;
        let path = dir.join("config.toml");
        let text = std::fs::read_to_string(&path).map_err(|source| Error::Read {
            path: path.clone(),
            source,
        })?;
        Self::parse(&text, &dir).map_err(|source| Error::Config { path, source })
    }

    /// Parses the file text. Relative paths resolve against `dir`.
    pub(crate) fn parse(text: &str, dir: &Path) -> Result<Self, toml::de::Error> {
        let file: File = toml::from_str(text)?;
        for name in file.repos.keys() {
            if name.parse::<RepoName>().is_err() {
                return Err(serde::de::Error::custom(format!(
                    "repos.{name:?} is not a valid owner/name"
                )));
            }
        }
        let key_path = dir.join(
            file.app
                .key_path
                .unwrap_or_else(|| PathBuf::from("app.pem")),
        );
        Ok(Self {
            app_id: file.app.id,
            key_path,
            repos: file.repos,
        })
    }

    /// The settings for `repo`, or a refusal if it is not listed. Every
    /// operation goes through here first.
    pub fn repo(&self, repo: &RepoName) -> Result<&RepoConfig, Error> {
        self.repos
            .get(&repo.to_string())
            .ok_or_else(|| Error::RepoNotConfigured(repo.to_string()))
    }

    /// The App's private key, PEM. Refuses a key file that others can read.
    pub fn read_key(&self) -> Result<Vec<u8>, Error> {
        let path = &self.key_path;
        let read = |source| Error::Read {
            path: path.clone(),
            source,
        };
        let mode = std::fs::metadata(path).map_err(read)?.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(Error::KeyMode {
                path: path.clone(),
                mode,
            });
        }
        std::fs::read(path).map_err(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[app]
id = 123456

[repos."rsb/lumen"]
[repos."rsb/gm-serverless"]
"#;

    fn parse(text: &str) -> Result<Config, toml::de::Error> {
        Config::parse(text, Path::new("/cfg"))
    }

    #[test]
    fn parses_the_sample_and_defaults_the_key_beside_it() {
        let config = parse(SAMPLE).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(config.app_id, 123456);
        assert_eq!(config.key_path, Path::new("/cfg/app.pem"));
        assert_eq!(config.repos.len(), 2);
    }

    #[test]
    fn key_path_resolves_against_the_config_dir() {
        let config =
            parse("[app]\nid = 1\nkey_path = \"keys/a.pem\"\n").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(config.key_path, Path::new("/cfg/keys/a.pem"));
        let config = parse("[app]\nid = 1\nkey_path = \"/elsewhere/a.pem\"\n")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(config.key_path, Path::new("/elsewhere/a.pem"));
    }

    #[test]
    fn repo_lookup_refuses_what_is_not_listed() {
        let config = parse(SAMPLE).unwrap_or_else(|e| panic!("{e}"));
        let lumen: RepoName = "rsb/lumen".parse().unwrap_or_else(|e| panic!("{e}"));
        let other: RepoName = "rsb/notes".parse().unwrap_or_else(|e| panic!("{e}"));
        assert!(config.repo(&lumen).is_ok());
        assert!(matches!(
            config.repo(&other),
            Err(Error::RepoNotConfigured(name)) if name == "rsb/notes"
        ));
    }

    #[test]
    fn rejects_unknown_keys_and_bad_repo_names() {
        assert!(parse("[app]\nid = 1\ntoken = \"x\"\n").is_err());
        assert!(parse("[app]\nid = 1\n[repos.lumen]\n").is_err());
        assert!(parse("[app]\nid = 1\n[repos.\"rsb/lumen\"]\nimage = \"x\"\n").is_err());
        assert!(parse("").is_err());
    }
}
