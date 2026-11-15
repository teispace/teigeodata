//! `cargo xtask docs-examples`: the docs site's examples are programs, and they ran (phase 7,
//! [research](../../docs/02-research/notes/docs-site.md)).
//!
//! Each language has one tour under `site/examples/<lang>/` that runs every query of its guide
//! on the released pack pinned in `site/data.json` and prints it section by section
//! (`=== <section>`). Every language prints the same text, checked against
//! `site/examples/tour.out`; the command line has its own (`site/examples/cli/tour.out`). The
//! pages include each section's code (a region of the program) and its output, split into
//! `site/examples/out/<section>.txt` (`site/examples/cli/out/` for the command line).
//!
//! `--check` fails on any difference, and on split files that do not match the checked-in
//! output; `--bless` writes them from one language's run. A language whose toolchain or
//! binding build is missing is skipped and named, and fails the check only when `--lang`
//! asked for it.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use sha2::{Digest as _, Sha256};

/// The pack every example runs on.
#[derive(Deserialize)]
pub(crate) struct Data {
    /// The data release that holds it.
    pub(crate) release: String,
    pub(crate) file: String,
    bytes: u64,
    sha256: String,
    url: String,
}

/// One language: how to run its tour, and what it needs first.
struct Lang {
    name: &'static str,
    /// The tool that must be on `PATH`.
    tool: &'static str,
    /// Files that must exist (binding builds), relative to the root.
    needs: &'static [&'static str],
    /// The command, with `{pack}`, `{root}` and `{ffi}` (the C library, relative to the root)
    /// replaced; `needs` and `env` are filled the same way.
    command: &'static [&'static str],
    /// Where to run it, relative to the root.
    dir: &'static str,
    env: &'static [(&'static str, &'static str)],
    /// Symbolic links made first, `(link, target)` relative to the root: a package the tour
    /// imports by its published name, resolved to this checkout's build.
    links: &'static [(&'static str, &'static str)],
    /// Its own expected output (`None`: the shared tour).
    own: Option<&'static str>,
}

/// The C library as `cargo build --release -p teigeodata-ffi` writes it.
#[cfg(target_os = "macos")]
const FFI: &str = "target/release/libteigeodata_ffi.dylib";
#[cfg(windows)]
const FFI: &str = "target/release/teigeodata_ffi.dll";
#[cfg(not(any(target_os = "macos", windows)))]
const FFI: &str = "target/release/libteigeodata_ffi.so";

