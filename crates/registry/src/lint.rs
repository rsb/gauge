//! The registry linter: what a rule file can say that is still wrong. A
//! finding is an outcome, not an error; the caller prints them and exits 1.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use crate::rule::{CheckClass, State};
use crate::{Entry, Registry};

/// The problem a finding names, as it appears in the finding line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Problem {
    /// The file's stem is not the rule's id.
    FileName,
    /// Two rules share an id.
    DuplicateId,
    /// Two rules share a statement.
    DuplicateStatement,
    /// `why` is empty.
    MissingWhy,
    /// An enforced or trial rule has no check and is not judgement.
    NoCheck,
    /// An enforced rule has no exemplar.
    NoExemplar,
    /// A rule past draft has no `lifecycle.since`.
    Unstamped,
    /// A retired rule has no reason.
    RetiredWithoutReason,
    /// A date is not `YYYY-MM-DD`.
    BadDate,
    /// A check's precision is outside 0 to 1.
    BadPrecision,
}

impl Problem {
    fn name(self) -> &'static str {
        match self {
            Problem::FileName => "file-name",
            Problem::DuplicateId => "duplicate-id",
            Problem::DuplicateStatement => "duplicate-statement",
            Problem::MissingWhy => "missing-why",
            Problem::NoCheck => "no-check",
            Problem::NoExemplar => "no-exemplar",
            Problem::Unstamped => "unstamped",
            Problem::RetiredWithoutReason => "retired-without-reason",
            Problem::BadDate => "bad-date",
            Problem::BadPrecision => "bad-precision",
        }
    }
}

/// One line a person reads: `path: problem: what`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub path: PathBuf,
    pub problem: Problem,
    pub what: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}: {}",
            self.path.display(),
            self.problem.name(),
            self.what
        )
    }
}

/// Every finding in the registry, in path order.
pub fn lint(registry: &Registry) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut by_id: HashMap<&str, &Entry> = HashMap::new();
    let mut by_statement: HashMap<&str, &Entry> = HashMap::new();

    for entry in &registry.entries {
        let rule = &entry.rule;
        let finding = |problem, what: String| Finding {
            path: entry.path.clone(),
            problem,
            what,
        };

        let stem = entry
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if stem != rule.id {
            out.push(finding(
                Problem::FileName,
                format!("file is named {stem:?} but the rule is {:?}", rule.id),
            ));
        }
        if let Some(first) = by_id.insert(&rule.id, entry) {
            out.push(finding(
                Problem::DuplicateId,
                format!("{} is also {}", rule.id, first.path.display()),
            ));
        }
        if let Some(first) = by_statement.insert(rule.statement.trim(), entry) {
            out.push(finding(
                Problem::DuplicateStatement,
                format!("same statement as {}", first.rule.id),
            ));
        }
        if rule.why.trim().is_empty() {
            out.push(finding(Problem::MissingWhy, "a rule says why".to_owned()));
        }

        let state = rule.lifecycle.state;
        let needs_check = matches!(state, State::Trial | State::Enforced)
            && rule.check_class != CheckClass::Judgement;
        if needs_check && rule.checks.is_empty() {
            out.push(finding(
                Problem::NoCheck,
                "a trial or enforced rule names the check that holds it".to_owned(),
            ));
        }
        if state == State::Enforced && rule.exemplars.is_empty() {
            out.push(finding(
                Problem::NoExemplar,
                "an enforced rule points at a file that follows it".to_owned(),
            ));
        }
        if state != State::Draft && rule.lifecycle.since.is_none() {
            out.push(finding(
                Problem::Unstamped,
                "a rule past draft records the date it entered its state".to_owned(),
            ));
        }
        if state == State::Retired && rule.lifecycle.reason.is_none() {
            out.push(finding(
                Problem::RetiredWithoutReason,
                "a retired rule says why".to_owned(),
            ));
        }
        for (field, value) in [
            ("since", &rule.lifecycle.since),
            ("until", &rule.lifecycle.until),
        ] {
            if let Some(date) = value
                && !is_date(date)
            {
                out.push(finding(
                    Problem::BadDate,
                    format!("lifecycle.{field} {date:?} is not YYYY-MM-DD"),
                ));
            }
        }
        for check in &rule.checks {
            if let Some(p) = check.precision
                && !(0.0..=1.0).contains(&p)
            {
                out.push(finding(
                    Problem::BadPrecision,
                    format!("{} precision {p} is not between 0 and 1", check.reference),
                ));
            }
        }
    }
    out
}

/// `YYYY-MM-DD` by shape: four digits, dash, two, dash, two. The calendar
/// is not checked; a wrong day is caught by a person.
fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                *b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_has_the_shape() {
        assert!(is_date("2026-10-19"), "a plain date");
        assert!(!is_date("2026-1-19"), "two digits per part");
        assert!(!is_date("19-10-2026"), "year first");
        assert!(!is_date(""), "empty");
    }
}
