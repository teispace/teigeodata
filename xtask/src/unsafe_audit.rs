//! `cargo xtask unsafe-audit [--check]`: the inventory of every `unsafe` in shipped code
//! (phase 8.2, D-331), written to `docs/06-engineering/unsafe-inventory.md`.
//!
//! The workspace forbids `unsafe_code`; four modules of the product lower that to `deny` and
//! allow it block by block, and the language bindings use it where a host's native interface
//! needs it. This task scans the shipped sources (`crates/*/src`, `bindings/*/src`; tests,
//! examples and benches are not shipped) and lists each `unsafe` block and `unsafe impl` with
//! its file, line and argument: the `// SAFETY:` comment directly above it and the reason of
//! the `#[allow(unsafe_code, reason = ...)]` it sits under. Each file is listed with the tests
//! and gates that reach its blocks ([`EVIDENCE`]).
//!
//! It fails, and so does `--check` in fast-check:
//! - on a block or `unsafe impl` without a `SAFETY:` comment (clippy's
//!   `undocumented_unsafe_blocks` says the same while compiling; this sees every target and
//!   every `cfg`, a Windows-only block included);
//! - on a file with `unsafe` that [`EVIDENCE`] does not name, so a new `unsafe` module is
//!   reviewed and given its tests before it lands, and on an entry whose file has none left;
//! - with `--check`, when the page differs from what the sources say.
//!
//! Blocks with the same argument in a file share a row with all their lines: the generated FFI
//! and JNI code repeat a handful of arguments over every exported function.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// The page this task writes.
const PAGE: &str = "docs/06-engineering/unsafe-inventory.md";

/// Files written by `cargo xtask bindings`.
const GENERATED: &[&str] = &[
    "crates/teigeodata-ffi/src/generated.rs",
    "bindings/jvm/src/lib.rs",
];

/// Each file with `unsafe` and what reaches its blocks. A file missing here fails the audit.
const EVIDENCE: &[(&str, &str)] = &[
    (
        "crates/teigeodata-engine/src/map.rs",
        "Every engine test that opens a layer from a file or bytes; `tests/verify.rs` (a file \
         replaced or resized is seen by `Layer::unchanged`); Miri on the format reader the \
         views wrap (nightly `miri`); the `engine` fuzz target (nightly), through \
         `Layer::from_bytes`",
    ),
    (
        "crates/teigeodata-sync/src/holes.rs",
        "`holes::tests::a_punched_range_reads_zero_and_frees_its_blocks` on each platform's \
         file system (fast-check on Linux and macOS, its `windows` job on NTFS); the store's \
         partial-install tests, which punch through `Store` (`store.rs`)",
    ),
    (
        "crates/teigeodata-ffi/src/abi.rs",
        "`tests/abi.rs` (null, non-UTF-8 and over-long arguments, callbacks, cancelling, \
         buffers freed once) and `abi::tests::a_panic_stops_at_the_boundary`; the `ffi` fuzz target (nightly, \
         every engine function with fuzzed arguments); the C, C++, Go, Swift, Dart and .NET \
         parity suites, which call the ABI from each language",
    ),
    (
        "crates/teigeodata-ffi/src/generated.rs",
        "As `abi.rs`: every exported function is called by `tests/abi.rs` or a binding's parity \
         suite, and `the_harness_calls_every_engine_function` fails when the `ffi` fuzz target \
         misses an engine function; `cargo xtask bindings --check` keeps the file equal to its \
         generator",
    ),
    (
        "crates/teigeodata-ffi/src/fuzz.rs",
        "The fuzz harness itself (feature `fuzz`, never in a release build): `tests/fuzz.rs` \
         and the nightly `ffi` target",
    ),
    (
        "bindings/jvm/src/lib.rs",
        "The JVM parity suite (Java and Kotlin, `jvm.yml`) and `parity-mutants` for the JVM, \
         which call every native method through JNI",
    ),
    (
        "bindings/python/src/lib.rs",
        "The Python parity suite and `parity-mutants` for Python (`python.yml`)",
    ),
    (
        "bindings/node/src/lib.rs",
        "The Node.js parity suite and `parity-mutants` for Node.js (`node.yml`)",
    ),
];