const LANGS: &[Lang] = &[
    Lang {
        name: "rust",
        tool: "cargo",
        needs: &[],
        command: &[
            "cargo",
            "run",
            "-q",
            "--release",
            "--locked",
            "-p",
            "teigeodata-api",
            "--example",
            "tour",
            "--",
            "{pack}",
        ],
        dir: "",
        env: &[],
        links: &[],
        own: None,
    },
    Lang {
        name: "python",
        tool: "python3",
        needs: &["bindings/python/teigeodata/_native.abi3.so"],
        command: &["python3", "site/examples/python/tour.py", "{pack}"],
        dir: "",
        env: &[("PYTHONPATH", "{root}/bindings/python")],
        links: &[],
        own: None,
    },
    Lang {
        name: "node",
        tool: "node",
        needs: &["bindings/node/teigeodata.node"],
        command: &["node", "site/examples/node/tour.mjs", "{pack}"],
        dir: "",
        env: &[],
        links: &[(
            "site/examples/node/node_modules/@teispace/teigeodata",
            "bindings/node",
        )],
        own: None,
    },
    Lang {
        name: "wasm",
        tool: "node",
        needs: &["target/wasm/full/reader.js"],
        command: &["node", "site/examples/wasm/tour.mjs", "{pack}"],
        dir: "",
        env: &[],
        links: &[(
            "site/examples/wasm/node_modules/@teispace/teigeodata-wasm",
            "target/wasm/full",
        )],
        own: None,
    },
    Lang {
        name: "go",
        tool: "go",
        needs: &["{ffi}"],
        command: &["go", "run", ".", "{pack}"],
        dir: "site/examples/go",
        env: &[("TEIGEODATA_LIB", "{root}/{ffi}")],
        links: &[],
        own: None,
    },
    Lang {
        name: "swift",
        tool: "swift",
        needs: &["{ffi}"],
        command: &["swift", "run", "-c", "release", "-q", "Tour", "{pack}"],
        dir: "site/examples/swift/Tour",
        env: &[],
        links: &[],
        own: None,
    },
    Lang {
        name: "dart",
        tool: "dart",
        // The package's build hook builds the C library from this checkout (D-289).
        needs: &[],
        command: &[
            "sh",
            "-c",
            "dart pub get >&2 && dart run bin/tour.dart \"$0\"",
            "{pack}",
        ],
        dir: "site/examples/dart",
        env: &[],
        links: &[],
        own: None,
    },
    Lang {
        name: "c",
        tool: "cc",
        needs: &["{ffi}"],
        command: &[
            "sh",
            "-c",
            "mkdir -p target/docs-c && lib=$(dirname \"$1\") && \
             cc -std=c11 -Wall -Wextra -Werror -pedantic site/examples/c/tour.c -Ibindings/c \
             -L\"$lib\" -lteigeodata_ffi -Wl,-rpath,\"$lib\" -o target/docs-c/tour && \
             target/docs-c/tour \"$0\"",
            "{pack}",
            "{root}/{ffi}",
        ],
        dir: "",
        env: &[],
        links: &[],
        own: None,
    },
    Lang {
        name: "cpp",
        tool: "c++",
        needs: &["{ffi}"],
        command: &[
            "sh",
            "-c",
            "mkdir -p target/docs-c && lib=$(dirname \"$1\") && \
             c++ -std=c++17 -Wall -Wextra -Werror -pedantic -Wconversion site/examples/cpp/tour.cpp \
             -Ibindings/c -L\"$lib\" -lteigeodata_ffi -Wl,-rpath,\"$lib\" -o target/docs-c/tour-cpp && \
             target/docs-c/tour-cpp \"$0\"",
            "{pack}",
            "{root}/{ffi}",
        ],
        dir: "",
        env: &[],
        links: &[],
        own: None,
    },
    Lang {
        name: "java",
        tool: "java",
        // The classes as the JVM step builds them (javac into bindings/jvm/build) and the JNI
        // library of the bindings workspace.
        needs: &["bindings/jvm/build/org/teispace/teigeodata/Reader.class"],
        command: &[
            "java",
            "--enable-native-access=ALL-UNNAMED",
            "-Djava.library.path={root}/bindings/target/release",
            "-cp",
            "{root}/bindings/jvm/build",
            "site/examples/java/Tour.java",
            "{pack}",
        ],
        dir: "",
        env: &[],
        links: &[],
        own: None,
    },
    Lang {
        name: "kotlin",
        tool: "kotlinc",
        needs: &["bindings/jvm/build/org/teispace/teigeodata/Reader.class"],
        command: &[
            "sh",
            "-c",
            "kotlinc -Werror site/examples/kotlin/Tour.kt -cp \"$1/bindings/jvm/build\" \
             -include-runtime -d \"$1/target/docs-kotlin/tour.jar\" && \
             java --enable-native-access=ALL-UNNAMED -Djava.library.path=\"$1/bindings/target/release\" \
             -cp \"$1/target/docs-kotlin/tour.jar:$1/bindings/jvm/build\" TourKt \"$0\"",
            "{pack}",
            "{root}",
        ],
        dir: "",
        env: &[],
        links: &[],
        own: None,
    },
    Lang {
        name: "dotnet",
        tool: "dotnet",
        needs: &["{ffi}"],
        command: &[
            "sh",
            "-c",
            "dotnet build -c Release -v q site/examples/dotnet >&2 && \
             dotnet run --no-build -c Release --project site/examples/dotnet -- \"$0\"",
            "{pack}",
        ],
        dir: "",
        env: &[("TEIGEODATA_LIB", "{root}/{ffi}")],
        links: &[],
        own: None,
    },
    Lang {
        name: "cli",
        tool: "sh",
        needs: &["target/release/tgd"],
        command: &["sh", "site/examples/cli/tour.sh", "{pack}"],
        dir: "",
        env: &[("TGD", "{root}/target/release/tgd")],
        links: &[],
        own: Some("site/examples/cli"),
    },
];

