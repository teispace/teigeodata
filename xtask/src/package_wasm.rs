//! `cargo xtask package-wasm`: the WebAssembly npm packages (5.7, D-293). Each variant is a
//! set of pack components (D-291) built from `bindings/wasm`: wasm-bindgen `--target web`,
//! then `wasm-opt -Oz`, with the hand-written entries and a `package.json`, under
//! `target/wasm/<variant>/`. Its gzipped module must fit the variant's budget. `--test` runs
//! `bindings/wasm/test/parity.mjs` from the package on each of Node.js, Bun and Deno found.

use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::package_c::{command, version};

const USAGE: &str = "usage: cargo xtask package-wasm [--variant full|lookup] [--test]";

/// The wasm-bindgen version `bindings/wasm/Cargo.toml` pins; the CLI must match it.
const WASM_BINDGEN: &str = "0.2.130";

/// A published variant: its package name, the components it is built with, and its budget
/// for the module gzipped at level 9.
struct Variant {
    name: &'static str,
    package: &'static str,
    description: &'static str,
    /// `None` is every component (the crate's default features).
    features: Option<&'static str>,
    budget_gz: u64,
}

const VARIANTS: &[Variant] = &[
    Variant {
        name: "full",
        package: "@teispace/teigeodata-wasm",
        description: "Accurate, offline places in WebAssembly: every query, search included.",
        features: None,
        budget_gz: 262_144,
    },
    Variant {
        name: "lookup",
        package: "@teispace/teigeodata-wasm-lookup",
        description: "Accurate, offline places in WebAssembly: every query at a point or by ID, \
                      without search.",
        features: Some(
            "names,admin,elevation,timezone,timezone-index,boundaries,admin-index,dem,\
             dem-coarse,tz-rules,postal",
        ),
        budget_gz: 153_600,
    },
];

/// The files a package carries besides the module: wasm-bindgen's glue and the entries.
const ENTRIES: [&str; 5] = [
    "index.js",
    "index.d.ts",
    "node.js",
    "workerd.js",
    "reader.js",
];

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let (mut only, mut test) = (None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--variant" => only = Some(it.next().ok_or_else(|| vec![USAGE.to_owned()])?.clone()),
            "--test" => test = true,
            _ => return Err(vec![USAGE.to_owned()]),
        }
    }
    let variants: Vec<&Variant> = VARIANTS
        .iter()
        .filter(|v| only.as_deref().is_none_or(|o| o == v.name))
        .collect();
    if variants.is_empty() {
        return Err(vec![USAGE.to_owned()]);
    }
    tool_version("wasm-bindgen", WASM_BINDGEN)?;
    tool_version("wasm-opt", "")?;
    let version = version(root)?;
    let mut report = String::new();
    for v in variants {
        let dir = package(root, v, &version)?;
        let wasm = fs::read(dir.join("teigeodata_wasm_bg.wasm"))
            .map_err(|e| vec![format!("{}: {e}", dir.display())])?;
        let gz = gzip(&wasm)?;
        let line = format!(
            "{}: {} B, {gz} B gzipped (budget {}), {}",
            v.package,
            wasm.len(),
            v.budget_gz,
            dir.display()
        );
        if gz > v.budget_gz {
            return Err(vec![format!("{line}: over its budget")]);
        }
        report.push_str(&line);
        report.push('\n');
        if test {
            report.push_str(&parity(root, &dir)?);
        }
    }
    Ok(report.trim_end().to_owned())
}

