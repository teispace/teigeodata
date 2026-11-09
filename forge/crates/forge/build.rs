//! Per-stage code versions for `forge make` (D-108).
//!
//! A stage's code is its root module and every module it reaches through `crate::` paths,
//! transitively, plus the path dependencies (`crates/teigeodata-*`) and `Cargo.lock`. The
//! build hashes those files (BLAKE3, sorted by path) into `FORGE_CODE_<STAGE>`, so a change to
//! one stage's code reruns that stage and the stages after it, not the whole pipeline. The
//! reach is computed from the sources, so it cannot go stale; `main.rs` and `make.rs`
//! (dispatch and orchestration) are in every stage's code.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a build script reports a failure by panicking"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// (stage, its root module).
const STAGES: [(&str, &str); 6] = [
    ("READ", "read"),
    ("CONFLATE", "conflate"),
    ("BUILDINGS", "buildings"),
    ("CORE", "core"),
    ("TOPOLOGY", "topo"),
    ("BUILD", "build"),
];

/// The path dependencies, relative to this crate.
const DEPS: [&str; 3] = [
    "../../../crates/teigeodata-core",
    "../../../crates/teigeodata-format",
    "../../../crates/teigeodata-names",
];

fn main() {
    let here = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let src = here.join("src");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    // Every top-level module: its files and the modules its files name.
    let mut files: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for entry in fs::read_dir(&src).expect("src") {
        let path = entry.expect("entry").path();
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("utf-8 name")
            .to_string();
        let list = files.entry(name).or_default();
        if path.is_dir() {
            walk(&path, list);
        } else if path.extension().is_some_and(|e| e == "rs") {
            list.push(path);
        }
    }
    let mut uses: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (m, list) in &files {
        let set = uses.entry(m.clone()).or_default();
        for f in list {
            let text = fs::read_to_string(f).expect("source");
            for r in crate_refs(&text) {
                if r != *m && files.contains_key(&r) {
                    set.insert(r);
                }
            }
        }
    }
    let mut common: Vec<PathBuf> = vec![here.join("../../Cargo.lock"), here.join("Cargo.toml")];
    for d in DEPS {
        let d = here.join(d);
        println!("cargo:rerun-if-changed={}", d.display());
        common.push(d.join("Cargo.toml"));
        walk(&d.join("src"), &mut common);
    }
    for (stage, root) in STAGES {
        assert!(files.contains_key(root), "stage module {root} is missing");
        let mut reach = BTreeSet::from([root.to_string(), "main".into(), "make".into()]);
        let mut todo = vec![root.to_string()];
        while let Some(m) = todo.pop() {
            for r in uses.get(&m).into_iter().flatten() {
                if reach.insert(r.clone()) {
                    todo.push(r.clone());
                }
            }
        }
        let mut paths: Vec<&PathBuf> = reach
            .iter()
            .filter_map(|m| files.get(m))
            .flatten()
            .chain(common.iter())
            .collect();
        paths.sort();
        let mut h = blake3::Hasher::new();
        for p in paths {
            let rel = p.strip_prefix(&here).unwrap_or(p);
            h.update(rel.to_string_lossy().as_bytes());
            h.update(&[0]);
            h.update(&fs::read(p).unwrap_or_else(|e| panic!("{}: {e}", p.display())));
            h.update(&[0]);
        }
        println!(
            "cargo:rustc-env=FORGE_CODE_{stage}={}",
            h.finalize().to_hex()
        );
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for entry in rd {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The first segments of every `crate::` path in a source file, including grouped imports
/// (`crate::{a, b::c}`).
fn crate_refs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("crate::") {
        rest = &rest[i + "crate::".len()..];
        if let Some(group) = rest.strip_prefix('{') {
            // Top-level items of the group, to its matching brace.
            let (mut depth, mut item, mut at_start) = (0_usize, String::new(), true);
            for c in group.chars() {
                match c {
                    '{' => depth += 1,
                    '}' if depth == 0 => break,
                    '}' => depth -= 1,
                    ',' if depth == 0 => {
                        out.push(std::mem::take(&mut item));
                        at_start = true;
                        continue;
                    }
                    _ => {}
                }
                if depth == 0 && at_start {
                    if c.is_alphanumeric() || c == '_' {
                        item.push(c);
                    } else if !item.is_empty() {
                        at_start = false;
                    }
                }
            }
            out.push(item);
        } else {
            out.push(
                rest.chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect(),
            );
        }
    }
    out.retain(|s| !s.is_empty());
    out
}
