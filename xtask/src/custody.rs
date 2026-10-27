//! Custody, checked by `cargo xtask layers`: only the holder depends on a
//! crate in the custody table, and no crate depends on the holder. The table
//! is `[workspace.metadata.custody]`. See engineering/layers.md.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::Value;

use crate::workspace::{Crate, Graph};

/// How far down a crate's dependencies a custody crate counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// At any depth: a GitHub client, an HTTP client, a JWT signer.
    Transitive,
    /// Only where a crate lists it itself: a crate nearly every runtime links,
    /// such as `libc`, whose direct use is the launcher.
    Direct,
}

/// The one crate allowed custody, and the crates that count as custody.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Custody {
    pub holder: String,
    pub crates: BTreeMap<String, Reach>,
}

pub fn table(metadata: &Value) -> Result<Custody, String> {
    let custody = metadata
        .get("custody")
        .and_then(Value::as_object)
        .ok_or("no [workspace.metadata.custody] table")?;
    let holder = custody
        .get("holder")
        .and_then(Value::as_str)
        .ok_or("[workspace.metadata.custody]: holder is the name of one crate")?
        .to_owned();
    let mut crates = BTreeMap::new();
    for (key, reach) in [("transitive", Reach::Transitive), ("direct", Reach::Direct)] {
        let names = custody.get(key).and_then(Value::as_array).ok_or_else(|| {
            format!("[workspace.metadata.custody]: {key} is a list of crate names")
        })?;
        for name in names {
            let name = name.as_str().ok_or_else(|| {
                format!("[workspace.metadata.custody]: {key} holds crate names, not {name}")
            })?;
            if crates.insert(name.to_owned(), reach).is_some() {
                return Err(format!(
                    "[workspace.metadata.custody]: {name} is listed twice"
                ));
            }
        }
    }
    for key in custody.keys() {
        if !["holder", "transitive", "direct"].contains(&key.as_str()) {
            return Err(format!("[workspace.metadata.custody]: unknown key {key}"));
        }
    }
    Ok(Custody { holder, crates })
}

/// Every violation, one line each.
pub fn check(custody: &Custody, crates: &[Crate], graph: &Graph) -> Vec<String> {
    let holder = custody.holder.as_str();
    if !crates.iter().any(|c| c.name == holder) {
        return vec![format!("custody holder {holder} is not a workspace crate")];
    }
    let mut out = Vec::new();
    for c in crates.iter().filter(|c| c.name != holder) {
        let mut seen = BTreeSet::new();
        for dep in &c.deps {
            if (dep == holder || custody.crates.contains_key(dep)) && seen.insert(dep.as_str()) {
                out.push(violation(custody, &c.name, dep, &[]));
            }
        }
        for (dep, path) in reached(custody, &c.id, graph) {
            if seen.insert(dep) {
                out.push(violation(custody, &c.name, dep, &path));
            }
        }
    }
    out
}

fn violation(custody: &Custody, from: &str, dep: &str, path: &[&str]) -> String {
    let through = if path.is_empty() {
        String::new()
    } else {
        format!(" through {}", path.join(" -> "))
    };
    if dep == custody.holder {
        format!("{from} depends on {dep}{through}: no crate depends on the custody holder")
    } else {
        format!(
            "{from} depends on {dep}{through}: only {} may depend on a custody crate",
            custody.holder
        )
    }
}

/// The holder and each transitive custody crate reachable from `root`, with
/// the names between, on the shortest path. The holder's own dependencies
/// are not walked.
fn reached<'a>(custody: &Custody, root: &'a str, graph: &'a Graph) -> Vec<(&'a str, Vec<&'a str>)> {
    let mut parent: BTreeMap<&str, &str> = BTreeMap::new();
    let mut queue = VecDeque::from([root]);
    let mut found = Vec::new();
    while let Some(id) = queue.pop_front() {
        let Some(node) = graph.get(id) else { continue };
        for dep in &node.deps {
            if dep == root || parent.contains_key(dep.as_str()) {
                continue;
            }
            parent.insert(dep, id);
            let Some(target) = graph.get(dep) else {
                continue;
            };
            let name = target.name.as_str();
            let is_holder = name == custody.holder;
            if is_holder || custody.crates.get(name) == Some(&Reach::Transitive) {
                found.push((name, path_to(root, id, &parent, graph)));
            }
            if !is_holder {
                queue.push_back(dep);
            }
        }
    }
    found
}

