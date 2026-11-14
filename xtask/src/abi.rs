//! The C ABI's gates (phase 5.2, D-285).
//!
//! - **Compatibility:** the declarations of `bindings/c/teigeodata.h` against those of the last
//!   release's header. The header is generated from the IDL and cross-checked against the
//!   compiled crate by cbindgen (`cargo xtask bindings --check`), and every struct is
//!   `repr(C)` with fixed-width fields, so a change in a declaration is a change in the ABI on
//!   every target. A removed or changed declaration is a break; the report names the lowest
//!   version the next release may take, and `--release` fails when the workspace's version is
//!   lower.
//! - **Symbols:** a built shared library exports exactly the header's functions (without the
//!   store's under `--lite`), plus the few third-party symbols listed in [`ALLOWED`], each with
//!   its reason.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use crate::bindings::cheader::declarations;

/// Symbols a dependency exports from the library that cannot be hidden, and why each is safe.
const ALLOWED: &[(&str, &str)] = &[(
    "blake3_compress_in_place_portable",
    "blake3's NEON build (aarch64) defines it `#[no_mangle]` for its C code; rustc exports every \
     `no_mangle` function of a cdylib and neither ld64 nor GNU ld can hide it next to rustc's \
     export list. The BLAKE3 C library defines the same function with the same signature and \
     result, so interposition is harmless",
)];

const HEADER: &str = "bindings/c/teigeodata.h";

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let current =
        std::fs::read_to_string(root.join(HEADER)).map_err(|e| vec![format!("{HEADER}: {e}")])?;
    let mut summary = Vec::new();

    let tag = match value("--since") {
        Some(tag) => tag,
        None => last_release(root)?,
    };
    let base = git(root, &["show", &format!("{tag}:{HEADER}")])?;
    let diff = compare(&declarations(&base), &declarations(&current));
    let base_version = parse_version(tag.trim_start_matches('v'))
        .ok_or_else(|| vec![format!("tag `{tag}` is not a version")])?;
    let version = workspace_version(root)?;
    let minimum = diff.minimum(base_version);
    for line in &diff.breaks {
        summary.push(format!("  break: {line}"));
    }
    summary.push(format!(
        "ABI since {tag}: {} break(s), {} addition(s); the next release must be at least {}",
        diff.breaks.len(),
        diff.added.len(),
        show(minimum)
    ));
    if has("--release") && version < minimum {
        let mut problems: Vec<String> = diff.breaks.iter().map(|b| format!("break: {b}")).collect();
        problems.push(format!(
            "the workspace's version {} is below {} for these changes since {tag}",
            show(version),
            show(minimum)
        ));
        return Err(problems);
    }

    if let Some(lib) = value("--lib") {
        let exported = exported(Path::new(&lib))?;
        let mut expected = functions(&declarations(&current));
        if has("--lite") {
            for name in store_functions(root)? {
                expected.remove(&name);
            }
        }
        symbols(&exported, &expected)?;
        summary.push(format!(
            "{lib}: exports the header's {} functions{} and nothing else",
            expected.len(),
            if has("--lite") { " (lite)" } else { "" }
        ));
    }
    Ok(summary.join("\n"))
}

/// The declarations removed or changed (breaks) and added since a base header.
#[derive(Debug, Default, PartialEq)]
struct Diff {
    breaks: Vec<String>,
    added: Vec<String>,
}

impl Diff {
    /// The lowest version the next release may take after `base`: a break needs the next
    /// breaking version (the minor before 1.0, as Cargo reads it), anything else the next patch.
    fn minimum(&self, base: (u64, u64, u64)) -> (u64, u64, u64) {
        match (self.breaks.is_empty(), base) {
            (false, (0, minor, _)) => (0, minor + 1, 0),
            (false, (major, _, _)) => (major + 1, 0, 0),
            (true, (major, minor, patch)) => (major, minor, patch + 1),
        }
    }
}

