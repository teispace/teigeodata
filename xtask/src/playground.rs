//! `cargo xtask playground [--fetch] [--no-build]`: writes what the site's playground loads to
//! `site/public/playground/` (git-ignored; phase 7.6,
//! [research](../../docs/02-research/notes/docs-playground.md)): the full WebAssembly package
//! built from this commit, the pinned pack checked by SHA-256, and a `manifest.json` that names
//! them. The page loads these files from its own origin at run time, so it runs exactly the files
//! a user installs; every query runs in the page (only the map's tiles come from OpenStreetMap).

use std::fs;
use std::path::Path;

use crate::docs_examples::{data, pinned};
use crate::package_wasm;

const USAGE: &str = "usage: cargo xtask playground [--fetch] [--no-build]";

/// Where the files go, relative to the repository root.
const OUT: &str = "site/public/playground";

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
    let manifest = serde_json::json!({
        "release": data.release,
        "file": data.file,
        "bytes": data.bytes,
        "sha256": data.sha256,
        "package": "@teispace/teigeodata-wasm",
        "version": version,
        "engine_bytes": bytes,
    });
    let mut text =
        serde_json::to_string_pretty(&manifest).map_err(|e| vec![format!("manifest: {e}")])?;
    text.push('\n');
    fs::write(out.join("manifest.json"), text)
        .map_err(|e| vec![format!("{OUT}/manifest.json: {e}")])?;
    Ok(format!(
        "playground: {OUT}/ holds the engine {version} ({bytes} B) and {} ({} B, {})",
        data.file, data.bytes, data.release
    ))
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
