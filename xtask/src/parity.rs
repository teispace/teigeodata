//! `cargo xtask parity-mutants`: proves that every language's parity suite checks every value of
//! `bindings/parity.json` (5.9, [research](../../docs/02-research/notes/parity-gate.md)). For
//! each leaf it writes a copy with that one value changed, runs each suite against the copy
//! (through `TEIGEODATA_PARITY`, or the suite's path argument) and requires it to fail. A leaf
//! a suite survives is a value that language does not check.
//!
//! First every suite must pass on an unchanged copy: a suite that always fails would kill
//! every mutant. The suites are built once (`--no-build` skips it), then each language runs
//! its leaves in its own thread, `--jobs` languages at a time.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use serde_json::Value;

use crate::package_c::command;

const USAGE: &str = "usage: cargo xtask parity-mutants [--lang <a,b>] [--leaf <prefix>] \
                     [--jobs <n>] [--sample] [--no-build] [--list]";

/// Leaves a suite may survive: a leaf or a section (`distance_m.out_of_range` covers its
/// items), the languages it applies to (empty: every one) and why no check can see it. Keep it
/// short and reviewed with `parity.json`: every other survivor is a missing check.
const SURVIVABLE: &[(&str, &[&str], &str)] = &[
    (
        "distance_m.out_of_range",
        &[],
        "a point still out of range is refused the same way",
    ),
    (
        "engine.bad_id",
        &[],
        "an ID still malformed is refused the same way",
    ),
    (
        "engine.bad_iso",
        &[],
        "a code still malformed is refused the same way",
    ),
    (
        "engine.open_with.bad_components",
        &[],
        "a name still unknown is refused the same way",
    ),
    (
        "engine.open_with.bad_verify",
        &[],
        "a mode still unknown is refused the same way",
    ),
    (
        "engine.open_with.bad_outline_level",
        &[],
        "a level still too deep is refused the same way",
    ),
    (
        "store.bad_components",
        &[],
        "a name still unknown is refused the same way",
    ),
    (
        "tz.unknown",
        &[],
        "a zone still unknown has no rows the same way",
    ),
    (
        "tz.embedded.past_end",
        &[],
        "a range still further past the end is refused the same way",
    ),
    (
        "engine.open_with.corrupt_at",
        &[],
        "the next byte is in the same section",
    ),
    (
        "engine.search.limit",
        &[],
        "one more result keeps the same first one",
    ),
    (
        "engine.within_radius.limit",
        &[],
        "the circle holds fewer places than either limit",
    ),
    (
        "engine.search_with.focus_scale_m",
        &[],
        "a wider scale keeps the same order here",
    ),
    ("tz.gap.civil.4", &[], "a minute later is still in the gap"),
    ("tz.gap.civil.5", &[], "a second later is still in the gap"),
    (
        "tz.overlap.civil.4",
        &[],
        "a minute later is still in the overlap",
    ),
    (
        "tz.overlap.civil.5",
        &[],
        "a second later is still in the overlap",
    ),
    ("tz.offset.unix", &[], "a second later has the same offset"),
    (
        "engine.open_with.languages",
        &[],
        "a pack opened with `point` alone reads no names, whatever the languages",
    ),
    (
        "store.mirror",
        &[],
        "a mirror that cannot be reached fails the same way",
    ),
    (
        "store.pack",
        &[],
        "the refresh fails at the mirror before the name matters",
    ),
    (
        "error_codes",
        &["python", "node"],
        "errors are told apart by name here (exception classes, `err.code`), not by number",
    ),
];

/// Whether `leaf` may survive in `lang`.
fn survivable(leaf: &str, lang: &str) -> bool {
    SURVIVABLE.iter().any(|(at, langs, _)| {
        (leaf == *at || leaf.strip_prefix(at).is_some_and(|r| r.starts_with('.')))
            && (langs.is_empty() || langs.contains(&lang))
    })
}

/// A language's suite: how to build it once and how to run it on a parity file.
struct Suite {
    name: &'static str,
}

