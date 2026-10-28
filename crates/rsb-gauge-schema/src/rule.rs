//! The shape of one rule or heuristic file. These types are the schema:
//! `toml` reads a file into them, `schemars` derives the JSON Schema from
//! them, and an unknown field or value is a parse error, never a silent
//! default.
//!
//! A [`Rule`] must hold; not holding is a finding. A [`Heuristic`] is a
//! weighed default; not following it is a deviation. Everything speculative
//! goes in `tags`, which no check reads.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One engineering rule, as its TOML file states it.
///
/// Enforcement is derived from [`Consequence`] and [`Lifecycle`], never
/// stated: a trial rule only reports; an enforced business finding blocks;
/// an enforced coherence finding accrues debt and never blocks. This crate
/// holds no verdict logic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// `rl-NNNN`: opaque, never renumbered or reused. The file is
    /// `rules/<id>.toml`.
    pub id: String,
    /// One imperative, verifiable line.
    pub statement: String,
    /// The reason, as prose. Rendered for the agent on demand, not by default.
    pub why: String,
    /// Where the rule applies within the mount that selects it.
    pub scope: Scope,
    /// How the rule is held: exactly, approximately, or by a reviewer.
    pub check_class: CheckClass,
    /// What it costs when the rule does not hold. Set by the owner at
    /// registration, revisable through the lifecycle.
    pub consequence: Consequence,
    pub lifecycle: Lifecycle,
    /// The numbered rule in a repo's `engineering/` this rule restates.
    /// Transitional: retires when `gauge plan` renders and the rule text
    /// moves into the registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    /// The checks that hold the rule. A trial or enforced rule has at least
    /// one unless its class is judgement.
    #[serde(default)]
    pub checks: Vec<Check>,
    /// Files whose types the code must satisfy, relative to the mount.
    #[serde(default)]
    pub contracts: Vec<String>,
    /// Real files that show the rule followed, never copies.
    #[serde(default)]
    pub exemplars: Vec<String>,
    /// Names of the patterns this rule belongs to. Written by the lab.
    #[serde(default)]
    pub patterns: Vec<String>,
    /// Free descriptive strings. Never an enforcement input.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// One heuristic: a default with permission to deviate. It renders as
/// "prefer X; when the situation calls for otherwise, do otherwise and say
/// why in the hand-back". Not following it is a deviation, not a finding,
/// so it has no consequence class; a statement whose breach could reach
/// the user or the box is a rule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Heuristic {
    /// `hr-NNNN`: opaque, never renumbered or reused. The file is
    /// `heuristics/<id>.toml`.
    pub id: String,
    /// The default, as one line: what to prefer.
    pub statement: String,
    /// The reason, as prose.
    pub why: String,
    /// How a deviation is detected: exactly, approximately, or by a reviewer.
    pub check_class: CheckClass,
    pub lifecycle: Lifecycle,
    /// Where the heuristic applies within the mount that selects it.
    pub scope: Scope,
    /// The checks that detect a deviation.
    #[serde(default)]
    pub checks: Vec<Check>,
    /// Real files that show the default followed, never copies.
    #[serde(default)]
    pub exemplars: Vec<String>,
    /// Names of the patterns this heuristic belongs to. Written by the lab.
    #[serde(default)]
    pub patterns: Vec<String>,
    /// Free descriptive strings. Never an enforcement input.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Where a rule or heuristic applies. Which repos and app types it applies
/// to is decided by the profile nodes that select it, and the runtime by
/// the runtime leaf's platform; this narrows within a mount.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    /// Globs relative to the mount of the node that selects the rule.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Globs relative to the same mount, taken out of `paths`.
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// Exact fails on every breach and only on a breach. Approximate emits a
/// signal with known imprecision. Judgement is a reviewer grading against
/// the statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CheckClass {
    Exact,
    Approximate,
    Judgement,
}

impl CheckClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Approximate => "approximate",
            Self::Judgement => "judgement",
        }
    }
}

/// What a rule costs when it does not hold. Business: the user experiences
/// it. Coherence: the system is less like itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Consequence {
    Business,
    Coherence,
}

/// A pointer into a repo's `engineering/`: the file, relative to the repo
/// root, and the rule's number in it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub file: String,
    pub number: u32,
}

/// Draft is written, trial reports without consequence, enforced counts,
/// retired is kept so old findings and suppressions still resolve.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lifecycle {
    pub state: State,
    /// The date, `YYYY-MM-DD`, the entry entered its current state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// When retired, the date suppressions stop resolving. Absent means
    /// kept indefinitely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    /// When retired, the ids that took its place.
    #[serde(default)]
    pub replaced_by: Vec<String>,
    /// Earlier ids, so a stale suppression can be rewritten.
    #[serde(default)]
    pub renamed_from: Vec<String>,
    /// Why the entry entered its current state. Required when retired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Draft,
    Trial,
    Enforced,
    Retired,
}

/// One thing that holds a rule or detects a deviation from a heuristic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub kind: CheckKind,
    /// What the kind needs to find the check: a lint rule name, a file, a
    /// script, or the text a reviewer is given.
    #[serde(rename = "ref")]
    pub reference: String,
    /// For an approximate check, the measured precision as a fraction,
    /// once the lab has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<f64>,
}

