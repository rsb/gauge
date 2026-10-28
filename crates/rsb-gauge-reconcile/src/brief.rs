//! Brief v1 (`agent-registry/schemas/brief/v1`): what the driver tells an
//! agent for one attempt, built from observed state and checked against the
//! schema before it is recorded or written. A brief that fails is never
//! written; the loop parks instead.

use serde::Serialize;
use serde_json::{Map, Value};

/// The brief's `schema`.
pub const SCHEMA: &str = "brief/v1";

/// Where the outbox is, as the agent sees it inside agent-box.
pub const OUTBOX: &str = "/run/outbox";

/// The kinds an outbox may hold, closed (effects record §7).
pub const KINDS: &[&str] = &[
    "pr-open",
    "issue-comment",
    "criteria-tick",
    "deviation",
    "review-findings",
    "flagged-for-human",
    "blocked",
];

/// What an attempt's agent is for. Its effects are computed here, in code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Coder,
}

impl Role {
    /// The outbox kinds the role may write (effects record §7).
    pub fn allowed_effects(self) -> &'static [&'static str] {
        match self {
            Self::Coder => &[
                "pr-open",
                "issue-comment",
                "criteria-tick",
                "deviation",
                "flagged-for-human",
                "blocked",
            ],
        }
    }
}

/// The issue numbers for `Refs #n` and `Closes #n`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Issues {
    pub refs: Vec<u64>,
    pub closes: Vec<u64>,
}

/// A fresh brief, as the driver builds it. Resume briefs are ticket 07.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Brief {
    pub schema: &'static str,
    pub repo: String,
    pub base_sha: String,
    pub base_branch: String,
    pub branch: String,
    pub ticket_path: String,
    pub spec: String,
    pub issues: Issues,
    pub attempt: u32,
    pub kind: &'static str,
    pub outbox_path: &'static str,
    pub runtime_bound: u64,
    pub allowed_effects: Vec<&'static str>,
}

/// What a fresh brief is built from: the loop's ticket and branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreshBrief<'a> {
    pub repo: &'a str,
    pub issue: u64,
    pub base_sha: &'a str,
    pub base_branch: &'a str,
    pub branch: &'a str,
    pub ticket_path: &'a str,
    pub spec: &'a str,
    pub attempt: u32,
    pub runtime_bound: u64,
}

impl Brief {
    /// The fresh coder brief. The ticket's issue is both `refs` and
    /// `closes` (record 1 §3: the brief carries them, ticket files none).
    pub fn fresh(input: &FreshBrief<'_>) -> Self {
        Self {
            schema: SCHEMA,
            repo: input.repo.to_owned(),
            base_sha: input.base_sha.to_owned(),
            base_branch: input.base_branch.to_owned(),
            branch: input.branch.to_owned(),
            ticket_path: input.ticket_path.to_owned(),
            spec: input.spec.to_owned(),
            issues: Issues {
                refs: vec![input.issue],
                closes: vec![input.issue],
            },
            attempt: input.attempt,
            kind: "fresh",
            outbox_path: OUTBOX,
            runtime_bound: input.runtime_bound,
            allowed_effects: Role::Coder.allowed_effects().to_vec(),
        }
    }

    /// The brief as JSON, checked against brief v1: only a brief that
    /// passes is returned.
    pub fn to_checked(&self) -> Result<Value, Vec<String>> {
        let value = serde_json::to_value(self).map_err(|e| vec![e.to_string()])?;
        validate(&value)?;
        Ok(value)
    }
}

/// The branch a ticket's loop runs on, `<type>/<issue>-<slug>`, as
/// `rsb-skills`' `scripts/brief` derives it: the type from the ticket's
/// `slice`, the slug from its file name. `None` for a slice no branch is
/// cut for (`human`) or a path that is not a ticket file's.
pub fn branch(slice: &str, issue: u64, ticket_path: &str) -> Option<String> {
    let kind = match slice {
        "feature" => "feat",
        "technical" => "refactor",
        "groundwork" => "chore",
        _ => return None,
    };
    let file = ticket_path.rsplit('/').next()?.strip_suffix(".md")?;
    let (_, slug) = file.split_once('-')?;
    Some(format!("{kind}/{issue}-{slug}"))
}

/// The ticket's `slice`, read from its front matter.
pub fn slice(ticket: &str) -> Option<&str> {
    let rest = ticket.strip_prefix("---\n")?;
    let (front, _) = rest.split_once("\n---")?;
    front
        .lines()
        .find_map(|line| line.strip_prefix("slice:"))
        .map(|v| v.trim().trim_matches('"'))
}

