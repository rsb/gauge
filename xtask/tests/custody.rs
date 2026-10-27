//! `cargo xtask layers` against seeded workspaces under `tests/fixtures`.
//! Registry crates are stood in for by path crates under `fixtures/stubs`,
//! so every fixture resolves offline; the check reads names, not code.

use std::path::{Path, PathBuf};
use std::process::Command;

fn manifest(fixture: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture)
        .join("Cargo.toml")
}

/// Runs `xtask layers` on `manifest`: whether it passed, and its stderr.
/// `None` if it could not be started.
fn layers(manifest: &Path) -> Option<(bool, String)> {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["layers", "--manifest-path"])
        .arg(manifest)
        .output()
        .ok()?;
    Some((
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

#[test]
fn the_real_workspace_passes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml");
    assert_eq!(layers(&root), Some((true, String::new())));
}

#[test]
fn a_clean_fixture_passes() {
    assert_eq!(layers(&manifest("clean")), Some((true, String::new())));
}

#[test]
fn a_library_depending_on_a_custody_crate_fails() {
    assert_eq!(
        layers(&manifest("direct")),
        Some((
            false,
            "rsb-gauge-schema depends on ureq: only agent-driver may depend on a custody crate\n"
                .into()
        ))
    );
}

#[test]
fn a_library_reaching_a_custody_crate_transitively_fails() {
    assert_eq!(
        layers(&manifest("transitive")),
        Some((
            false,
            "rsb-gauge-reconcile depends on ureq through toolkit: \
             only agent-driver may depend on a custody crate\n"
                .into()
        ))
    );
}

#[test]
fn the_cli_depending_on_agent_driver_fails() {
    assert_eq!(
        layers(&manifest("driver-dependent")),
        Some((
            false,
            "rsb-gauge-cli depends on agent-driver: no crate depends on the custody holder\n"
                .into()
        ))
    );
}
