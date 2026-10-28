//! Brief v1: the validator agrees with the schema's own fixtures, and the
//! brief the driver builds passes it.

#![cfg(test)]

use std::path::Path;

use rsb_gauge_reconcile::brief::{self, Brief, FreshBrief};

fn fixtures(kind: &str) -> Vec<(String, serde_json::Value)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/brief-v1")
        .join(kind);
    let mut out: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| {
            let path = e.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            (
                path.display().to_string(),
                serde_json::from_str(&text).unwrap(),
            )
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn every_valid_fixture_passes_and_every_invalid_one_fails() {
    let valid = fixtures("valid");
    let invalid = fixtures("invalid");
    assert!(valid.len() >= 5 && invalid.len() >= 14);
    for (path, value) in valid {
        assert_eq!(brief::validate(&value), Ok(()), "{path}");
    }
    for (path, value) in invalid {
        assert!(brief::validate(&value).is_err(), "{path} passed");
    }
}

fn fresh() -> Brief {
    Brief::fresh(&FreshBrief {
        repo: "ref-rsb-serverless-stack",
        issue: 14,
        base_sha: "3f2a9c1e7b4d5a6f8e9d0c1b2a3f4e5d6c7b8a90",
        base_branch: "main",
        branch: "feat/14-invite-expiry",
        ticket_path: "specs/S-0002-invites/02-invite-expiry.md",
        spec: "S-0002",
        attempt: 1,
        runtime_bound: 3600,
    })
}

#[test]
fn the_fresh_coder_brief_is_the_fixture_and_passes() {
    let value = fresh().to_checked().unwrap();
    let fixture = fixtures("valid")
        .into_iter()
        .find(|(p, _)| p.ends_with("fresh-coder.json"))
        .unwrap()
        .1;
    assert_eq!(value, fixture);
}

#[test]
fn an_invalid_brief_is_refused_with_its_reasons() {
    let mut bad = fresh();
    bad.base_sha = "abc".into();
    bad.branch = "ghs_abcdefghijklmnopqrstuvwxyz0123456789".into();
    let errors = bad.to_checked().unwrap_err();
    assert!(
        errors.iter().any(|e| e.starts_with("base_sha")),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|e| e.contains("credential")),
        "{errors:?}"
    );
}

#[test]
fn the_branch_is_derived_from_the_ticket_as_scripts_brief_does() {
    let path = "specs/S-0002-invites/02-invite-expiry.md";
    assert_eq!(
        brief::branch("feature", 14, path).as_deref(),
        Some("feat/14-invite-expiry")
    );
    assert_eq!(
        brief::branch("technical", 3, path).as_deref(),
        Some("refactor/3-invite-expiry")
    );
    assert_eq!(
        brief::branch("groundwork", 3, path).as_deref(),
        Some("chore/3-invite-expiry")
    );
    assert_eq!(brief::branch("human", 3, path), None);
    let ticket = "---\nid: S-0002/2\nslice: feature\n---\n\n# Invite expiry\n";
    assert_eq!(brief::slice(ticket), Some("feature"));
    assert_eq!(brief::slice("# no front matter\n"), None);
}
