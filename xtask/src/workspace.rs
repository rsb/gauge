//! The workspace as the checks see it, read from `cargo metadata`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// One workspace crate and the packages it lists in any dependency table.
#[derive(Debug, Clone)]
pub struct Crate {
    pub name: String,
    pub id: String,
    pub deps: Vec<String>,
    pub dir: PathBuf,
}

/// One resolved package: its name, and the ids of the packages it depends
/// on, of every kind and for every target.
#[derive(Debug, Clone, Default)]
pub struct Node {
    pub name: String,
    pub deps: Vec<String>,
}

/// The resolved dependency graph, by package id.
pub type Graph = BTreeMap<String, Node>;

/// The workspace's root, crates, resolved graph and `[workspace.metadata]`.
pub struct Workspace {
    pub root: PathBuf,
    pub crates: Vec<Crate>,
    pub graph: Graph,
    pub metadata: Value,
}

/// Loads the workspace whose root manifest is `manifest`, or the one cargo
/// finds from the current directory.
pub fn load(manifest: Option<&Path>) -> Result<Workspace, String> {
    let mut command = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    command.args(["metadata", "--format-version", "1"]);
    if let Some(manifest) = manifest {
        command.arg("--manifest-path").arg(manifest);
    }
    let output = command
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
    let members: Vec<&str> = json
        .get("workspace_members")
        .and_then(Value::as_array)
        .ok_or("cargo metadata: no workspace members")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let packages = json
        .get("packages")
        .and_then(Value::as_array)
        .ok_or("cargo metadata: no packages")?;
    let names: BTreeMap<&str, &str> = packages
        .iter()
        .map(|p| (str_at(p, "id"), str_at(p, "name")))
        .collect();
    let crates = packages
        .iter()
        .filter(|p| members.contains(&str_at(p, "id")))
        .map(|p| Crate {
            name: str_at(p, "name").to_owned(),
            id: str_at(p, "id").to_owned(),
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
    let graph = json
        .get("resolve")
        .and_then(|r| r.get("nodes"))
        .and_then(Value::as_array)
        .ok_or("cargo metadata: no resolved graph")?
        .iter()
        .map(|n| {
            let id = str_at(n, "id");
            let node = Node {
                name: names.get(id).copied().unwrap_or_default().to_owned(),
                deps: n
                    .get("deps")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|d| str_at(d, "pkg").to_owned())
                    .collect(),
            };
            (id.to_owned(), node)
        })
        .collect();
    let metadata = json.get("metadata").cloned().unwrap_or_default();
    Ok(Workspace {
        root: PathBuf::from(str_at(&json, "workspace_root")),
        crates,
        graph,
        metadata,
    })
}

fn str_at<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}
