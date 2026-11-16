//! `bump <version>` and `bump --check`: the release version, everywhere it is written.
//!
//! A release is one version across the Rust workspaces, the bindings and their packages
//! ([releases](../../docs/06-engineering/releases.md#cutting-a-release)). It is written in a
//! dozen files and eight lockfiles; a release with one of them left behind ships a package that
//! names the wrong library (the .NET package checks `Support.cs`, the Go loader downloads by
//! `prebuilt.go`'s version). `bump` rewrites every one of them and refreshes the lockfiles'
//! workspace entries; `--check` fails, by file and line, on any that differs from the main
//! workspace's version, and runs in `check-workspace`.
//!
//! Each place is a line that starts with a fixed text (indent included, so a nested
//! `"version"` is never taken) and holds the version up to a fixed end. Generated files follow
//! from these: `bindings/c/parity_data.h` from `bindings/parity.json` (`cargo xtask bindings`).

use std::fs;
use std::path::Path;
use std::process::Command;

/// A place a version is written: the file, the line's start, where the version starts within
/// the line, the text that ends it ("" for the end of the line), and how many lines hold one.
struct Place {
    file: &'static str,
    start: &'static str,
    key: &'static str,
    end: &'static str,
    count: usize,
}

const fn place(
    file: &'static str,
    start: &'static str,
    key: &'static str,
    end: &'static str,
    count: usize,
) -> Place {
    Place {
        file,
        start,
        key,
        end,
        count,
    }
}

/// Every place the release version is written by hand. The first is the source of truth.
const PLACES: &[Place] = &[
    place("Cargo.toml", "version = \"", "version = \"", "\"", 1),
    place("Cargo.toml", "teigeodata-", "version = \"", "\"", 9),
    place(
        "bindings/Cargo.toml",
        "version = \"",
        "version = \"",
        "\"",
        1,
    ),
    place(
        "bindings/Cargo.toml",
        "teigeodata-",
        "version = \"",
        "\"",
        1,
    ),
    place(
        "forge/crates/forge/Cargo.toml",
        "teigeodata-",
        "version = \"",
        "\"",
        4,
    ),
    place(
        "bindings/parity.json",
        "  \"version\": \"",
        "  \"version\": \"",
        "\"",
        1,
    ),
    place(
        "bindings/node/package.json",
        "  \"version\": \"",
        "  \"version\": \"",
        "\"",
        1,
    ),
    place(
        "bindings/dart/pubspec.yaml",
        "version: ",
        "version: ",
        "",
        1,
    ),
    place(
        "bindings/dart/example/pubspec.yaml",
        "version: ",
        "version: ",
        "",
        1,
    ),
    place(
        "bindings/dotnet/TeiGeodata/Support.cs",
        "    public const string Version = \"",
        "Version = \"",
        "\"",
        1,
    ),
    place(
        "bindings/dotnet/TeiGeodata/TeiGeodata.csproj",
        "    <Version>",
        "<Version>",
        "</Version>",
        1,
    ),
    place(
        "bindings/go/internal/prebuilt/prebuilt.go",
        "const Version = \"",
        "const Version = \"",
        "\"",
        1,
    ),
    place("site/lib/site.ts", "  version: '", "version: '", "'", 1),
];

/// A generated file that must agree too: `cargo xtask bindings` writes it.
const GENERATED: (&str, &str, &str) = ("bindings/c/parity_data.h", "#define TGP_VERSION \"", "\"");

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let usage = || vec!["usage: cargo xtask bump <major.minor.patch> | --check".to_owned()];
    match args {
        [a] if a == "--check" => check(root),
        [v] if valid(v) => bump(root, v),
        _ => Err(usage()),
    }
}

/// Whether `v` is a release version: three numbers, no leading zeros, no pre-release.
fn valid(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        })
}

/// The version within `line` at `p`, with its byte range, when the line is one of `p`'s.
fn found<'a>(line: &'a str, p: &Place) -> Option<(usize, usize, &'a str)> {
    if !line.starts_with(p.start) {
        return None;
    }
    let from = line.find(p.key)? + p.key.len();
    let to = if p.end.is_empty() {
        line.len()
    } else {
        from + line.get(from..)?.find(p.end)?
    };
    Some((from, to, line.get(from..to)?))
}

