//! `cargo xtask launch`: no crate but the custody holder and `xtask` starts a
//! process. A source scan beside the dependency check in `custody`, which
//! cannot see the standard library. See engineering/layers.md.

/// Items of `std::process` that do not start a process.
const NOT_LAUNCHERS: &[&str] = &["ExitCode", "Termination", "exit", "abort", "id"];

/// Every line of `source` that names a process launcher, as
/// `path:line: text`, outside comments and items marked `#[cfg(test)]`.
pub fn launches(path: &str, source: &str, holder: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut test = Test::Out;
    for (i, line) in source.lines().enumerate() {
        let code = line.split("//").next().unwrap_or_default();
        let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        if test == Test::Out {
            match compact.split_once("#[cfg(test)]") {
                Some((_, rest)) => test = Test::Pending.after(rest),
                None => {
                    if compact.contains("Command::new") || names_a_launcher(&compact) {
                        out.push(format!(
                            "{path}:{}: {}: only {holder} may start a process",
                            i + 1,
                            line.trim()
                        ));
                    }
                }
            }
        } else {
            test = test.after(&compact);
        }
    }
    out
}

/// Where the scan stands against a `#[cfg(test)]` item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Test {
    /// Outside any test item.
    Out,
    /// After the attribute, before the item's `{` or `;`.
    Pending,
    /// Inside the item's braces, this deep.
    In(usize),
}

impl Test {
    fn after(self, code: &str) -> Test {
        code.chars().fold(self, |state, c| match (state, c) {
            (Test::Pending, '{') => Test::In(1),
            (Test::Pending, ';') | (Test::In(1), '}') => Test::Out,
            (Test::In(n), '{') => Test::In(n + 1),
            (Test::In(n), '}') => Test::In(n - 1),
            (state, _) => state,
        })
    }
}

/// Whether a whitespace-free line names `std::process` other than through
/// the items that do not start a process. A `use` group left open on the
/// line counts.
fn names_a_launcher(compact: &str) -> bool {
    compact.match_indices("std::process").any(|(at, needle)| {
        let rest = compact.get(at + needle.len()..).unwrap_or_default();
        let Some(rest) = rest.strip_prefix("::") else {
            return true;
        };
        match rest.strip_prefix('{') {
            Some(group) => match group.split_once('}') {
                Some((items, _)) => items
                    .split(',')
                    .map(ident)
                    .any(|item| !item.is_empty() && !NOT_LAUNCHERS.contains(&item)),
                None => true,
            },
            None => !NOT_LAUNCHERS.contains(&ident(rest)),
        }
    })
}

fn ident(s: &str) -> &str {
    let end = s
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(s.len());
    s.get(..end).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(source: &str) -> Vec<String> {
        launches("src/lib.rs", source, "agent-driver")
    }

    #[test]
    fn a_launcher_fails_with_its_line() {
        let source = "use std::process::Command;\n\
                      fn a() { let _ = tokio::process::Command::new(\"sh\"); }\n\
                      use std::process::{ExitCode, Stdio};\n\
                      use std::process;\n\
                      use std::process::{\n";
        assert_eq!(
            scan(source),
            [
                "src/lib.rs:1: use std::process::Command;: only agent-driver may start a process",
                "src/lib.rs:2: fn a() { let _ = tokio::process::Command::new(\"sh\"); }: \
                 only agent-driver may start a process",
                "src/lib.rs:3: use std::process::{ExitCode, Stdio};: \
                 only agent-driver may start a process",
                "src/lib.rs:4: use std::process;: only agent-driver may start a process",
                "src/lib.rs:5: use std::process::{: only agent-driver may start a process",
            ]
        );
    }

    #[test]
    fn items_that_start_no_process_pass() {
        let source = "use std::process::ExitCode;\n\
                      use std::process::{ExitCode, exit};\n\
                      fn a() { std::process::exit(1) }\n\
                      enum Command { Lint }\n\
                      // Command::new is for agent-driver.\n";
        assert_eq!(scan(source), Vec::<String>::new());
    }

    #[test]
    fn test_items_are_skipped() {
        let source = "#[cfg(test)]\n\
                      use std::process::Command;\n\
                      #[cfg(test)]\n\
                      mod tests {\n\
                          fn a() { std::process::Command::new(\"sh\"); }\n\
                          mod inner { use std::process::Stdio; }\n\
                      }\n\
                      fn b() { std::process::Command::new(\"sh\"); }\n\
                      #[cfg(test)] mod more { use std::process::Child; }\n\
                      use std::process::Child;\n";
        assert_eq!(
            scan(source),
            [
                "src/lib.rs:8: fn b() { std::process::Command::new(\"sh\"); }: \
                 only agent-driver may start a process",
                "src/lib.rs:10: use std::process::Child;: only agent-driver may start a process",
            ]
        );
    }
}
