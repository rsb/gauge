//! The audit log: one JSON line per request and decision, appended to
//! `~/.local/share/rsb/agent-driver/audit.jsonl`. It holds no body text and
//! no token.

use std::fmt;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::repo::RepoName;

/// Length of a [`RequestId`].
const ID_LEN: usize = 32;

/// A request id: 32 lowercase hex digits, so an id can never be a path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RequestId(String);

impl Serialize for RequestId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for RequestId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let hex = |c: char| c.is_ascii_digit() || ('a'..='f').contains(&c);
        if s.len() == ID_LEN && s.chars().all(hex) {
            Ok(Self(s.to_owned()))
        } else {
            Err(Error::RequestId(s.to_owned()))
        }
    }
}

/// One line. `repo`, `op` and `number` are absent for a request the driver
/// could not parse; `rule` is present only on a refusal.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Entry<'a> {
    pub time: String,
    pub id: &'a RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<&'a RepoName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    pub outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct Audit {
    path: PathBuf,
}

impl Audit {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one line. Opened with `O_APPEND` each time, created mode 600.
    pub fn record(&self, entry: &Entry<'_>) -> Result<(), Error> {
        let write = |source| Error::Write {
            path: self.path.clone(),
            source,
        };
        let mut line = serde_json::to_vec(entry).map_err(|e| write(e.into()))?;
        line.push(b'\n');
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&self.path)
            .map_err(write)?;
        file.write_all(&line).map_err(write)
    }
}

/// `at` as RFC 3339 in UTC, to the second. A clock before 1970 reads as
/// the epoch.
pub fn rfc3339(at: SystemTime) -> String {
    let secs = at.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Civil date from days since 1970-01-01, after Howard Hinnant's
    // `civil_from_days`, for non-negative days.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn times_are_rfc3339_utc_to_the_second() {
        let at = |secs| rfc3339(UNIX_EPOCH + Duration::from_secs(secs));
        assert_eq!(at(0), "1970-01-01T00:00:00Z");
        assert_eq!(at(951_782_399), "2000-02-28T23:59:59Z");
        assert_eq!(at(951_868_799), "2000-02-29T23:59:59Z");
        assert_eq!(
            rfc3339(UNIX_EPOCH + Duration::from_millis(1_792_252_800_999)),
            "2026-10-17T16:00:00Z"
        );
    }

    #[test]
    fn each_record_appends_one_line_with_no_body() {
        let dir = std::env::temp_dir().join(format!("agent-driver-audit-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        let audit = Audit::at(dir.join("audit.jsonl"));
        let id: RequestId = "0123456789abcdef0123456789abcdef"
            .parse()
            .unwrap_or_else(|e| panic!("{e}"));
        let repo: RepoName = "rsb/lumen".parse().unwrap_or_else(|e| panic!("{e}"));
        let refused = Entry {
            time: "2026-10-17T16:00:00Z".to_owned(),
            id: &id,
            repo: Some(&repo),
            op: Some("issue-close"),
            number: Some(3),
            outcome: "refused",
            rule: Some("refused-always"),
        };
        let unparsed = Entry {
            time: "2026-10-17T16:00:01Z".to_owned(),
            id: &id,
            repo: None,
            op: None,
            number: None,
            outcome: "error",
            rule: None,
        };
        audit.record(&refused).unwrap_or_else(|e| panic!("{e}"));
        audit.record(&unparsed).unwrap_or_else(|e| panic!("{e}"));
        let text = std::fs::read_to_string(audit.path()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            text,
            "{\"time\":\"2026-10-17T16:00:00Z\",\"id\":\"0123456789abcdef0123456789abcdef\",\
             \"repo\":\"rsb/lumen\",\"op\":\"issue-close\",\"number\":3,\"outcome\":\"refused\",\
             \"rule\":\"refused-always\"}\n\
             {\"time\":\"2026-10-17T16:00:01Z\",\"id\":\"0123456789abcdef0123456789abcdef\",\
             \"outcome\":\"error\"}\n"
        );
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(audit.path())
            .unwrap_or_else(|e| panic!("{e}"))
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        std::fs::remove_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    }
}
