//! `cargo xtask package-node`: the Node.js packages (5.7c, D-294), in the napi-rs layout. The
//! main package `teigeodata` holds the generated entries and the loader and names a
//! platform package per platform in `optionalDependencies`; each platform package,
//! `@teispace/teigeodata-<suffix>`, holds one prebuilt addon and the `os`, `cpu` and `libc`
//! fields npm installs it by. They are assembled under `target/node/<folder>/`.
//!
//! With `--addons <dir>` (the release), `<dir>` must hold `teigeodata.<suffix>.node` for every
//! platform. Without it, this machine's addon is built and only its platform is packaged.
//! `--test` installs the packages as npm would and runs the parity test
//! (`bindings/node/test/install.mjs`).

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::package_c::{command, version};

const USAGE: &str = "usage: cargo xtask package-node [--addons <dir>] [--test]";

const MAIN: &str = "teigeodata";
/// The per-platform packages, `@teispace/teigeodata-<suffix>`: scoped so only Teispace can
/// publish one (D-358), installed by `MAIN` and never named by a user.
const PLATFORM: &str = "@teispace/teigeodata";

/// A platform the addon is built for: its package suffix (napi-rs names) and npm's fields.
struct Platform {
    suffix: &'static str,
    os: &'static str,
    cpu: &'static str,
    libc: Option<&'static str>,
}

const PLATFORMS: &[Platform] = &[
    Platform {
        suffix: "darwin-arm64",
        os: "darwin",
        cpu: "arm64",
        libc: None,
    },
    Platform {
        suffix: "darwin-x64",
        os: "darwin",
        cpu: "x64",
        libc: None,
    },
    Platform {
        suffix: "linux-arm64-gnu",
        os: "linux",
        cpu: "arm64",
        libc: Some("glibc"),
    },
    Platform {
        suffix: "linux-arm64-musl",
        os: "linux",
        cpu: "arm64",
        libc: Some("musl"),
    },
    Platform {
        suffix: "linux-x64-gnu",
        os: "linux",
        cpu: "x64",
        libc: Some("glibc"),
    },
    Platform {
        suffix: "linux-x64-musl",
        os: "linux",
        cpu: "x64",
        libc: Some("musl"),
    },
    Platform {
        suffix: "win32-arm64-msvc",
        os: "win32",
        cpu: "arm64",
        libc: None,
    },
    Platform {
        suffix: "win32-x64-msvc",
        os: "win32",
        cpu: "x64",
        libc: None,
    },
];

/// The main package's files from `bindings/node` (generated entries, the loader).
const MAIN_FILES: [&str; 6] = [
    "index.js",
    "index.mjs",
    "index.d.ts",
    "native.js",
    "reader.js",
    "target.js",
];

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let (mut addons, mut test) = (None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--addons" => {
                addons = Some(PathBuf::from(
                    it.next().ok_or_else(|| vec![USAGE.to_owned()])?,
                ));
            }
            "--test" => test = true,
            _ => return Err(vec![USAGE.to_owned()]),
        }
    }
    let version = version(root)?;
    let out = root.join("target/node");
    let _ = fs::remove_dir_all(&out);
    let built: Vec<(&Platform, PathBuf)> = match &addons {
        Some(dir) => PLATFORMS
            .iter()
            .map(|p| (p, dir.join(format!("teigeodata.{}.node", p.suffix))))
            .collect(),
        None => vec![host_addon(root)?],
    };
    let missing: Vec<String> = built
        .iter()
        .filter(|(_, f)| !f.is_file())
        .map(|(_, f)| format!("{}: missing", f.display()))
        .collect();
    if !missing.is_empty() {
        return Err(missing);
    }
    let mut report = String::new();
    for (p, addon) in &built {
        let dir = out.join(p.suffix);
        mkdir(&dir)?;
        let file = format!("teigeodata.{}.node", p.suffix);
        copy(addon, &dir.join(&file))?;
        licence(root, &dir)?;
        write(&dir.join("package.json"), &platform_json(p, &version))?;
        write(&dir.join("README.md"), &platform_readme(p))?;
        let size = fs::metadata(dir.join(&file)).map_or(0, |m| m.len());
        let _ = writeln!(
            report,
            "{PLATFORM}-{}: {size} B, {}",
            p.suffix,
            dir.display()
        );
    }
    let main = out.join("teigeodata");
    mkdir(&main)?;
    let node = root.join("bindings/node");
    for f in MAIN_FILES {
        copy(&node.join(f), &main.join(f))?;
    }
    licence(root, &main)?;
    write(&main.join("package.json"), &main_json(&version))?;
    write(&main.join("README.md"), &main_readme())?;
    let _ = writeln!(report, "{MAIN}: {}", main.display());
    if test {
        let script = node.join("test/install.mjs").display().to_string();
        command(root, "node", &[&script, &out.display().to_string()])?;
        report.push_str("  installed as npm would; parity ok\n");
    }
    Ok(report.trim_end().to_owned())
}