/// Every version written at `p`: (line number, version).
fn versions(text: &str, p: &Place) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| found(l, p).map(|(_, _, v)| (i + 1, v.trim().to_owned())))
        .collect()
}

/// The main workspace's version: the source of truth.
fn current(root: &Path) -> Result<String, String> {
    let p = PLACES.first().ok_or("no version places")?;
    let text = fs::read_to_string(root.join(p.file)).map_err(|e| format!("{}: {e}", p.file))?;
    match versions(&text, p).as_slice() {
        [(_, v)] => Ok(v.clone()),
        other => Err(format!(
            "{}: expected one `version = \"…\"` line, found {}",
            p.file,
            other.len()
        )),
    }
}

/// Every place, the generated file and every lockfile's workspace packages agree with the main
/// workspace's version.
pub(crate) fn check(root: &Path) -> Result<String, Vec<String>> {
    let want = current(root).map_err(|e| vec![e])?;
    let mut problems = Vec::new();
    let mut seen = 0;
    for p in PLACES {
        let text = match fs::read_to_string(root.join(p.file)) {
            Ok(t) => t,
            Err(e) => {
                problems.push(format!("{}: {e}", p.file));
                continue;
            }
        };
        let found = versions(&text, p);
        if found.len() != p.count {
            problems.push(format!(
                "{}: {} lines start with `{}`, expected {}",
                p.file,
                found.len(),
                p.start,
                p.count
            ));
        }
        for (line, v) in found {
            seen += 1;
            if v != want {
                problems.push(format!(
                    "{}:{line}: version {v}, the workspace's is {want} (run `cargo xtask bump {want}`)",
                    p.file
                ));
            }
        }
    }
    let (file, start, end) = GENERATED;
    match fs::read_to_string(root.join(file)) {
        Ok(text) => {
            let v = text
                .lines()
                .find_map(|l| l.strip_prefix(start)?.split(end).next());
            if v != Some(want.as_str()) {
                problems.push(format!(
                    "{file}: version {}, the workspace's is {want} (run `cargo xtask bindings`)",
                    v.unwrap_or("absent")
                ));
            }
        }
        Err(e) => problems.push(format!("{file}: {e}")),
    }
    for lock in lockfiles(root).map_err(|e| vec![e])? {
        problems.extend(stale_lock(root, &lock, &want));
    }
    if problems.is_empty() {
        Ok(format!(
            "version ok: {want} in {seen} places, the parity data and every lockfile"
        ))
    } else {
        Err(problems)
    }
}

/// The repository's lockfiles (`git ls-files`), each a workspace CI builds with `--locked`.
fn lockfiles(root: &Path) -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "*Cargo.lock"])
        .output()
        .map_err(|e| format!("git ls-files: {e}"))?;
    if !out.status.success() {
        return Err("git ls-files failed".to_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_owned)
        .collect())
}

/// The lockfile's own packages (no `source`, so path dependencies) named `teigeodata-*`
/// whose version is not `want`. A crate at 0.0.0 (the forge, the fuzz targets) is never
/// published and has no release version.
fn stale_lock(root: &Path, lock: &str, want: &str) -> Vec<String> {
    let text = match fs::read_to_string(root.join(lock)) {
        Ok(t) => t,
        Err(e) => return vec![format!("{lock}: {e}")],
    };
    let table: toml::Table = match toml::from_str(&text) {
        Ok(t) => t,
        Err(e) => return vec![format!("{lock}: {e}")],
    };
    let Some(packages) = table.get("package").and_then(toml::Value::as_array) else {
        return Vec::new();
    };
    packages
        .iter()
        .filter_map(toml::Value::as_table)
        .filter(|p| !p.contains_key("source"))
        .filter_map(|p| {
            let name = p.get("name")?.as_str()?;
            let v = p.get("version")?.as_str()?;
            (name.starts_with("teigeodata-") && v != want && v != "0.0.0").then(|| {
                format!(
                    "{lock}: {name} {v}, the workspace's is {want} (run `cargo update -w` there)"
                )
            })
        })
        .collect()
}

