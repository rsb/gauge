//! Workspace checks: `cargo xtask layers`, `cargo xtask errors`.

mod errors;
mod layers;
mod workspace;

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let task = std::env::args().nth(1);
    let result = match task.as_deref() {
        Some("layers") => layers(),
        Some("errors") => errors(),
        _ => Err(vec!["usage: cargo xtask <layers|errors>".into()]),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(lines) => {
            for line in lines {
                eprintln!("{line}");
            }
            ExitCode::FAILURE
        }
    }
}

fn layers() -> Result<(), Vec<String>> {
    let ws = workspace::load().map_err(|e| vec![e])?;
    let table = layers::table(&ws.metadata).map_err(|e| vec![e])?;
    failures(layers::check(&table, &ws.crates))
}

fn errors() -> Result<(), Vec<String>> {
    let ws = workspace::load().map_err(|e| vec![e])?;
    let root = std::env::current_dir().map_err(|e| vec![format!("current dir: {e}")])?;
    let mut out = errors::banned_crates(&ws.crates);
    for c in &ws.crates {
        // The errors check names the pattern it looks for.
        let own = (c.name == "xtask").then(|| c.dir.join("src/errors.rs"));
        for file in rust_files(&c.dir).map_err(|e| vec![format!("{}: {e}", c.dir.display())])? {
            if own.as_ref() == Some(&file) {
                continue;
            }
            let source = std::fs::read_to_string(&file)
                .map_err(|e| vec![format!("{}: {e}", file.display())])?;
            let shown = file
                .strip_prefix(&root)
                .unwrap_or(&file)
                .display()
                .to_string();
            out.extend(errors::boxed_errors(&shown, &source));
        }
    }
    failures(out)
}

fn failures(lines: Vec<String>) -> Result<(), Vec<String>> {
    if lines.is_empty() { Ok(()) } else { Err(lines) }
}

/// Every `.rs` file under `dir`, skipping build output.
fn rust_files(dir: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "target") {
                out.extend(rust_files(&path)?);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(out)
}