const SUITES: &[Suite] = &[
    Suite { name: "python" },
    Suite { name: "node" },
    Suite { name: "go" },
    Suite { name: "jvm" },
    Suite { name: "swift" },
    Suite { name: "dart" },
    Suite { name: "c" },
    Suite { name: "cpp" },
];

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let (mut langs, mut prefix, mut jobs) = (None, None, 2usize);
    let (mut build, mut list, mut sample) = (true, false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| vec![USAGE.to_owned()]);
        match a.as_str() {
            "--lang" => langs = Some(value()?),
            "--leaf" => prefix = Some(value()?),
            "--jobs" => jobs = value()?.parse().map_err(|_| vec![USAGE.to_owned()])?,
            "--no-build" => build = false,
            "--list" => list = true,
            "--sample" => sample = true,
            _ => return Err(vec![USAGE.to_owned()]),
        }
    }
    let original = root.join("bindings/parity.json");
    let text =
        fs::read_to_string(&original).map_err(|e| vec![format!("{}: {e}", original.display())])?;
    let parity: Value =
        serde_json::from_str(&text).map_err(|e| vec![format!("parity.json: {e}")])?;
    let mut leaves = Vec::new();
    collect(&parity, &mut Vec::new(), &mut leaves);
    if let Some(p) = &prefix {
        leaves.retain(|l| l.starts_with(p.as_str()));
    }
    if sample {
        // The first leaf of each top-level section that a suite can observe (leaves every
        // language may survive prove nothing): a few runs that prove on every push that no
        // suite has stopped reading the file.
        let mut seen = std::collections::BTreeSet::new();
        leaves.retain(|l| {
            !survivable(l, "") && seen.insert(l.split('.').next().unwrap_or_default().to_owned())
        });
    }
    if list {
        return Ok(leaves.join("\n"));
    }
    let suites: Vec<&Suite> = match &langs {
        Some(names) => names
            .split(',')
            .map(|n| {
                SUITES
                    .iter()
                    .find(|s| s.name == n)
                    .ok_or_else(|| vec![format!("no suite {n}")])
            })
            .collect::<Result<_, _>>()?,
        None => SUITES.iter().collect(),
    };
    if build {
        prepare(root, &suites)?;
    }
    let survivors: Mutex<BTreeMap<&str, Vec<String>>> = Mutex::new(BTreeMap::new());
    let problems: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let queue = Mutex::new(suites.clone());
    std::thread::scope(|scope| {
        for _ in 0..jobs.max(1) {
            scope.spawn(|| {
                while let Some(suite) = queue.lock().map_or(None, |mut q| q.pop()) {
                    match mutate_suite(root, suite, &parity, &leaves) {
                        Ok(lived) => {
                            // As each language ends, so a long run shows its results early.
                            eprintln!("{}: {} survived {lived:?}", suite.name, lived.len());
                            if let Ok(mut s) = survivors.lock() {
                                s.insert(suite.name, lived);
                            }
                        }
                        Err(e) => {
                            if let Ok(mut p) = problems.lock() {
                                p.extend(e);
                            }
                        }
                    }
                }
            });
        }
    });
    let mut problems = problems.into_inner().unwrap_or_default();
    let survivors = survivors.into_inner().unwrap_or_default();
    let mut report = String::new();
    for (name, lived) in &survivors {
        let unexpected: Vec<&String> = lived.iter().filter(|l| !survivable(l, name)).collect();
        let _ = writeln!(
            report,
            "{name}: {} leaves, {} killed, {} survived ({} allowed)",
            leaves.len(),
            leaves.len() - lived.len(),
            lived.len(),
            lived.len() - unexpected.len()
        );
        for l in unexpected {
            problems.push(format!(
                "{name}: {l} survived (the suite does not check it)"
            ));
        }
    }
    if problems.is_empty() {
        Ok(report.trim_end().to_owned())
    } else {
        eprint!("{report}");
        Err(problems)
    }
}

/// Every leaf's path, `about` notes left out: `engine.search.first_id`, `fold.0.1`.
fn collect(value: &Value, path: &mut Vec<String>, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if k.starts_with("about") {
                    continue;
                }
                path.push(k.clone());
                collect(v, path, out);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                path.push(i.to_string());
                collect(v, path, out);
                path.pop();
            }
        }
        _ => out.push(path.join(".")),
    }
}