fn compare(base: &BTreeMap<String, String>, now: &BTreeMap<String, String>) -> Diff {
    let mut diff = Diff::default();
    for (name, old) in base {
        match now.get(name) {
            Some(new) if new == old => {}
            Some(new) => diff.breaks.push(format!("`{old}` is now `{new}`")),
            None => diff.breaks.push(format!("`{old}` was removed")),
        }
    }
    for name in now.keys() {
        if !base.contains_key(name) {
            diff.added.push(name.clone());
        }
    }
    diff
}

/// The header's function names (not its callback typedefs, structs or constants).
fn functions(decls: &BTreeMap<String, String>) -> BTreeSet<String> {
    decls
        .iter()
        .filter(|(name, decl)| name.starts_with("tg_") && !decl.contains(&format!("(*{name})")))
        .map(|(name, _)| name.clone())
        .collect()
}

/// The C names of the functions behind a Cargo feature, from the IDL.
fn store_functions(root: &Path) -> Result<Vec<String>, Vec<String>> {
    let path = root.join("bindings/idl.json");
    let text =
        std::fs::read_to_string(&path).map_err(|e| vec![format!("{}: {e}", path.display())])?;
    let idl: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| vec![format!("idl.json: {e}")])?;
    let text_of = |v: &serde_json::Value, key: &str| {
        v.get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let list = |v: &serde_json::Value, key: &str| {
        v.get(key)
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let gated = |v: &serde_json::Value| v.get("feature").is_some_and(|f| !f.is_null());
    let mut out = Vec::new();
    for f in list(&idl, "functions").iter().filter(|f| gated(f)) {
        out.push(format!("tg_{}", text_of(f, "name")));
    }
    for h in list(&idl, "handles").iter().filter(|h| gated(h)) {
        let snake = text_of(h, "snake");
        out.push(format!("tg_{snake}_free"));
        for m in list(h, "methods") {
            out.push(format!("tg_{snake}_{}", text_of(&m, "name")));
        }
    }
    Ok(out)
}

fn symbols(exported: &BTreeSet<String>, expected: &BTreeSet<String>) -> Result<(), Vec<String>> {
    let allowed: BTreeSet<&str> = ALLOWED.iter().map(|(name, _)| *name).collect();
    let mut problems = Vec::new();
    for name in expected.difference(exported) {
        problems.push(format!(
            "the header declares `{name}`, which the library does not export"
        ));
    }
    for name in exported.difference(expected) {
        if !allowed.contains(name.as_str()) {
            problems.push(format!(
                "the library exports `{name}`, which the header does not declare"
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The global symbols a shared library defines, without the platform's `_` prefix.
fn exported(lib: &Path) -> Result<BTreeSet<String>, Vec<String>> {
    let macho = lib.extension().is_some_and(|e| e == "dylib");
    let mut cmd = Command::new("nm");
    if macho {
        cmd.arg("-gU");
    } else {
        cmd.args(["-D", "--defined-only"]);
    }
    let out = cmd
        .arg(lib)
        .output()
        .map_err(|e| vec![format!("nm: {e}")])?;
    if !out.status.success() {
        return Err(vec![format!(
            "nm {}: {}",
            lib.display(),
            String::from_utf8_lossy(&out.stderr)
        )]);
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace().rev();
            let name = words.next()?;
            let kind = words.next()?;
            // Defined text and data; weak (`W`, `V`) and absolute (`A`) symbols are the
            // toolchain's, not ours.
            matches!(kind, "T" | "D" | "B" | "R" | "S").then(|| {
                let name = if macho {
                    name.strip_prefix('_').unwrap_or(name)
                } else {
                    name
                };
                name.to_owned()
            })
        })
        .collect())
}

fn last_release(root: &Path) -> Result<String, Vec<String>> {
    let tag = git(
        root,
        &["describe", "--tags", "--abbrev=0", "--match", "v[0-9]*"],
    )
    .map_err(|mut e| {
        e.push("no release tag: fetch the tags (`git fetch --tags`) or pass --since".into());
        e
    })?;
    Ok(tag.trim().to_owned())
}

fn git(root: &Path, args: &[&str]) -> Result<String, Vec<String>> {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|e| vec![format!("git: {e}")])?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(vec![format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )])
    }
}

fn workspace_version(root: &Path) -> Result<(u64, u64, u64), Vec<String>> {
    let text = std::fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|e| vec![format!("Cargo.toml: {e}")])?;
    text.lines()
        .find_map(|l| l.strip_prefix("version = \""))
        .and_then(|v| parse_version(v.trim_end_matches('"')))
        .ok_or_else(|| vec!["Cargo.toml: no workspace version".into()])
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(str::parse::<u64>);
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(Ok(a)), Some(Ok(b)), Some(Ok(c)), None) => Some((a, b, c)),
        _ => None,
    }
}