/// Checks `brief` against brief v1 and the credential rule `check.py` adds:
/// no string, key or value, may look like a credential. Every problem is
/// returned, each as `<path>: <what>`.
pub fn validate(brief: &Value) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    match brief.as_object() {
        Some(object) => check_brief(object, &mut errors),
        None => errors.push("(root): not an object".to_owned()),
    }
    if strings(brief).iter().any(|s| credential(s)) {
        errors.push("(root): holds something that looks like a credential".to_owned());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

const REQUIRED: &[&str] = &[
    "schema",
    "repo",
    "base_sha",
    "base_branch",
    "branch",
    "ticket_path",
    "spec",
    "issues",
    "attempt",
    "kind",
    "outbox_path",
    "runtime_bound",
    "allowed_effects",
];

/// A text field and the pattern it must match.
type Check = (&'static str, fn(&str) -> bool);

const CAUSES: &[&str] = &[
    "exited_nonzero",
    "timed_out",
    "no_outbox",
    "intent_lost",
    "work_lost",
];

fn check_brief(brief: &Map<String, Value>, errors: &mut Vec<String>) {
    let mut fail = |path: &str, what: &str| errors.push(format!("{path}: {what}"));
    for key in brief.keys() {
        if !REQUIRED.contains(&key.as_str()) && key != "resume" {
            fail(key, "not a brief v1 field");
        }
    }
    for key in REQUIRED {
        if !brief.contains_key(*key) {
            fail(key, "missing");
        }
    }
    let text = |key: &str| brief.get(key).and_then(Value::as_str);
    let checks: [Check; 8] = [
        ("schema", |s| s == SCHEMA),
        ("repo", |s| !s.is_empty() && s.bytes().all(name_byte)),
        ("base_sha", is_sha),
        ("base_branch", is_branch),
        ("branch", is_branch),
        ("ticket_path", is_ticket_path),
        ("spec", is_spec),
        ("outbox_path", |s| s.starts_with('/')),
    ];
    for (key, test) in checks {
        if brief.contains_key(key) && !text(key).is_some_and(test) {
            fail(key, "does not match the schema");
        }
    }
    for key in ["attempt", "runtime_bound"] {
        if brief.contains_key(key)
            && !brief
                .get(key)
                .and_then(Value::as_u64)
                .is_some_and(|n| n >= 1)
        {
            fail(key, "not an integer of at least 1");
        }
    }
    if let Some(issues) = brief.get("issues") {
        match issues.as_object() {
            Some(issues) => {
                if issues.keys().any(|k| k != "refs" && k != "closes") {
                    fail("issues", "holds a field other than refs and closes");
                }
                for key in ["refs", "closes"] {
                    if !issues.get(key).is_some_and(issue_numbers) {
                        fail(
                            &format!("issues/{key}"),
                            "not a non-empty set of issue numbers",
                        );
                    }
                }
            }
            None => fail("issues", "not an object"),
        }
    }
    let kind = text("kind");
    if brief.contains_key("kind") && !matches!(kind, Some("fresh" | "resume")) {
        fail("kind", "not fresh or resume");
    }
    match (kind, brief.get("resume")) {
        (Some("resume"), None) => fail("resume", "missing on a resume brief"),
        (Some("resume"), Some(resume)) => check_resume(resume, &mut fail),
        (_, Some(_)) => fail("resume", "present on a brief that is not a resume"),
        (_, None) => {}
    }
    if let Some(effects) = brief.get("allowed_effects") {
        let ok = effects.as_array().is_some_and(|items| {
            !items.is_empty()
                && unique(items)
                && items
                    .iter()
                    .all(|i| i.as_str().is_some_and(|s| KINDS.contains(&s)))
        });
        if !ok {
            fail("allowed_effects", "not a non-empty set of outbox kinds");
        }
    }
}

fn check_resume(resume: &Value, fail: &mut impl FnMut(&str, &str)) {
    let Some(resume) = resume.as_object() else {
        return fail("resume", "not an object");
    };
    for key in resume.keys() {
        if !["cause", "from_head", "last_verdict"].contains(&key.as_str()) {
            fail(&format!("resume/{key}"), "not a resume field");
        }
    }
    match resume.get("cause") {
        Some(Value::Null) => {
            if !resume.get("last_verdict").is_some_and(Value::is_object) {
                fail(
                    "resume/last_verdict",
                    "must be an object when cause is null",
                );
            }
        }
        Some(Value::String(cause)) if CAUSES.contains(&cause.as_str()) => {}
        Some(_) => fail("resume/cause", "not an attempt cause a resume carries"),
        None => fail("resume/cause", "missing"),
    }
    if !resume
        .get("from_head")
        .and_then(Value::as_str)
        .is_some_and(is_sha)
    {
        fail("resume/from_head", "not a commit id");
    }
    match resume.get("last_verdict") {
        Some(Value::Null | Value::Object(_)) => {}
        Some(_) => fail("resume/last_verdict", "not an object or null"),
        None => fail("resume/last_verdict", "missing"),
    }
}

fn issue_numbers(value: &Value) -> bool {
    value.as_array().is_some_and(|items| {
        !items.is_empty()
            && unique(items)
            && items.iter().all(|i| i.as_u64().is_some_and(|n| n >= 1))
    })
}

fn unique(items: &[Value]) -> bool {
    items
        .iter()
        .enumerate()
        .all(|(i, a)| items.iter().skip(i + 1).all(|b| a != b))
}

/// `[A-Za-z0-9._-]`.
fn name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')
}

/// `^[0-9a-f]{40}([0-9a-f]{24})?$`.
fn is_sha(s: &str) -> bool {
    matches!(s.len(), 40 | 64)
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `^[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)*$`.
fn is_branch(s: &str) -> bool {
    s.split('/')
        .all(|part| !part.is_empty() && part.bytes().all(name_byte))
}

/// `^S-[0-9]{4}$`.
fn is_spec(s: &str) -> bool {
    s.strip_prefix("S-")
        .is_some_and(|n| n.len() == 4 && n.bytes().all(|b| b.is_ascii_digit()))
}

/// `[a-z0-9]+(-[a-z0-9]+)*`.
fn is_slug(s: &str) -> bool {
    s.split('-').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    })
}