/// The value with the leaf at `path` changed so that no check can still pass: a string is
/// reversed and gains a letter (an input then misses too: `fold` drops punctuation, a search
/// forgives a typo, and `""` still changes), an integer gains 1, a float `max(1, |x|)`, a boolean
/// flips.
fn plant(parity: &Value, path: &str) -> Option<Value> {
    let mut copy = parity.clone();
    let mut at = &mut copy;
    for key in path.split('.') {
        at = match at {
            Value::Object(map) => map.get_mut(key)?,
            Value::Array(items) => items.get_mut(key.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    *at = match at {
        Value::String(s) => Value::String(s.chars().rev().chain(['q']).collect()),
        Value::Bool(b) => Value::Bool(!*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::from(i + 1)
            } else if let Some(u) = n.as_u64() {
                Value::from(u + 1)
            } else {
                let x = n.as_f64()?;
                Value::from(x + x.abs().max(1.0))
            }
        }
        _ => return None,
    };
    Some(copy)
}

/// Runs one suite on an unchanged copy (it must pass), then on each leaf's mutant (it must
/// fail); returns the leaves it survived.
fn mutate_suite(
    root: &Path,
    suite: &Suite,
    parity: &Value,
    leaves: &[String],
) -> Result<Vec<String>, Vec<String>> {
    let file = root.join(format!("bindings/parity.planted.{}.json", suite.name));
    let write = |v: &Value| {
        fs::write(&file, v.to_string()).map_err(|e| vec![format!("{}: {e}", file.display())])
    };
    write(parity)?;
    if !passes(root, suite, &file)? {
        let _ = fs::remove_file(&file);
        return Err(vec![format!(
            "{}: the suite fails on an unchanged parity.json (run it alone to see why)",
            suite.name
        )]);
    }
    let mut lived = Vec::new();
    for leaf in leaves {
        let mutant = plant(parity, leaf).ok_or_else(|| vec![format!("{leaf}: not a scalar")])?;
        write(&mutant)?;
        if passes(root, suite, &file)? {
            lived.push(leaf.clone());
        }
    }
    let _ = fs::remove_file(&file);
    Ok(lived)
}

/// Whether the suite passes on the parity file at `file`.
fn passes(root: &Path, suite: &Suite, file: &Path) -> Result<bool, Vec<String>> {
    let bindings = root.join("bindings");
    let ffi = root.join(format!("target/release/{}", lib("teigeodata_ffi")));
    let path = file.display().to_string();
    let mut cmd = match suite.name {
        "python" => {
            let mut c = Command::new("python3");
            c.current_dir(bindings.join("python"))
                .args(["-m", "unittest", "discover", "-s", "tests"])
                .env("PYTHONPATH", ".");
            c
        }
        "node" => {
            let mut c = Command::new("node");
            c.current_dir(bindings.join("node"))
                .args(["--test", "test/parity.test.js"]);
            c
        }
        "go" => {
            let mut c = Command::new(root.join("target/go-parity.test"));
            c.current_dir(bindings.join("go"))
                .args(["-test.count=1"])
                .env("TEIGEODATA_LIB", &ffi);
            c
        }
        "jvm" => {
            let mut c = Command::new("java");
            c.current_dir(&bindings).args([
                "--enable-native-access=ALL-UNNAMED",
                "-Djava.library.path=target/release",
                "-cp",
                "jvm/build",
                "org.teispace.teigeodata.ParityTest",
                &path,
            ]);
            c
        }
        "swift" => {
            let mut c = Command::new(bindings.join("swift/.build/release/Parity"));
            c.current_dir(bindings.join("swift")).arg(&path);
            c
        }
        "dart" => {
            let mut c = Command::new(root.join("target/dart-parity/bundle/bin/parity_test"));
            c.current_dir(bindings.join("dart"));
            c
        }
        "c" | "cpp" => {
            let exe = compile_c(root, suite.name, file)?;
            let mut c = Command::new(exe);
            c.current_dir(root);
            c
        }
        other => return Err(vec![format!("no runner for {other}")]),
    };
    let status = cmd
        .env("TEIGEODATA_PARITY", &path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| vec![format!("{}: {e}", suite.name)])?;
    Ok(status.success())
}

/// Compiles the C or C++ suite against the header of the parity file at `file`, in its own
/// directory under `target/` (the source sits beside its header, so `#include "parity_data.h"`
/// finds the planted one, not `bindings/c`'s); returns the program. A compile error is a
/// problem, not a killed mutant.
fn compile_c(root: &Path, lang: &str, file: &Path) -> Result<PathBuf, Vec<String>> {
    let text = fs::read_to_string(file).map_err(|e| vec![format!("{}: {e}", file.display())])?;
    let parity: Value = serde_json::from_str(&text).map_err(|e| vec![format!("{e}")])?;
    let dir = root.join(format!("target/parity-{lang}"));
    let (source, compiler, std) = if lang == "c" {
        ("parity.c", "cc", "-std=c11")
    } else {
        ("parity.cpp", "c++", "-std=c++17")
    };
    fs::create_dir_all(&dir).map_err(|e| vec![format!("{}: {e}", dir.display())])?;
    fs::copy(root.join("bindings/c").join(source), dir.join(source))
        .map_err(|e| vec![format!("{source}: {e}")])?;
    fs::write(
        dir.join("parity_data.h"),
        header(&parity).map_err(|e| vec![e])?,
    )
    .map_err(|e| vec![format!("{}: {e}", dir.display())])?;
    let release = root.join("target/release").display().to_string();
    let exe = dir.join("parity");
    let (src, out) = (
        dir.join(source).display().to_string(),
        exe.display().to_string(),
    );
    let include = format!("-I{}", root.join("bindings/c").display());
    let (link, rpath) = (format!("-L{release}"), format!("-Wl,-rpath,{release}"));
    command(
        root,
        compiler,
        &[
            std,
            "-Wall",
            "-Wextra",
            "-Werror",
            "-pedantic",
            &src,
            &include,
            &link,
            "-lteigeodata_ffi",
            &rpath,
            "-o",
            &out,
        ],
    )?;
    Ok(exe)
}

/// This platform's file name of a Rust cdylib.
fn lib(name: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("lib{name}.dylib")
    } else if cfg!(windows) {
        format!("{name}.dll")
    } else {
        format!("lib{name}.so")
    }
}

/// Builds the C library and each chosen suite once.
fn prepare(root: &Path, suites: &[&Suite]) -> Result<(), Vec<String>> {
    let bindings = root.join("bindings");
    let has = |n: &str| suites.iter().any(|s| s.name == n);
    command(
        root,
        "cargo",
        &[
            "build",
            "--locked",
            "--release",
            "-p",
            "teigeodata-ffi",
            "--lib",
        ],
    )?;
    let shims: Vec<&str> = [
        ("python", "teigeodata-python"),
        ("node", "teigeodata-node"),
        ("jvm", "teigeodata-jvm"),
    ]
    .iter()
    .filter(|(n, _)| has(n))
    .map(|(_, p)| *p)
    .collect();
    if !shims.is_empty() {
        let mut args = vec!["build", "--locked", "--release"];
        for p in &shims {
            args.extend(["-p", p]);
        }
        let mut cmd = Command::new("cargo");
        cmd.current_dir(&bindings)
            .args(&args)
            .env("PYO3_BUILD_EXTENSION_MODULE", "1");
        check(&mut cmd, "cargo build (the shims)")?;
    }
    let copy = |from: PathBuf, to: PathBuf| {
        fs::copy(&from, &to)
            .map(|_| ())
            .map_err(|e| vec![format!("{}: {e}", from.display())])
    };
    let release = bindings.join("target/release");
    if has("python") {
        copy(
            release.join(lib("teigeodata_python")),
            bindings.join("python/teigeodata/_native.abi3.so"),
        )?;
    }
    if has("node") {
        copy(
            release.join(lib("teigeodata_node")),
            bindings.join("node/teigeodata.node"),
        )?;
    }
    if has("jvm") {
        let sources = |dir: &str| -> Result<Vec<String>, Vec<String>> {
            let d = bindings.join(dir);
            let mut files: Vec<String> = fs::read_dir(&d)
                .map_err(|e| vec![format!("{}: {e}", d.display())])?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "java"))
                .map(|p| p.display().to_string())
                .collect();
            files.sort();
            Ok(files)
        };
        let mut args = vec!["-nowarn".to_owned(), "-d".into(), "jvm/build".into()];
        args.extend(sources("jvm/java/org/teispace/teigeodata")?);
        args.extend(sources("jvm/test/org/teispace/teigeodata")?);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        command(&bindings, "javac", &args)?;
    }
    if has("go") {
        let out = root.join("target/go-parity.test").display().to_string();
        command(&bindings.join("go"), "go", &["test", "-c", "-o", &out, "."])?;
    }
    if has("swift") {
        command(
            &bindings.join("swift"),
            "swift",
            &["build", "-c", "release", "--product", "Parity"],
        )?;
    }
    if has("dart") {
        // `dart build cli` runs the build hook and bundles the library; `compile exe` does not.
        let out = root.join("target/dart-parity").display().to_string();
        command(
            &bindings.join("dart"),
            "dart",
            &["pub", "get", "--no-example"],
        )?;
        command(
            &bindings.join("dart"),
            "dart",
            &["build", "cli", "-t", "test/parity_test.dart", "-o", &out],
        )?;
    }
    Ok(())
}

