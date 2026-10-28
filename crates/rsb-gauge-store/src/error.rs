use std::path::PathBuf;

use rsb_gauge_reconcile::ReconcileError;

/// Why the store could not be opened, read or written.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("GAUGE_STORE is not set; it names the store file and nothing defaults it")]
    NoPath,
    #[error("{0} does not exist; create it with `gauge store init`")]
    Missing(PathBuf),
    #[error("{0} already exists")]
    Exists(PathBuf),
    #[error("cannot create {path}")]
    Create {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the store is locked past the busy timeout")]
    Busy(#[source] rusqlite::Error),
    #[error("the store fails its integrity check: {0}")]
    Corrupt(String),
    #[error("the store has schema {found}; this build knows up to {known}")]
    Newer { found: i64, known: i64 },
    #[error("the store has no {0} setting")]
    NoSetting(&'static str),
    #[error("{what} {value} is out of range")]
    Range { what: &'static str, value: String },
    #[error(transparent)]
    Value(#[from] ReconcileError),
    #[error("an event payload")]
    Payload(#[from] serde_json::Error),
    #[error("sqlite")]
    Sqlite(#[source] rusqlite::Error),
}

impl StoreError {
    /// Whether this is the store being unreachable: missing, locked past the
    /// timeout, or failing its integrity check. The tick acts on nothing.
    pub fn is_unreachable(&self) -> bool {
        matches!(self, Self::Missing(_) | Self::Busy(_) | Self::Corrupt(_))
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        use rusqlite::ErrorCode;
        match error.sqlite_error_code() {
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => Self::Busy(error),
            Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt) => {
                Self::Corrupt(error.to_string())
            }
            _ => Self::Sqlite(error),
        }
    }
}
