//! `cargo xtask playground [--fetch] [--no-build]`: writes what the site's playground loads to
//! `site/public/playground/` (git-ignored; phase 7.6,
//! [research](../../docs/02-research/notes/docs-playground.md)): the full WebAssembly package
//! built from this commit, the pinned pack checked by SHA-256, the catalog of packs a reader can
//! add at run time (`site/packs.json`, D-357: each cut by `tgd extract` from a pinned release
//! asset and checked against the SHA-256 written there), and a `manifest.json` that names them. The page loads these files from its own origin at run time, so it runs exactly the files
//! a user installs; every query runs in the page (only the map's tiles come from OpenStreetMap).

use std::fs;
use std::path::Path;

use std::collections::BTreeMap;
use std::process::Command;

use serde::Deserialize;

use crate::docs_examples::{Data, data, pinned, sha256_file};
use crate::package_wasm;

const USAGE: &str = "usage: cargo xtask playground [--fetch] [--no-build]";

/// Where the files go, relative to the repository root.
const OUT: &str = "site/public/playground";

/// The catalog, relative to the repository root.
const CATALOG: &str = "site/packs.json";

/// `site/packs.json`.
#[derive(Deserialize)]
struct Catalog {
    sources: BTreeMap<String, Data>,
    packs: Vec<Entry>,
}

/// A pack of the catalog: how it is cut, and what the page shows of it.
#[derive(Deserialize, serde::Serialize)]
struct Entry {
    id: String,
    title: String,
    summary: String,
    source: String,
    extract: Vec<String>,
    bbox: [f64; 4],
    contents: Vec<String>,
    bytes: u64,
    sha256: String,
}

/// The package files a browser loads: the entry, the reader, wasm-bindgen's glue, the module,
/// and the notices that go with them.
const ENGINE: [&str; 6] = [
    "index.js",
    "reader.js",
    "teigeodata_wasm.js",
    "teigeodata_wasm_bg.wasm",
    "LICENSE",
    "NOTICE",
];

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let (mut fetch, mut build) = (false, true);
    for a in args {
        match a.as_str() {
            "--fetch" => fetch = true,
            "--no-build" => build = false,
            _ => return Err(vec![USAGE.to_owned()]),
        }
    }
    let data = data(root)?;
    let pack = pinned(root, &data, fetch)?;
    let package = if build {
        package_wasm::build(root, "full")?
    } else {
        root.join("target/wasm/full")
    };
    let version = package_version(&package)?;

    let out = root.join(OUT);
    if out.exists() {
        fs::remove_dir_all(&out).map_err(|e| vec![format!("{}: {e}", out.display())])?;
    }
    let engine = out.join("engine");
    fs::create_dir_all(&engine).map_err(|e| vec![format!("{}: {e}", engine.display())])?;
    let mut bytes = 0;
    for f in ENGINE {
        bytes += copy(&package.join(f), &engine.join(f))?;
    }
    copy(&pack, &out.join(&data.file))?;
    let catalog = catalog(root, &out, fetch, build)?;
    let manifest = serde_json::json!({
        "release": data.release,
        "file": data.file,
        "bytes": data.bytes,
        "sha256": data.sha256,
        "package": "@teispace/teigeodata-wasm",
        "version": version,
        "engine_bytes": bytes,
        "catalog": catalog,
    });
    let mut text =
        serde_json::to_string_pretty(&manifest).map_err(|e| vec![format!("manifest: {e}")])?;
    text.push('\n');
    fs::write(out.join("manifest.json"), text)
        .map_err(|e| vec![format!("{OUT}/manifest.json: {e}")])?;
    Ok(format!(
        "playground: {OUT}/ holds the engine {version} ({bytes} B), {} ({} B, {}) and {} packs \
         to add",
        data.file,
        data.bytes,
        data.release,
        catalog.len()
    ))
}

/// Cuts every pack of the catalog into `out/packs/` and answers the manifest's entries (each
/// with its file, release and licence). Sources are fetched with `fetch`, as the pinned pack;
/// the extracts run the release `tgd`, built first unless `build` is off.
fn catalog(
    root: &Path,
    out: &Path,
    fetch: bool,
    build: bool,
) -> Result<Vec<serde_json::Value>, Vec<String>> {
    let text =
        fs::read_to_string(root.join(CATALOG)).map_err(|e| vec![format!("{CATALOG}: {e}")])?;
    let catalog: Catalog =
        serde_json::from_str(&text).map_err(|e| vec![format!("{CATALOG}: {e}")])?;
    if build {
        let status = Command::new(env!("CARGO"))
            .current_dir(root)
            .args(["build", "--release", "--quiet", "-p", "teigeodata-cli"])
            .status()
            .map_err(|e| vec![format!("cargo build: {e}")])?;
        if !status.success() {
            return Err(vec![format!(
                "cargo build -p teigeodata-cli exited with {status}"
            )]);
        }
    }
    let tgd = root.join("target/release/tgd");
    let dir = out.join("packs");
    fs::create_dir_all(&dir).map_err(|e| vec![format!("{}: {e}", dir.display())])?;
    let mut errors = Vec::new();
    let mut entries = Vec::new();
    for pack in &catalog.packs {
        let Some(source) = catalog.sources.get(&pack.source) else {
            errors.push(format!(
                "{CATALOG}: {} names no source {:?}",
                pack.id, pack.source
            ));
            continue;
        };
        let from = pinned(root, source, fetch)?;
        let file = format!("{}.tgd", pack.id);
        let to = dir.join(&file);
        let output = Command::new(&tgd)
            .arg("extract")
            .arg(&from)
            .arg("-o")
            .arg(&to)
            .args(&pack.extract)
            .output()
            .map_err(|e| {
                vec![format!(
                    "{}: {e} (build it: drop --no-build)",
                    tgd.display()
                )]
            })?;
        if !output.status.success() {
            errors.push(format!(
                "{}: tgd extract exited with {}: {}",
                pack.id,
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
            continue;
        }
        let digest = sha256_file(&to)?;
        let size = fs::metadata(&to)
            .map_err(|e| vec![format!("{}: {e}", to.display())])?
            .len();
        if digest != pack.sha256 || size != pack.bytes {
            errors.push(format!(
                "{}: the extract is {size} B with SHA-256 {digest}; {CATALOG} pins {} B and {}",
                pack.id, pack.bytes, pack.sha256
            ));
            continue;
        }
        let mut entry =
            serde_json::to_value(pack).map_err(|e| vec![format!("{}: {e}", pack.id)])?;
        if let Some(object) = entry.as_object_mut() {
            object.remove("extract");
            object.remove("source");
            object.insert("file".into(), format!("packs/{file}").into());
            object.insert("release".into(), source.release.clone().into());
            object.insert("licence".into(), source.licence.clone().into());
        }
        entries.push(entry);
    }
    if errors.is_empty() {
        Ok(entries)
    } else {
        Err(errors)
    }
}

/// The package's version, from the `package.json` the build wrote.
fn package_version(dir: &Path) -> Result<String, Vec<String>> {
    let path = dir.join("package.json");
    let text = fs::read_to_string(&path).map_err(|e| {
        vec![format!(
            "{}: {e} (build it: drop --no-build, or `cargo xtask package-wasm --variant full`)",
            path.display()
        )]
    })?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| vec![format!("{}: {e}", path.display())])?;
    json.get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| vec![format!("{}: no version", path.display())])
}

fn copy(from: &Path, to: &Path) -> Result<u64, Vec<String>> {
    fs::copy(from, to).map_err(|e| vec![format!("{} -> {}: {e}", from.display(), to.display())])
}