fn show((a, b, c): (u64, u64, u64)) -> String {
    format!("{a}.{b}.{c}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "
#define TG_OK 0
#define TG_ERROR_IO 2
typedef struct tg_bytes { const uint8_t *data; size_t len; } tg_bytes;
typedef struct tg_engine tg_engine;
typedef int32_t (*tg_callback_progress)(void *ctx, uint64_t done);
int32_t tg_engine_new(tg_engine **out);
int32_t tg_engine_get(const tg_engine *engine, uint64_t id, tg_bytes *out);
";

    fn diff(now: &str) -> Diff {
        compare(&declarations(BASE), &declarations(now))
    }

    #[test]
    fn the_same_header_has_no_break() {
        assert_eq!(diff(BASE), Diff::default());
        assert_eq!(diff(BASE).minimum((0, 3, 1)), (0, 3, 2));
    }

    #[test]
    fn an_added_function_is_compatible() {
        let now =
            format!("{BASE}int32_t tg_engine_layers(const tg_engine *engine, tg_bytes *out);\n");
        let d = diff(&now);
        assert_eq!(d.breaks.len(), 0);
        assert_eq!(d.added, ["tg_engine_layers"]);
    }

    #[test]
    fn every_kind_of_change_is_a_break() {
        for now in [
            // A function removed.
            BASE.replace("int32_t tg_engine_new(tg_engine **out);", ""),
            // A parameter's type changed.
            BASE.replace("uint64_t id", "uint32_t id"),
            // A struct field added (the layout grows).
            BASE.replace("size_t len; }", "size_t len; size_t cap; }"),
            // A constant's value changed.
            BASE.replace("TG_ERROR_IO 2", "TG_ERROR_IO 3"),
            // A callback's signature changed.
            BASE.replace("uint64_t done);", "uint32_t done);"),
        ] {
            let d = diff(&now);
            assert_eq!(d.breaks.len(), 1, "{now}");
            assert_eq!(d.minimum((0, 3, 1)), (0, 4, 0));
            assert_eq!(d.minimum((1, 2, 3)), (2, 0, 0));
        }
    }

    #[test]
    fn symbols_must_match_the_header() {
        let set = |names: &[&str]| {
            names
                .iter()
                .map(|n| (*n).to_owned())
                .collect::<BTreeSet<_>>()
        };
        let expected = set(&["tg_engine_new", "tg_engine_get"]);
        assert!(symbols(&set(&["tg_engine_new", "tg_engine_get"]), &expected).is_ok());
        assert!(
            symbols(
                &set(&[
                    "tg_engine_new",
                    "tg_engine_get",
                    "blake3_compress_in_place_portable"
                ]),
                &expected
            )
            .is_ok()
        );
        assert_eq!(
            symbols(&set(&["tg_engine_new"]), &expected).map_err(|e| e.len()),
            Err(1)
        );
        assert_eq!(
            symbols(
                &set(&["tg_engine_new", "tg_engine_get", "stray"]),
                &expected
            )
            .map_err(|e| e.len()),
            Err(1)
        );
        assert_eq!(functions(&declarations(BASE)), expected);
    }

    #[test]
    fn versions_parse() {
        assert_eq!(parse_version("0.3.1"), Some((0, 3, 1)));
        assert_eq!(parse_version("1.0.0-rc.1"), Some((1, 0, 0)));
        assert_eq!(parse_version("1.0"), None);
    }
}
