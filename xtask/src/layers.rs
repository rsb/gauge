//! `cargo xtask layers`: a crate depends only on crates in lower layers.
//! The table is `[workspace.metadata.layers]`. See engineering/layers.md.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::workspace::Crate;

/// Where a workspace crate sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    At(u64),
    /// Binaries and tooling: their own dependencies are not checked, and no
    /// layered crate may depend on them.
    Exempt,
}

pub type Table = BTreeMap<String, Layer>;

pub fn table(metadata: &Value) -> Result<Table, String> {
    let entries = metadata
        .get("layers")
        .and_then(Value::as_object)
        .ok_or("no [workspace.metadata.layers] table")?;
    entries
        .iter()
        .map(|(name, value)| match (value.as_u64(), value.as_str()) {
            (Some(n), _) => Ok((name.clone(), Layer::At(n))),
            (_, Some("exempt")) => Ok((name.clone(), Layer::Exempt)),
            _ => Err(format!(
                "{name}: a layer is a number or \"exempt\", not {value}"
            )),
        })
        .collect()
}

/// Every violation, one line each.
pub fn check(table: &Table, crates: &[Crate]) -> Vec<String> {
    let workspace: Vec<&str> = crates.iter().map(|c| c.name.as_str()).collect();
    let mut out = Vec::new();
    for c in crates {
        let from = match table.get(&c.name) {
            None => {
                out.push(format!(
                    "{}: not in [workspace.metadata.layers]; give it a layer",
                    c.name
                ));
                continue;
            }
            Some(Layer::Exempt) => continue,
            Some(Layer::At(n)) => *n,
        };
        for dep in c
            .deps
            .iter()
            .filter(|d| workspace.contains(&d.as_str()) && **d != c.name)
        {
            match table.get(dep) {
                Some(Layer::At(to)) if *to < from => {}
                Some(Layer::At(to)) => out.push(format!(
                    "{} (layer {from}) depends on {dep} (layer {to}): a crate depends only on lower layers",
                    c.name
                )),
                Some(Layer::Exempt) => out.push(format!(
                    "{} (layer {from}) depends on {dep} (exempt): no layered crate depends on a binary or tooling",
                    c.name
                )),
                // Reported under its own name above.
                None => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn krate(name: &str, deps: &[&str]) -> Crate {
        Crate {
            name: name.into(),
            id: name.into(),
            deps: deps.iter().map(|d| d.to_string()).collect(),
            dir: Default::default(),
        }
    }

    fn gauge_table() -> Table {
        table(&json!({ "layers": {
            "registry": 0, "render": 1, "checks": 1, "score": 2, "lab": 3,
            "insights": "exempt", "rsb-gauge-cli": "exempt", "xtask": "exempt",
        }}))
        .unwrap()
    }

    #[test]
    fn crates_that_depend_only_on_lower_layers_pass() {
        let crates = [
            krate("registry", &["thiserror"]),
            krate("render", &["registry"]),
            krate("score", &["registry", "checks", "serde"]),
            krate("lab", &["score"]),
            krate("insights", &["lab", "registry"]),
            krate("xtask", &["serde_json"]),
        ];
        assert_eq!(check(&gauge_table(), &crates), Vec::<String>::new());
    }

    #[test]
    fn a_dependency_on_a_higher_layer_fails_naming_both_crates() {
        let crates = [
            krate("render", &["registry", "score"]),
            krate("score", &["render"]),
        ];
        assert_eq!(
            check(&gauge_table(), &crates),
            ["render (layer 1) depends on score (layer 2): a crate depends only on lower layers"]
        );
    }

    #[test]
    fn a_dependency_within_one_layer_fails() {
        let crates = [krate("lab", &["score"]), krate("score", &["lab"])];
        assert_eq!(
            check(&gauge_table(), &crates),
            ["score (layer 2) depends on lab (layer 3): a crate depends only on lower layers"]
        );
        let mut same = gauge_table();
        same.insert("lab".into(), Layer::At(2));
        assert_eq!(check(&same, &crates).len(), 2);
    }

    #[test]
    fn a_crate_missing_from_the_table_fails() {
        assert_eq!(
            check(&gauge_table(), &[krate("catalog", &[])]),
            ["catalog: not in [workspace.metadata.layers]; give it a layer"]
        );
    }

    #[test]
    fn a_layered_crate_may_not_depend_on_an_exempt_one() {
        let crates = [krate("score", &["xtask"]), krate("xtask", &[])];
        assert_eq!(
            check(&gauge_table(), &crates),
            [
                "score (layer 2) depends on xtask (exempt): no layered crate depends on a binary or tooling"
            ]
        );
    }
}