/// The names from `root` (exclusive) to `last` (inclusive).
fn path_to<'a>(
    root: &str,
    last: &'a str,
    parent: &BTreeMap<&'a str, &'a str>,
    graph: &'a Graph,
) -> Vec<&'a str> {
    let mut path = Vec::new();
    let mut at = last;
    while at != root {
        path.push(graph.get(at).map_or(at, |n| n.name.as_str()));
        match parent.get(at) {
            Some(up) => at = up,
            None => break,
        }
    }
    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Node;
    use serde_json::json;

    fn custody() -> Custody {
        table(&json!({ "custody": {
            "holder": "agent-driver",
            "transitive": ["ureq", "jsonwebtoken"],
            "direct": ["libc"],
        }}))
        .unwrap()
    }

    /// A workspace of `members`, each `(name, deps)`, over a graph of
    /// `(name, deps)` where every id is the name.
    fn workspace(members: &[(&str, &[&str])], others: &[(&str, &[&str])]) -> (Vec<Crate>, Graph) {
        let crates = members
            .iter()
            .map(|(name, deps)| Crate {
                name: (*name).into(),
                id: (*name).into(),
                deps: deps.iter().map(|d| d.to_string()).collect(),
                dir: Default::default(),
            })
            .collect();
        let graph = members
            .iter()
            .chain(others)
            .map(|(name, deps)| {
                let node = Node {
                    name: (*name).into(),
                    deps: deps.iter().map(|d| d.to_string()).collect(),
                };
                (name.to_string(), node)
            })
            .collect();
        (crates, graph)
    }

    #[test]
    fn the_holder_alone_depending_on_custody_crates_passes() {
        let (crates, graph) = workspace(
            &[
                ("rsb-gauge-schema", &["serde"]),
                ("rsb-gauge-cli", &["rsb-gauge-schema"]),
                ("agent-driver", &["ureq", "jsonwebtoken", "libc"]),
            ],
            &[
                ("serde", &[]),
                ("ureq", &["libc"]),
                ("jsonwebtoken", &[]),
                ("libc", &[]),
            ],
        );
        assert_eq!(check(&custody(), &crates, &graph), Vec::<String>::new());
    }

    #[test]
    fn a_direct_custody_dependency_fails() {
        let (crates, graph) = workspace(
            &[("rsb-gauge-schema", &["ureq"]), ("agent-driver", &[])],
            &[("ureq", &[])],
        );
        assert_eq!(
            check(&custody(), &crates, &graph),
            ["rsb-gauge-schema depends on ureq: only agent-driver may depend on a custody crate"]
        );
    }

    #[test]
    fn a_transitive_custody_dependency_fails_naming_the_path() {
        let (crates, graph) = workspace(
            &[
                ("rsb-gauge-reconcile", &["rsb-gauge-schema"]),
                ("rsb-gauge-schema", &["toolkit"]),
                ("agent-driver", &[]),
            ],
            &[("toolkit", &["octo"]), ("octo", &["ureq"]), ("ureq", &[])],
        );
        assert_eq!(
            check(&custody(), &crates, &graph),
            [
                "rsb-gauge-reconcile depends on ureq through rsb-gauge-schema -> toolkit -> octo: \
                 only agent-driver may depend on a custody crate",
                "rsb-gauge-schema depends on ureq through toolkit -> octo: \
                 only agent-driver may depend on a custody crate",
            ]
        );
    }

    #[test]
    fn a_direct_only_crate_counts_when_listed_not_when_reached() {
        let (crates, graph) = workspace(
            &[
                ("rsb-gauge-store", &["runtime"]),
                ("rsb-gauge-cli", &["libc"]),
                ("agent-driver", &[]),
            ],
            &[("runtime", &["libc"]), ("libc", &[])],
        );
        assert_eq!(
            check(&custody(), &crates, &graph),
            ["rsb-gauge-cli depends on libc: only agent-driver may depend on a custody crate"]
        );
    }

    #[test]
    fn any_crate_depending_on_the_holder_fails_once() {
        let (crates, graph) = workspace(
            &[
                ("rsb-gauge-cli", &["agent-driver"]),
                ("xtask", &["helper"]),
                ("agent-driver", &["ureq"]),
            ],
            &[("helper", &["agent-driver"]), ("ureq", &[])],
        );
        assert_eq!(
            check(&custody(), &crates, &graph),
            [
                "rsb-gauge-cli depends on agent-driver: no crate depends on the custody holder",
                "xtask depends on agent-driver through helper: no crate depends on the custody holder",
            ]
        );
    }

    #[test]
    fn a_holder_outside_the_workspace_fails() {
        let (crates, graph) = workspace(&[("rsb-gauge-schema", &[])], &[]);
        assert_eq!(
            check(&custody(), &crates, &graph),
            ["custody holder agent-driver is not a workspace crate"]
        );
    }

    #[test]
    fn a_malformed_table_is_an_error() {
        assert!(table(&json!({})).is_err());
        assert!(table(&json!({ "custody": { "transitive": [], "direct": [] }})).is_err());
        assert!(
            table(&json!({ "custody": { "holder": "a", "transitive": ["x"], "direct": ["x"] }}))
                .is_err()
        );
        assert!(
            table(&json!({ "custody": {
                "holder": "a", "transitive": [], "direct": [], "allowed": ["b"]
            }}))
            .is_err()
        );
    }
}