/// One `unsafe` site.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Site {
    line: usize,
    what: &'static str,
    argument: String,
}

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let check = args.iter().any(|a| a == "--check");
    let mut files = BTreeMap::new();
    let mut problems = Vec::new();
    for dir in source_dirs(root) {
        for path in rust_files(&dir) {
            let rel = rel(root, &path);
            let text = fs::read_to_string(&path).map_err(|e| vec![format!("{rel}: {e}")])?;
            let (sites, unsafe_fns) = scan(&text);
            for s in &sites {
                if s.argument.is_empty() {
                    problems.push(format!(
                        "{rel}:{}: {} without a `// SAFETY:` comment directly above it",
                        s.line, s.what
                    ));
                }
            }
            if !sites.is_empty() || unsafe_fns > 0 {
                files.insert(rel, (sites, unsafe_fns));
            }
        }
    }
    for rel in files.keys() {
        if !EVIDENCE.iter().any(|(f, _)| f == rel) {
            problems.push(format!(
                "{rel} uses `unsafe` but is not in xtask/src/unsafe_audit.rs EVIDENCE: review \
                 it and name the tests that reach its blocks"
            ));
        }
    }
    for (f, _) in EVIDENCE {
        if !files.contains_key(*f) {
            problems.push(format!("EVIDENCE names {f}, which has no `unsafe` left"));
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }

    let body = render(&files);
    let path = root.join(PAGE);
    let old = fs::read_to_string(&path).unwrap_or_default();
    let (_, old_body) = split_page(&old);
    if old_body == Some(body.as_str()) {
        return Ok(summary(&files, "is current"));
    }
    if check {
        return Err(vec![format!(
            "{PAGE} is stale: run `cargo xtask unsafe-audit` and commit it"
        )]);
    }
    let date = today();
    let page = format!(
        "---\ntitle: \"The unsafe inventory\"\nstatus: accepted\nupdated: {date}\n---\n\n{body}"
    );
    fs::write(&path, page).map_err(|e| vec![format!("{PAGE}: {e}")])?;
    Ok(summary(&files, "written"))
}

fn summary(files: &BTreeMap<String, (Vec<Site>, usize)>, state: &str) -> String {
    let blocks: usize = files.values().map(|(s, _)| s.len()).sum();
    format!(
        "unsafe inventory {state}: {blocks} blocks and impls in {} files, each argued ({PAGE})",
        files.len()
    )
}

/// The page's `updated` date and its body after the frontmatter, if it has one.
fn split_page(page: &str) -> (Option<String>, Option<&str>) {
    let Some(rest) = page.strip_prefix("---\n") else {
        return (None, None);
    };
    let Some(end) = rest.find("\n---\n") else {
        return (None, None);
    };
    let fm = rest.get(..end).unwrap_or_default();
    let date = fm
        .lines()
        .find_map(|l| l.strip_prefix("updated: "))
        .map(str::to_owned);
    let body = rest
        .get(end + 5..)
        .map(|b| b.strip_prefix('\n').unwrap_or(b));
    (date, body)
}

fn source_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for group in ["crates", "bindings"] {
        let Ok(entries) = fs::read_dir(root.join(group)) else {
            continue;
        };
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path().join("src"))
            .filter(|p| p.is_dir())
            .collect();
        found.sort();
        dirs.extend(found);
    }
    dirs
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The code of a line: comments and string literals blanked, so `unsafe` in either is not
/// counted. Line-based: a block comment or raw string spanning lines is not tracked, which
/// the shipped sources do not use around `unsafe`.
fn code_of(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut in_str = false;
    while let Some(c) = chars.next() {
        if in_str {
            match c {
                '\\' => {
                    chars.next();
                }
                '"' => in_str = false,
                _ => {}
            }
            out.push(' ');
        } else if c == '"' {
            in_str = true;
            out.push(' ');
        } else if c == '/' && chars.peek() == Some(&'/') {
            break;
        } else {
            out.push(c);
        }
    }
    out
}

