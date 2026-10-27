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
}