/// The adapter a check runs through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CheckKind {
    Eslint,
    Clippy,
    AstGrep,
    DependencyCruiser,
    Xtask,
    Test,
    Script,
    Review,
}

impl Rule {
    /// The id, statement and class in one line, for a report.
    pub fn summary(&self) -> String {
        format!(
            "{} [{}] {}",
            self.id,
            self.check_class.as_str(),
            self.statement
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RL_0001: &str = r#"
id = "rl-0001"
statement = "Return a `Result<T, E>`. Never throw or reject."
why = "A thrown error escapes the type system and the log line, and an isolate serves many requests."
check_class = "exact"
consequence = "coherence"
contracts = ["packages/core/result/src/result.ts"]
exemplars = ["packages/business/user/src/invitation.ts"]
patterns = ["errors-as-values"]

[scope]
paths = ["src/**"]

[lifecycle]
state = "enforced"
since = "2026-10-13"

[[checks]]
kind = "eslint"
ref = "no-restricted-syntax: Return a Result. See rl-0001."
"#;

    const HR_0001: &str = r#"
id = "hr-0001"
statement = "Prefer one operation per file."
why = "A reader finds an operation by its name."
check_class = "approximate"

[scope]
paths = ["src/operations/**"]

[lifecycle]
state = "trial"
since = "2026-10-28"
"#;

    #[test]
    fn a_rule_file_reads_into_the_types() {
        let rule: Rule = toml::from_str(RL_0001).expect("rl-0001 parses");
        assert_eq!(rule.id, "rl-0001");
        assert_eq!(rule.check_class, CheckClass::Exact);
        assert_eq!(rule.consequence, Consequence::Coherence);
        assert_eq!(rule.scope.paths, ["src/**"]);
        assert_eq!(rule.source, None);
        assert_eq!(rule.lifecycle.state, State::Enforced);
        assert_eq!(rule.checks.first().map(|c| c.kind), Some(CheckKind::Eslint));
        assert_eq!(
            rule.summary(),
            "rl-0001 [exact] Return a `Result<T, E>`. Never throw or reject."
        );
    }

    #[test]
    fn a_rule_without_a_consequence_is_refused() {
        let text = RL_0001.replace("consequence = \"coherence\"\n", "");
        let err = toml::from_str::<Rule>(&text).expect_err("consequence is required");
        assert!(err.to_string().contains("consequence"), "{err}");
    }

    #[test]
    fn the_fields_v2_removed_are_refused() {
        // Before the first table header, so the key is the rule's, not a table's.
        for removed in ["default_level = \"error\"", "severity = \"high\""] {
            let text = RL_0001.replace(
                "check_class = \"exact\"",
                &format!("check_class = \"exact\"\n{removed}"),
            );
            assert!(toml::from_str::<Rule>(&text).is_err(), "{removed} parsed");
        }
        for removed in ["tree = \"rsb/web\"", "toolchain = \"typescript\""] {
            let text = RL_0001.replace("[scope]\n", &format!("[scope]\n{removed}\n"));
            assert!(toml::from_str::<Rule>(&text).is_err(), "{removed} parsed");
        }
    }

    #[test]
    fn the_old_check_class_names_are_refused() {
        for old in ["mechanical", "heuristic"] {
            let text = RL_0001.replace("\"exact\"", &format!("\"{old}\""));
            let err = toml::from_str::<Rule>(&text).expect_err("an old class name");
            assert!(err.to_string().contains(old), "{err}");
        }
    }

    #[test]
    fn a_heuristic_file_reads_into_the_types() {
        let heuristic: Heuristic = toml::from_str(HR_0001).expect("hr-0001 parses");
        assert_eq!(heuristic.check_class, CheckClass::Approximate);
        assert_eq!(heuristic.lifecycle.state, State::Trial);
        assert!(heuristic.checks.is_empty());
    }

    #[test]
    fn a_heuristic_with_a_consequence_is_refused() {
        let text = HR_0001.replace(
            "check_class = \"approximate\"",
            "check_class = \"approximate\"\nconsequence = \"business\"",
        );
        let err = toml::from_str::<Heuristic>(&text).expect_err("a heuristic has no consequence");
        assert!(err.to_string().contains("consequence"), "{err}");
    }

    #[test]
    fn each_schema_names_its_own_fields() {
        let rule = serde_json::to_string(&schemars::schema_for!(Rule)).expect("serializes");
        for word in [
            "exact",
            "approximate",
            "judgement",
            "business",
            "coherence",
            "source",
            "retired",
        ] {
            assert!(rule.contains(word), "rule schema lacks {word}");
        }
        let heuristic = schemars::schema_for!(Heuristic);
        let text = serde_json::to_string(&heuristic).expect("serializes");
        assert!(
            text.contains("approximate"),
            "heuristic schema lacks a class"
        );
        let fields = heuristic
            .get("properties")
            .and_then(|p| p.as_object())
            .expect("the heuristic schema lists its fields");
        assert!(fields.contains_key("check_class"));
        assert!(
            !fields.contains_key("consequence"),
            "heuristic schema has a consequence"
        );
    }
}