/// The parsed command line.
#[derive(Default)]
struct Args {
    check: bool,
    bless: bool,
    fetch: bool,
    pack: Option<PathBuf>,
    asked: Vec<String>,
}

fn parse(args: &[String]) -> Result<Args, Vec<String>> {
    let mut check = false;
    let mut bless = false;
    let mut fetch = false;
    let mut pack: Option<PathBuf> = None;
    let mut asked: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--check" => check = true,
            "--bless" => bless = true,
            "--fetch" => fetch = true,
            "--pack" => pack = it.next().map(PathBuf::from),
            "--lang" => {
                let list = it
                    .next()
                    .ok_or_else(|| vec!["--lang needs a value".to_owned()])?;
                asked.extend(list.split(',').map(str::to_owned));
            }
            other => return Err(vec![format!("unknown option {other}")]),
        }
    }
    if check && bless {
        return Err(vec!["--check and --bless exclude each other".into()]);
    }
    for a in &asked {
        if !LANGS.iter().any(|l| l.name == a) {
            let names: Vec<_> = LANGS.iter().map(|l| l.name).collect();
            return Err(vec![format!(
                "unknown language {a}; one of {}",
                names.join(", ")
            )]);
        }
    }
    Ok(Args {
        check,
        bless,
        fetch,
        pack,
        asked,
    })
}

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let Args {
        check,
        bless,
        fetch,
        pack,
        asked,
    } = parse(args)?;
    let data: Data = serde_json::from_str(&read(&root.join("site/data.json"))?)
        .map_err(|e| vec![format!("site/data.json: {e}")])?;
    let pack = match pack {
        Some(p) => p,
        None => pinned(root, &data, fetch)?,
    };
    let pack = pack
        .canonicalize()
        .map_err(|e| vec![format!("{}: {e}", pack.display())])?;

    let mut out = String::new();
    let mut problems = Vec::new();
    let chosen: Vec<&Lang> = LANGS
        .iter()
        .filter(|l| asked.is_empty() || asked.iter().any(|a| a == l.name))
        .collect();
    if bless && chosen.iter().filter(|l| l.own.is_none()).count() > 1 {
        return Err(vec![
            "--bless writes the shared tour from one language: pass --lang".into(),
        ]);
    }
    for lang in chosen {
        if let Some(why) = missing(root, lang) {
            let _ = writeln!(out, "{:8} skipped: {why}", lang.name);
            if !asked.is_empty() {
                problems.push(format!("{}: {why}", lang.name));
            }
            continue;
        }
        if let Err(e) = link(root, lang) {
            problems.push(format!("{}: {e}", lang.name));
            continue;
        }
        let printed = match execute(root, lang, &pack) {
            Ok(p) => p,
            Err(e) => {
                problems.push(format!("{}: {e}", lang.name));
                continue;
            }
        };
        let dir = root.join(lang.own.unwrap_or("site/examples"));
        let expected_path = dir.join("tour.out");
        if bless {
            write(&expected_path, &printed)?;
            split(&dir, &printed, true)?;
            let _ = writeln!(out, "{:8} blessed {}", lang.name, rel(root, &expected_path));
            continue;
        }
        let expected = fs::read_to_string(&expected_path).unwrap_or_default();
        if printed == expected {
            let _ = writeln!(
                out,
                "{:8} ok ({} sections)",
                lang.name,
                sections(&printed).len()
            );
        } else {
            problems.push(format!(
                "{}: differs from {}: {}",
                lang.name,
                rel(root, &expected_path),
                first_difference(&expected, &printed)
            ));
        }
        if check {
            problems.extend(split(&dir, &expected, false)?);
        }
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        eprint!("{out}");
        Err(problems)
    }
}