/// Builds `v` and assembles its package; returns its folder.
fn package(root: &Path, v: &Variant, version: &str) -> Result<PathBuf, Vec<String>> {
    let bindings = root.join("bindings");
    let mut args = vec![
        "build",
        "--locked",
        "--profile",
        "wasm",
        "--target",
        "wasm32-unknown-unknown",
        "-p",
        "teigeodata-wasm",
    ];
    if let Some(f) = v.features {
        args.extend(["--no-default-features", "--features", f]);
    }
    command(&bindings, "cargo", &args)?;
    let built = bindings.join("target/wasm32-unknown-unknown/wasm/teigeodata_wasm.wasm");
    let dir = root.join("target/wasm").join(v.name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| vec![format!("{}: {e}", dir.display())])?;
    let out = dir.display().to_string();
    command(
        root,
        "wasm-bindgen",
        &[
            "--target",
            "web",
            "--out-dir",
            &out,
            "--out-name",
            "teigeodata_wasm",
            "--remove-name-section",
            "--remove-producers-section",
            &built.display().to_string(),
        ],
    )?;
    let module = dir.join("teigeodata_wasm_bg.wasm").display().to_string();
    command(root, "wasm-opt", &["-Oz", &module, "-o", &module])?;
    let js = bindings.join("wasm/js");
    for f in ENTRIES {
        copy(&js.join(f), &dir.join(f))?;
    }
    for f in ["LICENSE", "NOTICE"] {
        copy(&root.join(f), &dir.join(f))?;
    }
    let manifest = package_json(v, version);
    fs::write(dir.join("package.json"), manifest)
        .map_err(|e| vec![format!("{}: {e}", dir.display())])?;
    fs::write(dir.join("README.md"), readme(v))
        .map_err(|e| vec![format!("{}: {e}", dir.display())])?;
    Ok(dir)
}

/// The package's `package.json`: ES modules, an entry per host by export condition.
fn package_json(v: &Variant, version: &str) -> String {
    let manifest = serde_json::json!({
        "name": v.package,
        "version": version,
        "description": v.description,
        "license": "Apache-2.0",
        "repository": {
            "type": "git",
            "url": "git+https://github.com/teispace/teigeodata.git",
            "directory": "bindings/wasm",
        },
        "homepage": "https://github.com/teispace/teigeodata",
        "keywords": ["geodata", "gazetteer", "geocoding", "offline", "timezone", "webassembly"],
        "type": "module",
        "main": "./node.js",
        "types": "./index.d.ts",
        "sideEffects": ["./node.js", "./workerd.js"],
        "files": [
            "index.js", "index.d.ts", "node.js", "workerd.js", "reader.js", "teigeodata_wasm.js",
            "teigeodata_wasm.d.ts", "teigeodata_wasm_bg.wasm", "teigeodata_wasm_bg.wasm.d.ts",
            "LICENSE", "NOTICE", "README.md",
        ],
        "engines": { "node": ">=22" },
    });
    crate::package_node::Manifest {
        fields: &manifest,
        exports: Exports {
            main: Conditions {
                types: "./index.d.ts",
                workerd: "./workerd.js",
                node: "./node.js",
                default: "./index.js",
            },
            wasm: "./teigeodata_wasm_bg.wasm",
            package: "./package.json",
        },
    }
    .text()
}

/// `package.json`'s `exports`, in the order hosts match them (a `json!` map sorts its keys).
#[derive(serde::Serialize)]
struct Exports {
    #[serde(rename = ".")]
    main: Conditions,
    #[serde(rename = "./teigeodata_wasm_bg.wasm")]
    wasm: &'static str,
    #[serde(rename = "./package.json")]
    package: &'static str,
}

/// Node.js and bundlers take the first condition that applies: `default` last, `types` first.
#[derive(serde::Serialize)]
struct Conditions {
    types: &'static str,
    workerd: &'static str,
    node: &'static str,
    default: &'static str,
}

fn readme(v: &Variant) -> String {
    format!(
        "# {}\n\n{}\n\nThe components it answers: `components()`. A query that needs one it was \
         built without throws an `Error` with `code` `MissingComponent`.\n\n```js\nimport {{ \
         init, Engine }} from \"{}\";\n\nawait init(); // loads the module (at once in Node.js, \
         Bun and Workers)\nconst engine = new Engine();\nengine.openBytes(new Uint8Array(await \
         (await fetch(\"np.tgd\")).arrayBuffer()));\nconst places = engine.reverse(27.7, 85.3, \
         5); // a columnar blob (Uint8Array)\n```\n\nDocumentation: \
         https://github.com/teispace/teigeodata\n",
        v.package, v.description, v.package
    )
}

