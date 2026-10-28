use std::path::{Path, PathBuf};

use rsb_gauge_schema::rule::{CheckClass, Consequence, Source, State};
use rsb_gauge_schema::{Problem, Registry, RegistryError, lint};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A registry root in the temp dir with `files` written under it, each a
/// path relative to the root and its text. Removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str, files: &[(&str, &str)]) -> std::io::Result<Self> {
        let root =
            std::env::temp_dir().join(format!("gauge-registry-{name}-{}", std::process::id()));
        std::fs::create_dir_all(root.join("rules"))?;
        for (rel, text) in files {
            let path = root.join(rel);
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(path, text)?;
        }
        Ok(Self(root))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0) {
            eprintln!("cannot remove {}: {e}", self.0.display());
        }
    }
}

const HEURISTIC: &str = r#"
id = "hr-0001"
statement = "Prefer one operation per file."
why = "A reader finds an operation by its name."
check_class = "judgement"

[scope]
paths = ["src/**"]

[lifecycle]
state = "draft"
"#;

#[test]
fn a_clean_registry_loads_every_rule_and_heuristic() {
    let registry = Registry::load(&fixture("clean")).expect("the clean fixture loads");
    let ids: Vec<&str> = registry.rules.iter().map(|e| e.item.id.as_str()).collect();
    assert_eq!(ids, ["rl-0001", "rl-0002", "rl-0003"]);

    let rl_0002 = &registry.rule("rl-0002").expect("rl-0002 is loaded").item;
    assert_eq!(rl_0002.check_class, CheckClass::Judgement);
    assert_eq!(rl_0002.consequence, Consequence::Business);

    let hr_0001 = &registry
        .heuristic("hr-0001")
        .expect("hr-0001 is loaded")
        .item;
    assert_eq!(hr_0001.check_class, CheckClass::Approximate);
    assert_eq!(hr_0001.lifecycle.state, State::Trial);
    assert_eq!(hr_0001.checks.first().and_then(|c| c.precision), Some(0.8));
    assert_eq!(registry.heuristics.len(), 1);

    assert_eq!(
        lint(&registry),
        Vec::new(),
        "a clean registry has no findings"
    );
}

#[test]
fn a_pointer_rule_carries_its_source() {
    let registry = Registry::load(&fixture("clean")).expect("the clean fixture loads");
    let pointer = &registry.rule("rl-0003").expect("rl-0003 is loaded").item;
    assert_eq!(
        pointer.source,
        Some(Source {
            file: "engineering/migrations.md".to_owned(),
            number: 13,
        })
    );
    assert_eq!(pointer.consequence, Consequence::Coherence);
    let plain = &registry.rule("rl-0001").expect("rl-0001 is loaded").item;
    assert_eq!(plain.source, None, "a rule with its own text has no source");
}

#[test]
fn a_dirty_registry_reports_each_problem_with_its_file() {
    let registry = Registry::load(&fixture("dirty")).expect("the dirty fixture parses");
    let findings = lint(&registry);
    let problems: Vec<(String, Problem)> = findings
        .iter()
        .map(|f| {
            let file = f.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            (file.to_owned(), f.problem)
        })
        .collect();
    assert_eq!(
        problems,
        [
            ("rl-0002.toml".to_owned(), Problem::MissingWhy),
            ("rl-0002.toml".to_owned(), Problem::NoExemplar),
            ("rl-0002.toml".to_owned(), Problem::Unstamped),
            ("rl-0002.toml".to_owned(), Problem::BadPrecision),
            ("rl-0003.toml".to_owned(), Problem::RetiredWithoutReason),
            ("rl-0003.toml".to_owned(), Problem::BadDate),
            ("hr-0001.toml".to_owned(), Problem::DuplicateStatement),
            ("hr-0001.toml".to_owned(), Problem::NoCheck),
        ]
    );
    let line = findings.last().map(ToString::to_string).unwrap_or_default();
    assert!(
        line.ends_with(
            "hr-0001.toml: no-check: a trial or enforced entry names the check that holds it"
        ),
        "{line}"
    );
}

#[test]
fn a_heuristic_with_a_consequence_is_refused() {
    let text = HEURISTIC.replace(
        "check_class = \"judgement\"",
        "check_class = \"judgement\"\nconsequence = \"coherence\"",
    );
    let scratch = Scratch::new("consequence", &[("heuristics/hr-0001.toml", &text)])
        .expect("scratch registry");
    let err = Registry::load(&scratch.0).expect_err("a heuristic has no consequence");
    assert!(
        matches!(err, RegistryError::Parse { ref path, ref source }
            if path.ends_with("hr-0001.toml") && source.to_string().contains("consequence")),
        "{err:?}"
    );
}

#[test]
fn a_file_without_its_kinds_prefix_is_refused() {
    // A heuristic's id under rules/, then an old topic-style id.
    for (name, file) in [
        ("prefix", "rules/hr-0001.toml"),
        ("old", "rules/ERR-1.toml"),
    ] {
        let scratch = Scratch::new(name, &[(file, HEURISTIC)]).expect("scratch registry");
        let err = Registry::load(&scratch.0).expect_err("the wrong prefix");
        assert!(
            matches!(err, RegistryError::FileName { prefix: "rl", .. }),
            "{file}: {err:?}"
        );
        assert!(
            err.to_string().ends_with("is not named rl-NNNN.toml"),
            "{err}"
        );
    }
}

#[test]
fn a_file_holding_another_id_is_refused() {
    let scratch = Scratch::new("mismatch", &[("heuristics/hr-0002.toml", HEURISTIC)])
        .expect("scratch registry");
    let err = Registry::load(&scratch.0).expect_err("the file is named for another id");
    assert!(
        matches!(err, RegistryError::IdMismatch { ref path, ref id }
            if path.ends_with("hr-0002.toml") && id == "hr-0001"),
        "{err:?}"
    );
}

#[test]
fn a_topic_directory_is_refused() {
    let scratch =
        Scratch::new("nested", &[("rules/errors/rl-0001.toml", "")]).expect("scratch registry");
    let err = Registry::load(&scratch.0).expect_err("rules/ is flat");
    assert!(
        matches!(err, RegistryError::Nested(ref path) if path.ends_with("rules/errors")),
        "{err:?}"
    );
}

#[test]
fn a_file_that_does_not_parse_stops_the_load_and_names_the_file() {
    let scratch = Scratch::new(
        "parse",
        &[(
            "rules/rl-0001.toml",
            "id = \"rl-0001\"\nseverity = \"high\"\n",
        )],
    )
    .expect("scratch registry");
    let err = Registry::load(&scratch.0).expect_err("an unknown field refuses the file");
    assert!(
        matches!(err, RegistryError::Parse { ref path, .. } if path.ends_with("rl-0001.toml")),
        "{err:?}"
    );
}

#[test]
fn a_root_without_rules_is_not_a_registry() {
    let err = Registry::load(&fixture("nowhere")).expect_err("no such fixture");
    assert!(matches!(err, RegistryError::NotDirectory(_)), "{err:?}");
    // The old shape: the rules directory itself given as the root.
    let err = Registry::load(&fixture("clean/rules")).expect_err("no rules/ under it");
    assert!(
        matches!(err, RegistryError::NotDirectory(ref path) if path.ends_with("clean/rules/rules")),
        "{err:?}"
    );
}
