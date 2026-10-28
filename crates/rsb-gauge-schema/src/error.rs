use std::path::PathBuf;

/// Why a registry could not be loaded. A lint finding is not an error; it
/// is an outcome the caller reports.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("{0} is not a directory")]
    NotDirectory(PathBuf),
    #[error("cannot read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot parse {path}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("{path} has schema {found}; this build reads schema 1")]
    ReposSchema { path: PathBuf, found: u32 },
    #[error("{path} lists {name} twice")]
    ReposDuplicate { path: PathBuf, name: String },
}