/// `^specs/S-[0-9]{4}-<slug>/[0-9]{2}-<slug>\.md$`.
fn is_ticket_path(s: &str) -> bool {
    let mut parts = s.split('/');
    let (Some("specs"), Some(folder), Some(file), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let folder_ok = folder.get(..6).is_some_and(is_spec)
        && folder.get(6..7) == Some("-")
        && folder.get(7..).is_some_and(is_slug);
    let file_ok = file.strip_suffix(".md").is_some_and(|stem| {
        stem.get(..2)
            .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
            && stem.get(2..3) == Some("-")
            && stem.get(3..).is_some_and(is_slug)
    });
    folder_ok && file_ok
}

/// Every string in `value`, object keys included.
fn strings(value: &Value) -> Vec<&str> {
    match value {
        Value::String(s) => vec![s.as_str()],
        Value::Array(items) => items.iter().flat_map(strings).collect(),
        Value::Object(map) => map
            .iter()
            .flat_map(|(k, v)| std::iter::once(k.as_str()).chain(strings(v)))
            .collect(),
        _ => Vec::new(),
    }
}

/// `check.py`'s `CREDENTIAL`: `gh[pousr]_[A-Za-z0-9]{20,}`, `github_pat_`,
/// `-----BEGIN [A-Z ]*PRIVATE KEY`, `sk-[A-Za-z0-9-]{20,}`, `xox[abpr]-`.
pub fn credential(s: &str) -> bool {
    let run = |rest: &str, ok: fn(u8) -> bool| rest.bytes().take_while(|&b| ok(b)).count();
    if s.contains("github_pat_") {
        return true;
    }
    (0..s.len()).any(|i| {
        let Some(rest) = s.get(i..) else {
            return false;
        };
        let b = rest.as_bytes();
        if let [b'g', b'h', c, b'_', ..] = b
            && b"pousr".contains(c)
            && run(rest.get(4..).unwrap_or_default(), |b| {
                b.is_ascii_alphanumeric()
            }) >= 20
        {
            return true;
        }
        if let Some(tail) = rest.strip_prefix("sk-")
            && run(tail, |b| b.is_ascii_alphanumeric() || b == b'-') >= 20
        {
            return true;
        }
        if let [b'x', b'o', b'x', c, b'-', ..] = b
            && b"abpr".contains(c)
        {
            return true;
        }
        if let Some(tail) = rest.strip_prefix("-----BEGIN ") {
            let n = run(tail, |b| b.is_ascii_uppercase() || b == b' ');
            // `[A-Z ]*` is greedy but may give back: any split point works.
            return (0..=n).any(|k| tail.get(k..).is_some_and(|t| t.starts_with("PRIVATE KEY")));
        }
        false
    })
}
