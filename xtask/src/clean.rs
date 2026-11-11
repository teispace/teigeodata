//! `cargo xtask clean`: frees disk ([resources](../../docs/06-engineering/resources.md)).

use std::fs;
use std::path::Path;
use std::process::Command;

/// Removes incremental caches and scratch directories of both workspaces (the main one and
/// `forge/`) and the forge's transient scratch (`data/build/tz-rules`); with `all`, also the
/// built packs and stage outputs in `data/build/` (a Nepal rebuild takes minutes) and runs
/// `cargo clean` in both workspaces. The fetched sources in `data/cache/store/` are kept:
/// they are pinned inputs, not build output.
pub(crate) fn run(root: &Path, all: bool) -> Result<String, Vec<String>> {
    let mut removed = Vec::new();
    let mut problems = Vec::new();

    let mut candidates = vec![root.join("scratch"), root.join("data/build/tz-rules")];
    if all {
        candidates.push(root.join("data/build"));
    }
    for target in [root.join("target"), root.join("forge/target")] {
        for profile_dir in read_dirs(&target) {
            candidates.push(profile_dir.join("incremental"));
        }
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
        for (dir, shown) in [
            (root.to_path_buf(), "target/"),
            (root.join("forge"), "forge/target/"),
        ] {
            if !dir.join("Cargo.toml").is_file() {
                continue;
            }
            let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
                .arg("clean")
                .current_dir(&dir)
                .status();
            match status {
                Ok(s) if s.success() => removed.push(format!("{shown} (cargo clean)")),
                Ok(s) => problems.push(format!("cargo clean in {shown} exited with {s}")),
                Err(e) => problems.push(format!("cargo clean in {shown}: {e}")),
            }
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