/// Builds this machine's addon (the `dist` profile); its platform and file.
fn host_addon(root: &Path) -> Result<(&'static Platform, PathBuf), Vec<String>> {
    let (os, cpu) = (
        match std::env::consts::OS {
            "macos" => "darwin",
            "windows" => "win32",
            other => other,
        },
        match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "x64",
            other => other,
        },
    );
    let libc = (os == "linux").then_some(if cfg!(target_env = "musl") {
        "musl"
    } else {
        "glibc"
    });
    let platform = PLATFORMS
        .iter()
        .find(|p| p.os == os && p.cpu == cpu && p.libc == libc)
        .ok_or_else(|| vec![format!("no Node.js package is built for {os}-{cpu}")])?;
    let bindings = root.join("bindings");
    command(
        &bindings,
        "cargo",
        &[
            "build",
            "--locked",
            "--profile",
            "dist",
            "-p",
            "teigeodata-node",
        ],
    )?;
    let file = match os {
        "darwin" => "libteigeodata_node.dylib",
        "win32" => "teigeodata_node.dll",
        _ => "libteigeodata_node.so",
    };
    Ok((platform, bindings.join("target/dist").join(file)))
}

fn main_json(version: &str) -> String {
    let optional: serde_json::Map<String, serde_json::Value> = PLATFORMS
        .iter()
        .map(|p| (format!("{PLATFORM}-{}", p.suffix), version.into()))
        .collect();
    let mut files: Vec<&str> = MAIN_FILES.to_vec();
    files.extend(["LICENSE", "NOTICE", "README.md"]);
    let manifest = serde_json::json!({
        "name": MAIN,
        "version": version,
        "description": "Accurate, offline places for Node.js: the teigeodata addon, prebuilt \
                        for every major platform.",
        "license": "Apache-2.0",
        "repository": {
            "type": "git",
            "url": "git+https://github.com/teispace/teigeodata.git",
            "directory": "bindings/node",
        },
        "homepage": "https://github.com/teispace/teigeodata",
        "keywords": ["geodata", "gazetteer", "geocoding", "offline", "timezone", "napi"],
        "main": "./index.js",
        "types": "./index.d.ts",
        "files": files,
        "engines": { "node": ">=22" },
        "optionalDependencies": optional,
    });
    Manifest {
        fields: &manifest,
        exports: MainExports {
            main: MainConditions {
                types: "./index.d.ts",
                import: "./index.mjs",
                require: "./index.js",
            },
            package: "./package.json",
        },
    }
    .text()
}

fn platform_json(p: &Platform, version: &str) -> String {
    let file = format!("teigeodata.{}.node", p.suffix);
    let mut manifest = serde_json::json!({
        "name": format!("{PLATFORM}-{}", p.suffix),
        "version": version,
        "description": format!("The teigeodata Node.js addon for {}; installed by {MAIN}.", p.suffix),
        "license": "Apache-2.0",
        "repository": {
            "type": "git",
            "url": "git+https://github.com/teispace/teigeodata.git",
            "directory": "bindings/node",
        },
        "main": file,
        "files": [file, "LICENSE", "NOTICE", "README.md"],
        "os": [p.os],
        "cpu": [p.cpu],
        "engines": { "node": ">=22" },
    });
    if let (Some(libc), Some(m)) = (p.libc, manifest.as_object_mut()) {
        m.insert("libc".into(), serde_json::json!([libc]));
    }
    pretty(&manifest)
}

fn main_readme() -> String {
    format!(
        "# {MAIN}\n\nAccurate, offline places for Node.js (22 and later), Bun and Deno: the \
         teigeodata addon, prebuilt for macOS, Linux (glibc and musl) and Windows on x64 and \
         arm64. npm installs only the platform package this machine needs.\n\n```js\nimport {{ \
         Engine }} from \"{MAIN}\";\n\nconst engine = new Engine();\nengine.openFile(\"np.tgd\");\n\
         const places = engine.reverse(27.7, 85.3, 5); // a columnar blob (Buffer)\n```\n\nOn \
         another platform, or in a browser or a Worker, use `teigeodata-wasm`.\n\n\
         Documentation: https://github.com/teispace/teigeodata\n"
    )
}