/// The pinned pack under `target/docs-data/`, fetched and checked when `fetch` is set.
pub(crate) fn pinned(root: &Path, data: &Data, fetch: bool) -> Result<PathBuf, Vec<String>> {
    let dir = root.join("target/docs-data");
    let path = dir.join(&data.file);
    if !path.is_file() {
        if !fetch {
            return Err(vec![format!(
                "{} is missing: pass --fetch to download it ({} bytes from {})",
                rel(root, &path),
                data.bytes,
                data.url
            )]);
        }
        fs::create_dir_all(&dir).map_err(|e| vec![format!("{}: {e}", dir.display())])?;
        let status = Command::new("curl")
            .args(["-fsSL", "--retry", "3", "-o"])
            .arg(&path)
            .arg(&data.url)
            .status()
            .map_err(|e| vec![format!("curl: {e}")])?;
        if !status.success() {
            let _ = fs::remove_file(&path);
            return Err(vec![format!("curl {} exited with {status}", data.url)]);
        }
    }
    let mut hasher = Sha256::new();
    let mut file = fs::File::open(&path).map_err(|e| vec![format!("{}: {e}", path.display())])?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| vec![format!("{}: {e}", path.display())])?;
        if n == 0 {
            break;
        }
        hasher.update(buf.get(..n).unwrap_or_default());
    }
    let digest = hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        });
    if digest != data.sha256 {
        return Err(vec![format!(
            "{}: SHA-256 {digest}, site/data.json pins {}",
            rel(root, &path),
            data.sha256
        )]);
    }
    Ok(path)
}

/// Why `lang` cannot run here, if it cannot.
fn missing(root: &Path, lang: &Lang) -> Option<String> {
    let found = Command::new("sh")
        .args(["-c", &format!("command -v {}", lang.tool)])
        .output()
        .is_ok_and(|o| o.status.success());
    if !found {
        return Some(format!("{} is not installed", lang.tool));
    }
    lang.needs
        .iter()
        .find(|n| !root.join(n.replace("{ffi}", FFI)).exists())
        .map(|n| format!("{n} is not built"))
}

/// Makes `lang`'s links, replacing what is there.
fn link(root: &Path, lang: &Lang) -> Result<(), String> {
    for (at, target) in lang.links {
        let at = root.join(at);
        if let Some(parent) = at.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let _ = fs::remove_file(&at);
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join(target), &at)
            .map_err(|e| format!("{}: {e}", at.display()))?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(root.join(target), &at)
            .map_err(|e| format!("{}: {e}", at.display()))?;
    }
    Ok(())
}