/// Rewrites every place to `to`, then refreshes each lockfile's workspace entries.
fn bump(root: &Path, to: &str) -> Result<String, Vec<String>> {
    let from = current(root).map_err(|e| vec![e])?;
    let mut changed = 0;
    let mut problems = Vec::new();
    let mut files: Vec<&str> = PLACES.iter().map(|p| p.file).collect();
    files.dedup();
    for file in files {
        let path = root.join(file);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                problems.push(format!("{file}: {e}"));
                continue;
            }
        };
        let mut out = String::with_capacity(text.len());
        for line in text.split_inclusive('\n') {
            let body = line.trim_end_matches(['\n', '\r']);
            let rewritten = PLACES
                .iter()
                .filter(|p| p.file == file)
                .find_map(|p| found(body, p))
                .and_then(|(a, b, _)| Some(format!("{}{to}{}", body.get(..a)?, body.get(b..)?)));
            match rewritten {
                Some(new) => {
                    changed += usize::from(new != body);
                    out.push_str(&new);
                    out.push_str(line.get(body.len()..).unwrap_or_default());
                }
                None => out.push_str(line),
            }
        }
        if out != text
            && let Err(e) = fs::write(&path, out)
        {
            problems.push(format!("{file}: {e}"));
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let locks = lockfiles(root).map_err(|e| vec![e])?;
    for lock in &locks {
        let dir = Path::new(lock).parent().unwrap_or_else(|| Path::new(""));
        let status = Command::new("cargo")
            .current_dir(root.join(dir))
            .args(["update", "--workspace", "--quiet"])
            .status();
        if !matches!(status, Ok(s) if s.success()) {
            problems.push(format!("{lock}: `cargo update --workspace` failed"));
        }
    }
    // The site's install page shows the version it reads from Cargo.toml: write it again.
    let guides = Command::new("python3")
        .current_dir(root)
        .arg("site/scripts/guides.py")
        .status();
    if !matches!(guides, Ok(s) if s.success()) {
        problems.push(
            "site/scripts/guides.py failed: the install page keeps the old version".to_owned(),
        );
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(format!(
        "bumped {from} to {to}: {changed} lines, {} lockfiles, the install page. Next: `cargo xtask bindings` \
         (the parity data), the CHANGELOG's section, then `cargo xtask bump --check`",
        locks.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::{PLACES, found, valid, versions};

    #[test]
    fn a_release_version_is_three_plain_numbers() {
        for v in ["1.0.0", "0.3.1", "10.20.30"] {
            assert!(valid(v), "{v}");
        }
        for v in [
            "1.0",
            "1.0.0-rc.1",
            "01.0.0",
            "1.0.0.0",
            "v1.0.0",
            "1..0",
            "",
        ] {
            assert!(!valid(v), "{v}");
        }
    }

    #[test]
    fn each_place_takes_only_its_own_lines() {
        let json = PLACES
            .iter()
            .find(|p| p.file == "bindings/parity.json")
            .unwrap();
        let text =
            "{\n  \"version\": \"0.3.1\",\n  \"tz\": {\n    \"version\": \"2026f\"\n  }\n}\n";
        assert_eq!(versions(text, json), vec![(2, "0.3.1".to_owned())]);
        let dep = PLACES
            .iter()
            .find(|p| p.file == "forge/crates/forge/Cargo.toml")
            .unwrap();
        let line =
            r#"teigeodata-core = { version = "0.3.1", path = "../core", features = ["alloc"] }"#;
        let (a, b, v) = found(line, dep).unwrap();
        assert_eq!(v, "0.3.1");
        assert_eq!(&line[..a], r#"teigeodata-core = { version = ""#);
        assert!(line[b..].starts_with(r#"", path"#));
        assert!(found(r#"serde = { version = "1.0.0" }"#, dep).is_none());
        let yaml = PLACES
            .iter()
            .find(|p| p.file == "bindings/dart/pubspec.yaml")
            .unwrap();
        assert_eq!(
            versions("name: x\nversion: 0.3.1\n  version: 9\n", yaml).len(),
            1
        );
    }
}