fn platform_readme(p: &Platform) -> String {
    format!(
        "# {PLATFORM}-{}\n\nThe prebuilt teigeodata addon for {}. Install `{MAIN}`, which loads \
         it.\n",
        p.suffix, p.suffix
    )
}

/// `package.json`'s `exports` of the main package. A struct, not a `json!` map: hosts match
/// conditions in the order written, and a `serde_json` map sorts its keys.
#[derive(serde::Serialize)]
struct MainExports {
    #[serde(rename = ".")]
    main: MainConditions,
    #[serde(rename = "./package.json")]
    package: &'static str,
}

/// TypeScript takes `types` first.
#[derive(serde::Serialize)]
struct MainConditions {
    types: &'static str,
    import: &'static str,
    require: &'static str,
}

/// A manifest: `fields`, then `exports` in its own order.
#[derive(serde::Serialize)]
pub(crate) struct Manifest<'a, E: serde::Serialize> {
    #[serde(flatten)]
    pub(crate) fields: &'a serde_json::Value,
    pub(crate) exports: E,
}

impl<E: serde::Serialize> Manifest<'_, E> {
    /// The manifest as `package.json` text.
    pub(crate) fn text(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_default();
        text.push('\n');
        text
    }
}

fn pretty(v: &serde_json::Value) -> String {
    let mut text = serde_json::to_string_pretty(v).unwrap_or_default();
    text.push('\n');
    text
}

fn licence(root: &Path, dir: &Path) -> Result<(), Vec<String>> {
    for f in ["LICENSE", "NOTICE"] {
        copy(&root.join(f), &dir.join(f))?;
    }
    Ok(())
}

fn mkdir(dir: &Path) -> Result<(), Vec<String>> {
    fs::create_dir_all(dir).map_err(|e| vec![format!("{}: {e}", dir.display())])
}

fn copy(from: &Path, to: &Path) -> Result<(), Vec<String>> {
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| vec![format!("{}: {e}", from.display())])
}

fn write(path: &Path, text: &str) -> Result<(), Vec<String>> {
    fs::write(path, text).map_err(|e| vec![format!("{}: {e}", path.display())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_platform_has_its_own_package_and_fields() {
        let mut suffixes: Vec<_> = PLATFORMS.iter().map(|p| p.suffix).collect();
        suffixes.sort_unstable();
        suffixes.dedup();
        assert_eq!(suffixes.len(), PLATFORMS.len());
        for p in PLATFORMS {
            assert!(p.suffix.starts_with(&format!("{}-{}", p.os, p.cpu)));
            assert_eq!(p.os == "linux", p.libc.is_some());
        }
    }

    #[test]
    fn the_main_package_names_every_platform_at_its_version() {
        let m: serde_json::Value = serde_json::from_str(&main_json("1.2.3")).unwrap();
        let optional = m["optionalDependencies"].as_object().unwrap();
        assert_eq!(optional.len(), PLATFORMS.len());
        assert!(optional.values().all(|v| v == "1.2.3"));
        assert_eq!(m["exports"]["."]["import"], "./index.mjs");
        // Conditions match in order: TypeScript wants `types` first.
        let text = main_json("1.2.3");
        let at = |key: &str| text.find(&format!("\"{key}\": \"./")).unwrap();
        assert!(at("types") < at("import") && at("import") < at("require"));
    }

    #[test]
    fn a_linux_package_names_its_c_library() {
        let musl = PLATFORMS
            .iter()
            .find(|p| p.suffix == "linux-x64-musl")
            .unwrap();
        let m: serde_json::Value = serde_json::from_str(&platform_json(musl, "1.2.3")).unwrap();
        assert_eq!(m["libc"][0], "musl");
        assert_eq!(m["main"], "teigeodata.linux-x64-musl.node");
        let mac = PLATFORMS
            .iter()
            .find(|p| p.suffix == "darwin-arm64")
            .unwrap();
        let m: serde_json::Value = serde_json::from_str(&platform_json(mac, "1.2.3")).unwrap();
        assert!(m.get("libc").is_none());
    }
}