/// Each `unsafe` block and impl with its argument, and the count of `unsafe fn` (whose
/// contract is their `# Safety` doc or their module's, checked by clippy's
/// `missing_safety_doc`).
fn scan(text: &str) -> (Vec<Site>, usize) {
    let lines: Vec<&str> = text.lines().collect();
    let mut sites = Vec::new();
    let mut fns = 0;
    for (i, line) in lines.iter().enumerate() {
        let code = code_of(line);
        let mut rest = code.as_str();
        while let Some(at) = find_word(rest, "unsafe") {
            let after = rest.get(at + 6..).unwrap_or_default();
            let next = after.trim_start();
            let what = if next.starts_with('{') {
                Some("block")
            } else if next.starts_with("impl") {
                Some("unsafe impl")
            } else {
                if next.starts_with("fn") || next.starts_with("extern") {
                    fns += 1;
                }
                None
            };
            if let Some(what) = what {
                sites.push(Site {
                    line: i + 1,
                    what,
                    argument: argument(&lines, i),
                });
            }
            rest = after;
        }
    }
    (sites, fns)
}

/// `word` in `s` as a whole word.
fn find_word(s: &str, word: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(at) = s.get(from..)?.find(word).map(|a| a + from) {
        let before = s.get(..at).and_then(|b| b.chars().next_back());
        let after = s.get(at + word.len()..).and_then(|a| a.chars().next());
        let edge = |c: Option<char>| c.is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        if edge(before) && edge(after) {
            return Some(at);
        }
        from = at + word.len();
    }
    None
}

/// The argument of the `unsafe` on line `i`: the comment directly above it (or ending the
/// line) that opens with the safety marker, with the reason of the
/// `#[allow(unsafe_code, reason = ...)]` just above that comment when there is one.
fn argument(lines: &[&str], i: usize) -> String {
    let mut comment = Vec::new();
    if let Some(at) = lines.get(i).and_then(|l| l.find("// SAFETY:")) {
        comment.push(
            lines
                .get(i)
                .and_then(|l| l.get(at + 3..))
                .unwrap_or_default()
                .trim(),
        );
    }
    let mut j = i;
    // A statement rustfmt wrapped (`let x =` then `unsafe {` on the next line): the comment
    // is above the statement, as clippy reads it.
    while j > 0
        && lines
            .get(j - 1)
            .is_some_and(|l| !l.trim().starts_with("//") && code_of(l).trim_end().ends_with('='))
    {
        j -= 1;
    }
    let same_line = !comment.is_empty();
    while !same_line && j > 0 {
        j -= 1;
        let l = lines.get(j).map_or("", |l| l.trim());
        if let Some(c) = l.strip_prefix("//") {
            comment.push(c.trim());
            if c.trim_start().starts_with("SAFETY") {
                break;
            }
        } else {
            break;
        }
    }
    let text = comment.iter().rev().copied().collect::<Vec<_>>().join(" ");
    let Some(at) = text.find("SAFETY") else {
        return String::new();
    };
    let mut argument = text
        .get(at..)
        .unwrap_or_default()
        .trim_start_matches("SAFETY")
        .trim_start_matches(|c: char| c == ':' || c.is_whitespace())
        .to_owned();
    // The allow attribute ending directly above the comment.
    let mut top = j;
    if lines
        .get(j.wrapping_sub(1))
        .is_some_and(|l| l.trim().ends_with(")]"))
    {
        while top > 0 && j - top < 12 {
            top -= 1;
            if lines.get(top).is_some_and(|l| l.trim().starts_with("#[")) {
                break;
            }
        }
    }
    let attr: Vec<&str> = lines
        .get(top..j)
        .unwrap_or_default()
        .iter()
        .map(|l| l.trim())
        .collect();
    let attr = attr.join(" ");
    if let Some(r) = attr.find("reason = \"") {
        let reason = attr.get(r + 10..).unwrap_or_default();
        let end = reason.find('"').unwrap_or(reason.len());
        let reason = reason.get(..end).unwrap_or_default().replace("\\ ", " ");
        let reason = reason.split_whitespace().collect::<Vec<_>>().join(" ");
        if !reason.is_empty() && attr.contains("unsafe_code") {
            let _ = write!(argument, " (allowed: {reason})");
        }
    }
    argument
}

