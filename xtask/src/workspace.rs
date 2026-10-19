//! The workspace as the checks see it, read from `cargo metadata`.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// One workspace crate and the packages it lists in any dependency table.
#[derive(Debug, Clone)]
pub struct Crate {
    pub name: String,
    pub deps: Vec<String>,
    pub dir: PathBuf,
}

/// The workspace's crates and its `[workspace.metadata]`.
pub struct Workspace {
    pub crates: Vec<Crate>,
    pub metadata: Value,
}

pub fn load() -> Result<Workspace, String> {
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let json: Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    let crates = json
        .get("packages")
        .and_then(Value::as_array)
        .ok_or("cargo metadata: no packages")?
        .iter()
        .map(|p| Crate {
            name: str_at(p, "name").to_owned(),
            deps: p
                .get("dependencies")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|d| str_at(d, "name").to_owned())
                .collect(),
            dir: PathBuf::from(str_at(p, "manifest_path"))
                .parent()
                .map(PathBuf::from)
                .unwrap_or_default(),
        })
        .collect();
    let metadata = json.get("metadata").cloned().unwrap_or_default();
    Ok(Workspace { crates, metadata })
}

fn str_at<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}
