//! The shape of one rule file. These types are the schema: `toml` reads a
//! file into them, `schemars` derives the JSON Schema from them, and an
//! unknown field or value is a parse error, never a silent default.
//!
//! Three closed enums decide enforcement: [`CheckClass`], [`Level`] and
//! [`State`]. Everything speculative goes in `tags`, which no check reads.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One engineering rule, as its TOML file states it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// Opaque, never renumbered or reused. The file is named `<id>.toml`.
    pub id: String,
    /// One imperative, verifiable line.
    pub statement: String,
    /// The reason, as prose. Rendered for the agent on demand, not by default.
    pub why: String,
    /// Where the rule applies. Resolved when a projection is rendered.
    pub scope: Scope,
    /// How the rule is held: by a build failure, a signal, or a reviewer.
    pub check_class: CheckClass,
    /// The level a project gets unless its profile overrides it.
    pub default_level: Level,
    pub lifecycle: Lifecycle,
    /// The checks that hold the rule. An enforced rule has at least one
    /// unless its class is judgement.
    #[serde(default)]
    pub checks: Vec<Check>,
    /// Files whose types the code must satisfy, relative to the repo root.
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

/// Where a rule applies. The scope tree is rsb, application kind, variant,
/// repo; the toolchain decides which check adapters exist; the runtime
/// target decides which rules are selected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    /// A node in the scope tree, such as `rsb`, `rsb/web`,
    /// `rsb/web/cloudflare` or `rsb/web/cloudflare/gm-serverless`.
    pub tree: String,
    /// Absent means every toolchain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolchain: Option<Toolchain>,
    /// Absent means every runtime target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<Runtime>,
    /// Globs relative to the repo root.
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Toolchain {
    Typescript,
    Rust,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Runtime {
    Cloudflare,
    K8s,
    LinuxDesktop,
}

/// Mechanical fails the build. Heuristic emits a signal with known
/// imprecision. Judgement is a reviewer grading against the rule text and
/// never gates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CheckClass {
    Mechanical,
    Heuristic,
    Judgement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Level {
    Off,
    Info,
    Warn,
    Error,
}

/// Draft is written, trial reports without gating, enforced gates at its
/// level, retired is kept so old findings and suppressions still resolve.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lifecycle {
    pub state: State,
    /// The date, `YYYY-MM-DD`, the rule entered its current state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// For a retired rule, the date suppressions stop resolving. Absent
    /// means kept indefinitely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    /// For a retired rule, the ids that took its place.
    #[serde(default)]
    pub replaced_by: Vec<String>,
    /// Earlier ids of this rule, so a stale suppression can be rewritten.
    #[serde(default)]
    pub renamed_from: Vec<String>,
    /// Why the rule entered its current state. Required when retired.
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

/// One thing that holds a rule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub kind: CheckKind,
    /// What the kind needs to find the check: a lint rule name, a file, a
    /// script, or the text a reviewer is given.
    #[serde(rename = "ref")]
    pub reference: String,
    /// For a heuristic, the measured precision as a fraction, once the lab
    /// has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<f64>,
}

/// The adapter a check runs through. The toolchain decides which exist.
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
        let class = match self.check_class {
            CheckClass::Mechanical => "mechanical",
            CheckClass::Heuristic => "heuristic",
            CheckClass::Judgement => "judgement",
        };
        format!("{} [{class}] {}", self.id, self.statement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ERR_1: &str = r#"
id = "ERR-1"
statement = "Return a `Result<T, E>`. Never throw or reject."
why = "A thrown error escapes the type system and the log line, and an isolate serves many requests."
check_class = "mechanical"
default_level = "error"
contracts = ["packages/core/result/src/result.ts"]
exemplars = ["packages/business/user/src/invitation.ts"]
patterns = ["errors-as-values"]

[scope]
tree = "rsb/web"
toolchain = "typescript"
paths = ["packages/**/src/**", "apps/**/src/**"]

[lifecycle]
state = "enforced"
since = "2026-10-13"

[[checks]]
kind = "eslint"
ref = "no-restricted-syntax: Return a Result. See ERR-1."
"#;

    #[test]
    fn a_rule_file_reads_into_the_types() {
        let rule: Rule = toml::from_str(ERR_1).expect("ERR-1 parses");
        assert_eq!(rule.id, "ERR-1");
        assert_eq!(rule.check_class, CheckClass::Mechanical);
        assert_eq!(rule.scope.toolchain, Some(Toolchain::Typescript));
        assert_eq!(rule.scope.runtime, None);
        assert_eq!(rule.lifecycle.state, State::Enforced);
        assert_eq!(rule.checks.len(), 1);
        assert_eq!(rule.checks.first().map(|c| c.kind), Some(CheckKind::Eslint));
        assert_eq!(rule.patterns, ["errors-as-values"]);
        assert_eq!(
            rule.summary(),
            "ERR-1 [mechanical] Return a `Result<T, E>`. Never throw or reject."
        );
    }

    #[test]
    fn an_unknown_field_is_refused() {
        // Before the first table header, so the key is the rule's, not a check's.
        let text = ERR_1.replace(
            "check_class = \"mechanical\"",
            "check_class = \"mechanical\"\nseverity = \"high\"",
        );
        let err = toml::from_str::<Rule>(&text).expect_err("severity is not a field");
        assert!(err.to_string().contains("severity"), "{err}");
    }

    #[test]
    fn an_unknown_enum_value_is_refused() {
        let text = ERR_1.replace("\"mechanical\"", "\"strict\"");
        let err = toml::from_str::<Rule>(&text).expect_err("strict is not a class");
        assert!(err.to_string().contains("strict"), "{err}");
    }

    #[test]
    fn the_schema_names_the_three_enforcement_enums() {
        let schema = schemars::schema_for!(Rule);
        let text = serde_json::to_string(&schema).expect("the schema serializes");
        for word in [
            "mechanical",
            "heuristic",
            "judgement",
            "enforced",
            "retired",
            "warn",
        ] {
            assert!(text.contains(word), "schema lacks {word}");
        }
    }
}