fn render(files: &BTreeMap<String, (Vec<Site>, usize)>) -> String {
    let blocks: usize = files.values().map(|(s, _)| s.len()).sum();
    let fns: usize = files.values().map(|(_, f)| f).sum();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# The unsafe inventory\n\n\
         Generated by `cargo xtask unsafe-audit` from the shipped sources; do not edit. \
         fast-check runs `--check`, so this page is always the code's. The trust boundaries \
         these blocks sit on are in the [threat model](threat-model.md).\n\n\
         **{blocks} blocks and impls in {} files**, each with its `SAFETY:` argument; \
         **{fns} `unsafe fn`** (the C ABI's exports, JNI's entry points), whose contract is \
         their `# Safety` doc or their module's. Everything else in the workspace keeps \
         `forbid(unsafe_code)`.\n",
        files.len()
    );
    out.push_str("| File | Blocks | `unsafe fn` | Reached by |\n|---|---|---|---|\n");
    for (file, (sites, f)) in files {
        let evidence = EVIDENCE
            .iter()
            .find(|(e, _)| e == file)
            .map_or("", |(_, ev)| ev);
        let _ = writeln!(out, "| `{file}` | {} | {f} | {evidence} |", sites.len());
    }
    for (file, (sites, _)) in files {
        if sites.is_empty() {
            continue;
        }
        let _ = writeln!(out, "\n## `{file}`\n");
        let mut groups: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
        for s in sites {
            groups
                .entry((s.what, &s.argument))
                .or_default()
                .push(s.line);
        }
        let mut groups: Vec<_> = groups.into_iter().collect();
        groups.sort_by_key(|(_, lines)| lines.first().copied());
        if GENERATED.contains(&file.as_str()) {
            out.push_str(
                "Generated by `cargo xtask bindings`; the generator writes each argument.\n\n",
            );
        }
        out.push_str("| Lines | What | Argument |\n|---|---|---|\n");
        for ((what, arg), lines) in groups {
            let lines: Vec<String> = lines.iter().map(usize::to_string).collect();
            let _ = writeln!(out, "| {} | {what} | {} |", lines.join(", "), cell(arg));
        }
    }
    out
}

/// Text safe inside a Markdown table cell.
fn cell(s: &str) -> String {
    s.replace('|', "\\|")
}

/// Today's UTC date, `YYYY-MM-DD` (civil from days, H. Hinnant's algorithm).
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_blocks_impls_and_their_arguments() {
        let src = r#"
#[allow(
    unsafe_code,
    reason = "B13: the file never changes \
              while mapped"
)]
// SAFETY: see the reason above.
let map = unsafe { map(file) }?;
// SAFETY: two lines
// of argument.
unsafe impl Send for X {}
let s = "unsafe { in a string }"; // unsafe { in a comment }
pub unsafe extern "C" fn tg_x() {}
unsafe fn y() {}
let not_unsafe_word = 1;
"#;
        let (sites, fns) = scan(src);
        assert_eq!(fns, 2);
        assert_eq!(sites.len(), 2, "{sites:?}");
        assert_eq!(sites[0].what, "block");
        assert_eq!(
            sites[0].argument,
            "see the reason above. (allowed: B13: the file never changes while mapped)"
        );
        assert_eq!(sites[1].what, "unsafe impl");
        assert_eq!(sites[1].argument, "two lines of argument.");
    }

    /// The planted fault: a block with no `SAFETY:` comment, or one whose comment is not
    /// directly above it, has no argument, which `run` reports.
    #[test]
    fn a_block_without_its_comment_has_no_argument() {
        let (sites, _) = scan("fn f() {\n    let x = unsafe { g() };\n}\n");
        assert_eq!(sites[0].argument, "");
        let (sites, _) = scan("// SAFETY: far away\nlet a = 1;\nlet x = unsafe { g() };\n");
        assert_eq!(sites[0].argument, "");
        let (sites, _) = scan("// A plain comment.\nlet x = unsafe { g() };\n");
        assert_eq!(sites[0].argument, "");
    }

    #[test]
    fn the_page_body_survives_its_frontmatter() {
        let page = "---\ntitle: \"T\"\nstatus: accepted\nupdated: 2026-11-15\n---\n\n# Body\n";
        assert_eq!(
            split_page(page),
            (Some("2026-11-15".into()), Some("# Body\n"))
        );
        assert_eq!(split_page("no frontmatter"), (None, None));
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_407), (2025, 11, 15));
        assert_eq!(civil(11_016), (2000, 2, 29));
    }

    /// The repository itself: every block argued, every file in EVIDENCE, the page current.
    #[test]
    fn the_repository_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        run(root, &["--check".into()]).unwrap();
    }
}
