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
    #[error("{0} is a directory; rules/ and heuristics/ are flat")]
    Nested(PathBuf),
    #[error("{path} is not named {prefix}-NNNN.toml")]
    FileName { path: PathBuf, prefix: &'static str },
    #[error("{path} holds {id:?}; a file is named for the id it holds")]
    IdMismatch { path: PathBuf, id: String },
    #[error("{path} has schema {found}; this build reads schema 1")]
    ReposSchema { path: PathBuf, found: u32 },
    #[error("{path} lists {name} twice")]
    ReposDuplicate { path: PathBuf, name: String },
}