fn check(cmd: &mut Command, what: &str) -> Result<(), Vec<String>> {
    let status = cmd.status().map_err(|e| vec![format!("{what}: {e}")])?;
    if status.success() {
        Ok(())
    } else {
        Err(vec![format!("{what} failed ({status})")])
    }
}

/// Where the C and C++ suites read `parity.json` from: a header of macros, one per leaf
/// (`TGP_ENGINE_SEARCH_FIRST_ID`), plus `_COUNT` and `_ITEMS` (an initializer list) for each
/// array. `cargo xtask bindings` keeps it current; a planted run writes its own copy.
pub(crate) const HEADER: &str = "bindings/c/parity_data.h";

/// The C header of `parity`; two leaves whose macro names collide are an error.
pub(crate) fn header(parity: &Value) -> Result<String, String> {
    let mut out = String::from(
        "/* Generated by cargo xtask bindings from bindings/parity.json. Do not edit.\n \
         * The C and C++ parity suites read the golden values from these macros. */\n\
         #ifndef TEIGEODATA_PARITY_DATA_H\n#define TEIGEODATA_PARITY_DATA_H\n\n\
         #include <stdbool.h>\n\n",
    );
    define(parity, &mut Vec::new(), &mut out);
    out.push_str("\n#endif /* TEIGEODATA_PARITY_DATA_H */\n");
    let mut names = std::collections::HashSet::new();
    for line in out.lines() {
        if let Some(name) = line
            .strip_prefix("#define TGP_")
            .and_then(|l| l.split(' ').next())
            && !names.insert(name)
        {
            return Err(format!("parity.json: two values are both TGP_{name} in C"));
        }
    }
    Ok(out)
}

