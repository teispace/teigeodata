//! `cargo xtask clean`: frees disk ([resources](../../docs/06-engineering/resources.md)).

use std::fs;
use std::path::Path;
use std::process::Command;

/// Removes incremental caches and scratch directories; with `all`, runs `cargo clean` too.
pub(crate) fn run(root: &Path, all: bool) -> Result<String, Vec<String>> {
    let mut removed = Vec::new();
    let mut problems = Vec::new();

    let mut candidates = vec![root.join("scratch"), root.join("data/build")];
    for profile_dir in read_dirs(&root.join("target")) {
        candidates.push(profile_dir.join("incremental"));
    }
    for dir in candidates {
        if dir.is_dir() {
            match fs::remove_dir_all(&dir) {
                Ok(()) => removed.push(dir.display().to_string()),
                Err(e) => problems.push(format!("{}: {e}", dir.display())),
            }
        }
    }

    if all {
        let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .arg("clean")
            .current_dir(root)
            .status();
        match status {
            Ok(s) if s.success() => removed.push("target/ (cargo clean)".into()),
            Ok(s) => problems.push(format!("cargo clean exited with {s}")),
            Err(e) => problems.push(format!("cargo clean: {e}")),
        }
    }

    if problems.is_empty() {
        Ok(if removed.is_empty() {
            "nothing to clean".into()
        } else {
            format!("removed:\n  {}", removed.join("\n  "))
        })
    } else {
        Err(problems)
    }
}

/// The sub-directories of `dir`, or none when it does not exist.
fn read_dirs(dir: &Path) -> Vec<std::path::PathBuf> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default()
}
