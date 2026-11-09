//! `cargo xtask sizes`: Z2, the bytes each feature set of the runtime crates adds to a linked
//! library ([benchmarks](../../docs/06-engineering/benchmarks.md)).
//!
//! Builds `tools/size-probe` (a cdylib whose features each export one entry point) once per
//! feature set, natively with the release profile and for `wasm32-unknown-unknown` with the
//! size-optimised `wasm` profile, and subtracts a baseline build that links what any host
//! library has already (`std` formatting, a panic path, the allocator).
//! Sizes are raw and `gzip -9`.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The feature sets measured, in the order shown.
const SETS: [(&str, &str); 10] = [
    (
        "",
        "baseline: std formatting, panics, allocator (subtracted)",
    ),
    ("open", "open: head checks, section lookup"),
    ("verify", "open + BLAKE3 of every section"),
    ("signature", "open + Ed25519 signature check"),
    (
        "sections",
        "open + every section view (places, hierarchy, ID map, names, components)",
    ),
    ("blobs", "open + BLOBS (`ruzstd`)"),
    ("reader", "the whole reader: all of the above"),
    ("write", "the writer alone (forge side)"),
    (
        "engine-points",
        "the engine, points only: open, `get`, `reverse`",
    ),
    ("engine-all", "the engine, every query"),
];

/// One build: its bytes raw and gzipped.
#[derive(Clone, Copy, Default)]
struct Size {
    raw: u64,
    gz: u64,
}

pub(crate) fn run(root: &Path, wasm: bool, native: bool) -> Result<String, Vec<String>> {
    let probe = root.join("tools/size-probe");
    let mut targets = Vec::new();
    if native {
        targets.push(None);
    }
    if wasm {
        targets.push(Some("wasm32-unknown-unknown"));
    }
    let mut out = String::from(
        "| Feature set | native B | native gzip | wasm32 B | wasm32 gzip |\n|---|---:|---:|---:|---:|\n",
    );
    let mut rows: Vec<[Option<Size>; 2]> = vec![[None; 2]; SETS.len()];
    let mut problems = Vec::new();
    for target in &targets {
        for (i, (features, _)) in SETS.iter().enumerate() {
            match build(&probe, features, *target) {
                Ok(size) => {
                    if let Some(slot) = rows
                        .get_mut(i)
                        .and_then(|row| row.get_mut(usize::from(target.is_some())))
                    {
                        *slot = Some(size);
                    }
                }
                Err(e) => problems.push(format!(
                    "{} on {}: {e}",
                    show(features),
                    target.unwrap_or("native")
                )),
            }
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let base = rows.first().copied().unwrap_or_default();
    for ((_, label), row) in SETS.iter().zip(&rows).skip(1) {
        let cell = |k: usize| match (
            row.get(k).copied().flatten(),
            base.get(k).copied().flatten(),
        ) {
            (Some(s), Some(b)) => (
                kb(s.raw.saturating_sub(b.raw)),
                kb(s.gz.saturating_sub(b.gz)),
            ),
            _ => ("–".into(), "–".into()),
        };
        let ((nr, ng), (wr, wg)) = (cell(0), cell(1));
        let _ = writeln!(out, "| {label} | {nr} | {ng} | {wr} | {wg} |");
    }
    let empty = |k: usize| {
        base.get(k)
            .copied()
            .flatten()
            .map_or("–".into(), |s| kb(s.raw))
    };
    let _ = write!(
        out,
        "\nBaseline (subtracted): native {}, wasm32 {}.",
        empty(0),
        empty(1)
    );
    Ok(out)
}

fn show(features: &str) -> &str {
    if features.is_empty() {
        "empty"
    } else {
        features
    }
}

fn kb(bytes: u64) -> String {
    #[allow(
        clippy::cast_precision_loss,
        reason = "library sizes are far below 2^52"
    )]
    let k = bytes as f64 / 1024.0;
    format!("{k:.1} KiB")
}

/// Builds the probe with `features` for `target` (native when `None`); its size.
fn build(probe: &Path, features: &str, target: Option<&str>) -> Result<Size, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(probe)
        .args(["build", "--quiet", "--locked"]);
    let profile = if target.is_some() { "wasm" } else { "release" };
    cmd.args(["--profile", profile]);
    if let Some(t) = target {
        cmd.args(["--target", t]);
    }
    if !features.is_empty() {
        cmd.args(["--features", features]);
    }
    let status = cmd.status().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("cargo build exited with {status}"));
    }
    let file = artifact(probe, target, profile)?;
    let raw = fs::metadata(&file)
        .map_err(|e| format!("{}: {e}", file.display()))?
        .len();
    let gz = Command::new("gzip")
        .args(["-9", "-c"])
        .arg(&file)
        .output()
        .map_err(|e| format!("gzip: {e}"))?;
    if !gz.status.success() {
        return Err(format!("gzip exited with {}", gz.status));
    }
    Ok(Size {
        raw,
        gz: gz.stdout.len() as u64,
    })
}

/// The built library's path.
fn artifact(probe: &Path, target: Option<&str>, profile: &str) -> Result<PathBuf, String> {
    let dir = match target {
        Some(t) => probe.join("target").join(t).join(profile),
        None => probe.join("target").join(profile),
    };
    let names: &[&str] = if target.is_some() {
        &["size_probe.wasm"]
    } else {
        &["libsize_probe.dylib", "libsize_probe.so", "size_probe.dll"]
    };
    names
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
        .ok_or_else(|| format!("no library in {}", dir.display()))
}
