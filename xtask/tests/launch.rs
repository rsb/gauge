//! `cargo xtask launch` against the seeded workspace `tests/fixtures/launch`:
//! the holder and test code may start processes, a library may not.

use std::path::Path;
use std::process::Command;

/// Runs `xtask launch` on `manifest`: whether it passed, and its stderr.
/// `None` if it could not be started.
fn launch(manifest: &Path) -> Option<(bool, String)> {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["launch", "--manifest-path"])
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
    assert_eq!(launch(&root), Some((true, String::new())));
}

#[test]
fn a_library_starting_a_process_fails() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/launch/Cargo.toml");
    assert_eq!(
        launch(&manifest),
        Some((
            false,
            "crates/rsb-gauge-schema/src/lib.rs:1: use std::process::Command;: \
             only agent-driver may start a process\n\
             crates/rsb-gauge-schema/src/lib.rs:4: let _ = Command::new(\"curl\").status();: \
             only agent-driver may start a process\n"
                .into()
        ))
    );
}