/// Runs the parity test from `dir` on each JavaScript host found; at least Node.js must run.
fn parity(root: &Path, dir: &Path) -> Result<String, Vec<String>> {
    let test = root
        .join("bindings/wasm/test/parity.mjs")
        .display()
        .to_string();
    let parity = root.join("bindings/parity.json").display().to_string();
    let pkg = dir.display().to_string();
    let mut out = String::new();
    let hosts: [(&str, Vec<&str>); 3] = [
        ("node", vec![&test, &pkg, &parity]),
        ("bun", vec![&test, &pkg, &parity]),
        ("deno", vec!["run", "--allow-read", &test, &pkg, &parity]),
    ];
    for (host, args) in hosts {
        if Command::new(host).arg("--version").output().is_err() {
            if host == "node" {
                return Err(vec!["node: not found".into()]);
            }
            let _ = writeln!(out, "  {host}: not installed, not run");
            continue;
        }
        command(root, host, &args)?;
        let _ = writeln!(out, "  {host}: parity ok");
    }
    Ok(out)
}

fn copy(from: &Path, to: &Path) -> Result<(), Vec<String>> {
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| vec![format!("{}: {e}", from.display())])
}

/// Fails unless `tool` runs, and when `want` is not empty, reports that version.
fn tool_version(tool: &str, want: &str) -> Result<(), Vec<String>> {
    let out = Command::new(tool).arg("--version").output().map_err(|e| {
        vec![format!(
            "{tool}: {e} (install it: see bindings/wasm/README.md)"
        )]
    })?;
    let text = String::from_utf8_lossy(&out.stdout);
    if !want.is_empty() && !text.split_whitespace().any(|w| w == want) {
        return Err(vec![format!(
            "{tool} is {}, the crate needs {want}: cargo install wasm-bindgen-cli --version \
             {want} --locked",
            text.trim()
        )]);
    }
    Ok(())
}

fn gzip(bytes: &[u8]) -> Result<u64, Vec<String>> {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(bytes)
        .and_then(|()| gz.finish())
        .map(|v| v.len() as u64)
        .map_err(|e| vec![format!("gzip: {e}")])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_has_its_own_package() {
        let mut names: Vec<_> = VARIANTS.iter().map(|v| v.package).collect();
        names.dedup();
        assert_eq!(names.len(), VARIANTS.len());
        assert!(
            VARIANTS
                .iter()
                .all(|v| v.package.starts_with("@teispace/teigeodata-wasm"))
        );
    }

    #[test]
    fn the_manifest_names_an_entry_per_host() {
        let m: serde_json::Value =
            serde_json::from_str(&package_json(&VARIANTS[1], "1.2.3")).unwrap();
        assert_eq!(m["name"], "@teispace/teigeodata-wasm-lookup");
        assert_eq!(m["version"], "1.2.3");
        let e = &m["exports"]["."];
        assert_eq!(e["workerd"], "./workerd.js");
        assert_eq!(e["node"], "./node.js");
        assert_eq!(e["default"], "./index.js");
    }

    #[test]
    fn the_conditions_are_written_in_the_order_hosts_match_them() {
        // Node.js and bundlers take the first condition that applies: `default` must come
        // last, or Node.js loads the browser entry and `init()` fetches (found by the docs'
        // WebAssembly tour); TypeScript wants `types` first.
        let text = package_json(&VARIANTS[0], "1.2.3");
        let at = |key: &str| text.find(&format!("\"{key}\": \"./")).unwrap();
        assert!(at("types") < at("workerd") && at("workerd") < at("node"));
        assert!(at("node") < at("default"));
    }
}
