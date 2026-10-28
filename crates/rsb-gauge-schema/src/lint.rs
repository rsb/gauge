//! The registry linter: what a rule file can say that is still wrong. A
//! finding is an outcome, not an error; the caller prints them and exits 1.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::Registry;
use crate::rule::{Check, CheckClass, Lifecycle, State};

/// The problem a finding names, as it appears in the finding line. A file
/// named for the wrong id, and so a duplicate id, is refused by the load and
/// never reaches the linter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Problem {
    /// Two entries, rules or heuristics, share a statement.
    DuplicateStatement,
    /// `why` is empty.
    MissingWhy,
    /// A trial or enforced entry has no check and is not judgement.
    NoCheck,
    /// An enforced entry has no exemplar.
    NoExemplar,
    /// An entry past draft has no `lifecycle.since`.
    Unstamped,
    /// A retired entry has no reason.
    RetiredWithoutReason,
    /// A date is not `YYYY-MM-DD`.
    BadDate,
    /// A check's precision is outside 0 to 1.
    BadPrecision,
}

impl Problem {
    fn name(self) -> &'static str {
        match self {
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

/// What the linter reads, the same for a rule and a heuristic.
struct View<'a> {
    path: &'a Path,
    id: &'a str,
    statement: &'a str,
    why: &'a str,
    check_class: CheckClass,
    lifecycle: &'a Lifecycle,
    checks: &'a [Check],
    exemplars: &'a [String],
}

/// Every finding in the registry: rules in path order, then heuristics.
pub fn lint(registry: &Registry) -> Vec<Finding> {
    let rules = registry.rules.iter().map(|e| View {
        path: &e.path,
        id: &e.item.id,
        statement: &e.item.statement,
        why: &e.item.why,
        check_class: e.item.check_class,
        lifecycle: &e.item.lifecycle,
        checks: &e.item.checks,
        exemplars: &e.item.exemplars,
    });
    let heuristics = registry.heuristics.iter().map(|e| View {
        path: &e.path,
        id: &e.item.id,
        statement: &e.item.statement,
        why: &e.item.why,
        check_class: e.item.check_class,
        lifecycle: &e.item.lifecycle,
        checks: &e.item.checks,
        exemplars: &e.item.exemplars,
    });

    let mut out = Vec::new();
    let mut by_statement: HashMap<&str, &str> = HashMap::new();
    for entry in rules.chain(heuristics) {
        let finding = |problem, what: String| Finding {
            path: entry.path.to_path_buf(),
            problem,
            what,
        };

        if let Some(first) = by_statement.insert(entry.statement.trim(), entry.id) {
            out.push(finding(
                Problem::DuplicateStatement,
                format!("same statement as {first}"),
            ));
        }
        if entry.why.trim().is_empty() {
            out.push(finding(Problem::MissingWhy, "an entry says why".to_owned()));
        }

        let lifecycle = entry.lifecycle;
        let state = lifecycle.state;
        let needs_check = matches!(state, State::Trial | State::Enforced)
            && entry.check_class != CheckClass::Judgement;
        if needs_check && entry.checks.is_empty() {
            out.push(finding(
                Problem::NoCheck,
                "a trial or enforced entry names the check that holds it".to_owned(),
            ));
        }
        if state == State::Enforced && entry.exemplars.is_empty() {
            out.push(finding(
                Problem::NoExemplar,
                "an enforced entry points at a file that follows it".to_owned(),
            ));
        }
        if state != State::Draft && lifecycle.since.is_none() {
            out.push(finding(
                Problem::Unstamped,
                "an entry past draft records the date it entered its state".to_owned(),
            ));
        }
        if state == State::Retired && lifecycle.reason.is_none() {
            out.push(finding(
                Problem::RetiredWithoutReason,
                "a retired entry says why".to_owned(),
            ));
        }
        for (field, value) in [("since", &lifecycle.since), ("until", &lifecycle.until)] {
            if let Some(date) = value
                && !is_date(date)
            {
                out.push(finding(
                    Problem::BadDate,
                    format!("lifecycle.{field} {date:?} is not YYYY-MM-DD"),
                ));
            }
        }
        for check in entry.checks {
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