fn define(value: &Value, path: &mut Vec<String>, out: &mut String) {
    let name = || format!("TGP_{}", path.join("_").to_uppercase());
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if k.starts_with("about") {
                    continue;
                }
                path.push(k.clone());
                define(v, path, out);
                path.pop();
            }
        }
        Value::Array(items) => {
            let _ = writeln!(out, "#define {}_COUNT {}", name(), items.len());
            if let Some(list) = initializer(items) {
                let _ = writeln!(out, "#define {}_ITEMS {list}", name());
            }
            for (i, v) in items.iter().enumerate() {
                path.push(i.to_string());
                define(v, path, out);
                path.pop();
            }
        }
        scalar => {
            let _ = writeln!(out, "#define {} {}", name(), literal(scalar));
        }
    }
}

/// An array's elements as a C initializer list (`{a, b}, {c, d}` for an array of arrays); none
/// for an empty array or one holding objects.
fn initializer(items: &[Value]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    let parts: Option<Vec<String>> = items
        .iter()
        .map(|v| match v {
            Value::Object(_) => None,
            Value::Array(inner) => initializer(inner).map(|l| format!("{{{l}}}")),
            scalar => Some(literal(scalar)),
        })
        .collect();
    parts.map(|p| p.join(", "))
}

/// A scalar as a C literal: strings escaped in octal (never ambiguous with what follows),
/// integers outside 32 bits with `LL`, floats always with a point or exponent.
fn literal(value: &Value) -> String {
    match value {
        Value::String(s) => {
            let mut lit = String::from("\"");
            for b in s.bytes() {
                match b {
                    b'"' | b'\\' => {
                        lit.push('\\');
                        lit.push(char::from(b));
                    }
                    0x20..=0x7e => lit.push(char::from(b)),
                    _ => {
                        let _ = write!(lit, "\\{b:03o}");
                    }
                }
            }
            lit.push('"');
            lit
        }
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) if i32::try_from(i).is_ok() => i.to_string(),
            (Some(i), _) => format!("{i}LL"),
            (None, Some(f)) => {
                let text = format!("{f:?}");
                if text.contains(['.', 'e', 'E']) || text.contains("inf") || text.contains("NaN") {
                    text
                } else {
                    format!("{text}.0")
                }
            }
            _ => n.to_string(),
        },
        _ => "0".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{collect, header, plant, survivable};

    #[test]
    fn leaves_and_plants() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"about":"x","a":{"b":[1,2.5,"sé",true,""],"about_c":1},"d":-3}"#,
        )
        .unwrap_or_default();
        let mut out = Vec::new();
        collect(&v, &mut Vec::new(), &mut out);
        assert_eq!(out, ["a.b.0", "a.b.1", "a.b.2", "a.b.3", "a.b.4", "d"]);
        let planted = |p: &str| plant(&v, p).map(|m| m.to_string()).unwrap_or_default();
        assert!(planted("a.b.0").contains("[2,2.5"));
        assert!(planted("a.b.1").contains("5.0"));
        assert!(planted("a.b.2").contains("\"ésq\""));
        assert!(planted("a.b.4").contains("\"q\""));
        assert!(planted("a.b.3").contains("false"));
        assert!(planted("d").contains("-2"));
        assert!(plant(&v, "a.b").is_none());
    }

    #[test]
    fn allowed_survivors() {
        assert!(survivable("distance_m.out_of_range.2", "go"));
        assert!(!survivable("distance_m.out_of_range_x", "go"));
        assert!(survivable("error_codes.Io", "python"));
        assert!(!survivable("error_codes.Io", "go"));
        assert!(!survivable("engine.get.lon", "python"));
    }

    #[test]
    fn c_header() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"s":"S\u00e3o \"q\"","n":[1,-3000000000,2.5,1000.0],"b":false,"about":"x"}"#,
        )
        .unwrap_or_default();
        let h = header(&v).unwrap_or_default();
        assert!(
            h.contains("#define TGP_S \"S\\303\\243o \\\"q\\\"\"\n"),
            "{h}"
        );
        assert!(h.contains("#define TGP_N_COUNT 4\n"));
        assert!(h.contains("#define TGP_N_ITEMS 1, -3000000000LL, 2.5, 1000.0\n"));
        assert!(h.contains("#define TGP_N_3 1000.0\n"));
        assert!(h.contains("#define TGP_B false\n"));
        assert!(!h.contains("ABOUT"));
        let pairs: serde_json::Value =
            serde_json::from_str(r#"{"f":[["a","b"],["c","d"]]}"#).unwrap_or_default();
        let h = header(&pairs).unwrap_or_default();
        assert!(
            h.contains("#define TGP_F_ITEMS {\"a\", \"b\"}, {\"c\", \"d\"}\n"),
            "{h}"
        );
        let clash: serde_json::Value =
            serde_json::from_str(r#"{"a_b":1,"a":{"b":2}}"#).unwrap_or_default();
        assert!(header(&clash).is_err());
    }
}
