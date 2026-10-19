//! `cargo xtask errors`: no crate lists a banned error crate, and no source
//! boxes a `dyn Error`. See engineering/errors.md.

use crate::workspace::Crate;

/// Error crates that replace a typed error enum.
pub const BANNED: &[&str] = &[
    "anyhow",
    "eyre",
    "color-eyre",
    "failure",
    "error-chain",
    "snafu",
];

/// Every crate that lists a banned error crate, one line each.
pub fn banned_crates(crates: &[Crate]) -> Vec<String> {
    crates
        .iter()
        .flat_map(|c| {
            c.deps
                .iter()
                .filter(|d| BANNED.contains(&d.as_str()))
                .map(move |d| {
                    format!(
                        "{} lists {d}: use the crate's thiserror enum instead",
                        c.name
                    )
                })
        })
        .collect()
}

/// Every line of `source` that boxes a `dyn Error`, as `path:line: text`.
pub fn boxed_errors(path: &str, source: &str) -> Vec<String> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| boxes_dyn_error(line))
        .map(|(i, line)| format!("{path}:{}: {}", i + 1, line.trim()))
        .collect()
}

fn boxes_dyn_error(line: &str) -> bool {
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    compact.match_indices("Box<dyn").any(|(at, needle)| {
        let rest = compact.get(at + needle.len()..).unwrap_or_default();
        rest.split('>')
            .next()
            .is_some_and(|inner| inner.contains("Error"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn krate(name: &str, deps: &[&str]) -> Crate {
        Crate {
            name: name.into(),
            deps: deps.iter().map(|d| d.to_string()).collect(),
            dir: Default::default(),
        }
    }

    #[test]
    fn a_crate_listing_a_banned_error_crate_fails() {
        let crates = [
            krate("render", &["registry", "anyhow"]),
            krate("score", &["eyre", "thiserror"]),
            krate("registry", &["thiserror"]),
        ];
        assert_eq!(
            banned_crates(&crates),
            [
                "render lists anyhow: use the crate's thiserror enum instead",
                "score lists eyre: use the crate's thiserror enum instead",
            ]
        );
    }

    #[test]
    fn a_boxed_dyn_error_fails_with_its_line() {
        let source = "fn ok() -> Result<(), RawError> { Ok(()) }\n\
                      fn bad() -> Result<(), Box<dyn std::error::Error>> { Ok(()) }\n\
                      fn worse() -> Result<(), Box< dyn Error + Send >> { Ok(()) }\n\
                      fn render(e: &dyn std::error::Error) {}\n";
        assert_eq!(
            boxed_errors("crates/registry/src/lib.rs", source),
            [
                "crates/registry/src/lib.rs:2: fn bad() -> Result<(), Box<dyn std::error::Error>> { Ok(()) }",
                "crates/registry/src/lib.rs:3: fn worse() -> Result<(), Box< dyn Error + Send >> { Ok(()) }",
            ]
        );
    }
}
