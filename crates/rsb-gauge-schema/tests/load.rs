use std::path::Path;

use rsb_gauge_schema::rule::{CheckClass, Runtime, State, Toolchain};
use rsb_gauge_schema::{Problem, Registry, RegistryError, lint};

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn a_clean_registry_loads_every_rule_with_its_topic() {
    let registry = Registry::load(&fixture("clean")).expect("the clean fixture loads");
    let ids: Vec<&str> = registry
        .entries
        .iter()
        .map(|e| e.rule.id.as_str())
        .collect();
    assert_eq!(ids, ["ERR-1", "ERR-7", "DO-1"]);
    let topics: Vec<&str> = registry.entries.iter().map(|e| e.topic.as_str()).collect();
    assert_eq!(topics, ["errors", "errors", "web/cloudflare"]);

    let do_1 = registry.get("DO-1").expect("DO-1 is loaded").rule.clone();
    assert_eq!(do_1.check_class, CheckClass::Heuristic);
    assert_eq!(do_1.scope.toolchain, Some(Toolchain::Typescript));
    assert_eq!(do_1.scope.runtime, Some(Runtime::Cloudflare));
    assert_eq!(do_1.lifecycle.state, State::Trial);
    assert_eq!(do_1.checks.first().and_then(|c| c.precision), Some(0.8));

    assert_eq!(
        lint(&registry),
        Vec::new(),
        "a clean registry has no findings"
    );
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
            ("ERR-2.toml".to_owned(), Problem::FileName),
            ("ERR-2.toml".to_owned(), Problem::MissingWhy),
            ("ERR-2.toml".to_owned(), Problem::NoExemplar),
            ("ERR-2.toml".to_owned(), Problem::Unstamped),
            ("ERR-2.toml".to_owned(), Problem::BadPrecision),
            ("ERR-3.toml".to_owned(), Problem::DuplicateStatement),
            ("ERR-3.toml".to_owned(), Problem::RetiredWithoutReason),
            ("ERR-3.toml".to_owned(), Problem::BadDate),
        ]
    );
    let line = findings
        .first()
        .map(ToString::to_string)
        .unwrap_or_default();
    assert!(
        line.ends_with("ERR-2.toml: file-name: file is named \"ERR-2\" but the rule is \"ERR-20\""),
        "{line}"
    );
}

#[test]
fn a_file_that_does_not_parse_stops_the_load_and_names_the_file() {
    let dir = std::env::temp_dir().join(format!("gauge-registry-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(
        dir.join("BAD-1.toml"),
        "id = \"BAD-1\"\nseverity = \"high\"\n",
    )
    .expect("write");
    let err = Registry::load(&dir).expect_err("an unknown field refuses the file");
    assert!(
        matches!(err, RegistryError::Parse { ref path, .. } if path.ends_with("BAD-1.toml")),
        "{err:?}"
    );
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn a_missing_root_is_not_a_directory() {
    let err = Registry::load(&fixture("nowhere")).expect_err("no such fixture");
    assert!(matches!(err, RegistryError::NotDirectory(_)), "{err:?}");
}