fn execute(root: &Path, lang: &Lang, pack: &Path) -> Result<String, String> {
    let root_s = root.display().to_string();
    let pack_s = pack.display().to_string();
    let fill = |s: &str| {
        s.replace("{ffi}", FFI)
            .replace("{pack}", &pack_s)
            .replace("{root}", &root_s)
    };
    let (program, rest) = lang.command.split_first().ok_or("no command")?;
    let mut cmd = Command::new(fill(program));
    cmd.args(rest.iter().map(|a| fill(a)))
        .current_dir(root.join(lang.dir));
    for (k, v) in lang.env {
        cmd.env(k, fill(v));
    }
    let output = cmd.output().map_err(|e| format!("{program}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "exited with {}:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("output is not UTF-8: {e}"))
}

/// The tour's sections by name. A section name becomes a file name, so it is lowercase ASCII
/// letters, digits and `-`; a line that starts `=== ` with any other name is kept as text.
fn sections(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut current: Option<(String, String)> = None;
    for line in text.lines() {
        if let Some(name) = line
            .strip_prefix("=== ")
            .map(str::trim)
            .filter(|n| is_section(n))
        {
            if let Some((n, t)) = current.take() {
                out.insert(n, t);
            }
            current = Some((name.to_owned(), String::new()));
        } else if let Some((_, t)) = current.as_mut() {
            t.push_str(line);
            t.push('\n');
        }
    }
    if let Some((n, t)) = current {
        out.insert(n, t);
    }
    out
}

fn is_section(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Writes (`bless`) or checks `dir/out/<section>.txt` against `text`; the check names each
/// stale, missing or extra file.
fn split(dir: &Path, text: &str, bless: bool) -> Result<Vec<String>, Vec<String>> {
    let out = dir.join("out");
    let want = sections(text);
    let mut problems = Vec::new();
    if bless {
        let _ = fs::remove_dir_all(&out);
        for (name, body) in &want {
            write(&out.join(format!("{name}.txt")), body)?;
        }
        return Ok(problems);
    }
    for (name, body) in &want {
        let path = out.join(format!("{name}.txt"));
        if fs::read_to_string(&path).ok().as_deref() != Some(body.as_str()) {
            problems.push(format!(
                "{} does not match the tour's output: run --bless",
                path.display()
            ));
        }
    }
    if let Ok(entries) = fs::read_dir(&out) {
        for e in entries.flatten() {
            let name = e
                .file_name()
                .to_string_lossy()
                .trim_end_matches(".txt")
                .to_owned();
            if !want.contains_key(&name) {
                problems.push(format!("{} is no section of the tour", e.path().display()));
            }
        }
    }
    Ok(problems)
}

fn first_difference(expected: &str, printed: &str) -> String {
    let (mut e, mut p) = (expected.lines(), printed.lines());
    for n in 1.. {
        match (e.next(), p.next()) {
            (None, None) => break,
            (a, b) if a == b => {}
            (a, b) => {
                return format!(
                    "line {n}: expected {:?}, printed {:?}",
                    a.unwrap_or("<end>"),
                    b.unwrap_or("<end>")
                );
            }
        }
    }
    "only in line endings".into()
}

pub(crate) fn read(path: &Path) -> Result<String, Vec<String>> {
    fs::read_to_string(path).map_err(|e| vec![format!("{}: {e}", path.display())])
}

fn write(path: &Path, text: &str) -> Result<(), Vec<String>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| vec![format!("{}: {e}", parent.display())])?;
    }
    fs::write(path, text).map_err(|e| vec![format!("{}: {e}", path.display())])
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOUR: &str = "=== open\nopened\n=== search\na\nb\n=== ../evil\nkept\n";

    #[test]
    fn a_tour_splits_into_named_sections_and_bad_names_stay_text() {
        let s = sections(TOUR);
        assert_eq!(s.keys().collect::<Vec<_>>(), ["open", "search"]);
        assert_eq!(s["search"], "a\nb\n=== ../evil\nkept\n");
    }

    #[test]
    fn the_first_difference_names_the_line() {
        assert_eq!(
            first_difference("a\nb\n", "a\nc\n"),
            "line 2: expected \"b\", printed \"c\""
        );
        assert_eq!(
            first_difference("a\n", "a\nmore\n"),
            "line 2: expected \"<end>\", printed \"more\""
        );
    }

    #[test]
    fn the_check_finds_stale_missing_and_extra_section_files() {
        let dir = std::env::temp_dir().join(format!("tgd-docs-examples-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(split(&dir, TOUR, true).unwrap(), Vec::<String>::new());
        assert_eq!(split(&dir, TOUR, false).unwrap(), Vec::<String>::new());

        fs::write(dir.join("out/open.txt"), "changed\n").unwrap();
        fs::remove_file(dir.join("out/search.txt")).unwrap();
        fs::write(dir.join("out/old.txt"), "x\n").unwrap();
        let problems = split(&dir, TOUR, false).unwrap().join("\n");
        assert!(problems.contains("open.txt does not match"), "{problems}");
        assert!(problems.contains("search.txt does not match"), "{problems}");
        assert!(problems.contains("old.txt is no section"), "{problems}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
