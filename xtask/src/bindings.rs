//! `bindings [--check]`: extracts the IDL from `teigeodata-api` and generates every binding
//! layer from it with the templates in `xtask/templates/` (spike S4, ADR-0016).
//!
//! Generated files are checked in. `--check` renders everything in memory and fails on any
//! difference, so a change to the API that was not regenerated, or a hand edit to a generated
//! file, fails CI. It also compares the header with cbindgen's reading of the ABI crate.

pub(crate) mod cheader;
mod idl;

use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use minijinja::syntax::SyntaxConfig;
use minijinja::value::Serde;
use minijinja::{Environment, UndefinedBehavior, Value};

/// A generated file: its template, where it goes and its formatter. Rust and Go outputs are
/// passed through rustfmt and gofmt, so that `cargo fmt --check` and `gofmt -l` hold for them;
/// C# only has its runs of blank lines folded (its formatter needs the .NET SDK).
struct Target {
    template: &'static str,
    output: &'static str,
    fmt: Fmt,
}

/// The formatter a generated file goes through.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fmt {
    None,
    Rust,
    Go,
    Blank,
}

const TARGETS: &[Target] = &[
    Target {
        template: "ffi.rs.j2",
        output: "crates/teigeodata-ffi/src/generated.rs",
        fmt: Fmt::Rust,
    },
    Target {
        template: "c.h.j2",
        output: HEADER_OUTPUT,
        fmt: Fmt::None,
    },
    Target {
        template: "cpp.hpp.j2",
        output: "bindings/c/teigeodata.hpp",
        fmt: Fmt::None,
    },
    Target {
        template: "python.rs.j2",
        output: "bindings/python/src/lib.rs",
        fmt: Fmt::Rust,
    },
    Target {
        template: "python.pyi.j2",
        output: "bindings/python/teigeodata/_native.pyi",
        fmt: Fmt::None,
    },
    Target {
        template: "node.rs.j2",
        output: "bindings/node/src/lib.rs",
        fmt: Fmt::Rust,
    },
    Target {
        template: "node.d.ts.j2",
        output: "bindings/node/index.d.ts",
        fmt: Fmt::None,
    },
    Target {
        template: "node.index.js.j2",
        output: "bindings/node/index.js",
        fmt: Fmt::None,
    },
    Target {
        template: "node.index.mjs.j2",
        output: "bindings/node/index.mjs",
        fmt: Fmt::None,
    },
    Target {
        template: "node.reader.js.j2",
        output: "bindings/node/reader.js",
        fmt: Fmt::None,
    },
    Target {
        template: "wasm.reader.js.j2",
        output: "bindings/wasm/js/reader.js",
        fmt: Fmt::None,
    },
    Target {
        template: "wasm.rs.j2",
        output: "bindings/wasm/src/lib.rs",
        fmt: Fmt::Rust,
    },
    Target {
        template: "wasm.index.js.j2",
        output: "bindings/wasm/js/index.js",
        fmt: Fmt::None,
    },
    Target {
        template: "wasm.index.d.ts.j2",
        output: "bindings/wasm/js/index.d.ts",
        fmt: Fmt::None,
    },
    Target {
        template: "jni.rs.j2",
        output: "bindings/jvm/src/lib.rs",
        fmt: Fmt::Rust,
    },
    Target {
        template: "java.j2",
        output: "bindings/jvm/java/org/teispace/teigeodata/TeiGeodata.java",
        fmt: Fmt::None,
    },
    Target {
        template: "java-exceptions.j2",
        output: "bindings/jvm/java/org/teispace/teigeodata/TeiGeodataException.java",
        fmt: Fmt::None,
    },
    Target {
        template: "swift.j2",
        output: "bindings/swift/Sources/TeiGeodata/TeiGeodata.swift",
        fmt: Fmt::None,
    },
    Target {
        template: "dart.j2",
        output: "bindings/dart/lib/src/bindings.g.dart",
        fmt: Fmt::None,
    },
    Target {
        template: "dart_types.j2",
        output: "bindings/dart/lib/src/types.g.dart",
        fmt: Fmt::None,
    },
    Target {
        template: "dart_web.j2",
        output: "bindings/dart/lib/src/bindings_web.g.dart",
        fmt: Fmt::None,
    },
    Target {
        template: "go.j2",
        output: "bindings/go/teigeodata.go",
        fmt: Fmt::Go,
    },
    Target {
        template: "go_cgo.go.j2",
        output: "bindings/go/raw_cgo.go",
        fmt: Fmt::Go,
    },
    Target {
        template: "go_cgo.c.j2",
        output: "bindings/go/raw_cgo.c",
        fmt: Fmt::None,
    },
    Target {
        template: "go_purego.go.j2",
        output: "bindings/go/raw_purego.go",
        fmt: Fmt::Go,
    },
    Target {
        template: "cs.j2",
        output: "bindings/dotnet/TeiGeodata/Generated.cs",
        fmt: Fmt::Blank,
    },
    Target {
        template: "reference.index.mdx.j2",
        output: "site/content/docs/reference/index.mdx",
        fmt: Fmt::None,
    },
    Target {
        template: "reference.engine.mdx.j2",
        output: "site/content/docs/reference/engine.mdx",
        fmt: Fmt::None,
    },
    Target {
        template: "reference.store.mdx.j2",
        output: "site/content/docs/reference/store.mdx",
        fmt: Fmt::None,
    },
    Target {
        template: "reference.functions.mdx.j2",
        output: "site/content/docs/reference/functions.mdx",
        fmt: Fmt::None,
    },
    Target {
        template: "reference.types.mdx.j2",
        output: "site/content/docs/reference/types.mdx",
        fmt: Fmt::None,
    },
];

const HEADER_OUTPUT: &str = "bindings/c/teigeodata.h";
const IDL_OUTPUT: &str = "bindings/idl.json";
const API_SOURCE: &str = "crates/teigeodata-api/src/lib.rs";

/// Runs the task: writes changed files, or with `check` reports the ones that differ.
pub(crate) fn run(root: &Path, check: bool) -> Result<String, Vec<String>> {
    let source = fs::read_to_string(root.join(API_SOURCE))
        .map_err(|e| vec![format!("{API_SOURCE}: {e}")])?;
    let idl = idl::extract(&source)?;
    let mut files = vec![(IDL_OUTPUT, render_idl(&idl)?)];
    let env = environment(root).map_err(|e| vec![e])?;
    let ctx = Value::from(Serde(&idl));
    let mut problems = Vec::new();
    for t in TARGETS {
        let rendered = env
            .get_template(t.template)
            .and_then(|tpl| tpl.render(&ctx))
            .map_err(|e| format!("{}: {e:#}", t.template))
            .and_then(|text| match t.fmt {
                Fmt::None => Ok(text),
                Fmt::Rust => {
                    let config = root.join("rustfmt.toml");
                    let config = config.to_string_lossy();
                    format(
                        "rustfmt",
                        &["--config-path", &config, "--emit", "stdout", "--quiet"],
                        &text,
                    )
                }
                Fmt::Go => format("gofmt", &[], &text),
                Fmt::Blank => Ok(fold_blank_lines(&text)),
            });
        match rendered {
            Ok(text) => files.push((t.output, text)),
            Err(e) => problems.push(e),
        }
    }
    match fs::read_to_string(root.join("bindings/parity.json"))
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(parity) => match crate::parity::header(&parity) {
            Ok(text) => files.push((crate::parity::HEADER, text)),
            Err(e) => problems.push(e),
        },
        Err(e) => problems.push(format!("bindings/parity.json: {e}")),
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    if check && let Some((_, header)) = files.iter().find(|(rel, _)| *rel == HEADER_OUTPUT) {
        cheader::check(root, header)?;
    }
    let mut changed = Vec::new();
    for (rel, text) in &files {
        let path = root.join(rel);
        if fs::read_to_string(&path).ok().as_deref() == Some(text.as_str()) {
            continue;
        }
        changed.push(*rel);
        if !check {
            let written = path
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| fs::write(&path, text));
            written.map_err(|e| vec![format!("{rel}: {e}")])?;
        }
    }
    let (n, fns) = (files.len(), idl.functions.len());
    match (check, changed.is_empty()) {
        (_, true) => Ok(format!(
            "bindings ok: {n} files up to date ({fns} functions)"
        )),
        (true, false) => Err(changed
            .iter()
            .map(|f| format!("{f} is out of date: run `cargo xtask bindings`"))
            .collect()),
        (false, false) => {
            // Generated code moves the `unsafe` blocks the inventory lists by line.
            let audit = crate::unsafe_audit::run(root, &[])?;
            Ok(format!(
                "bindings: wrote {} of {n} files ({fns} functions): {}\n{audit}",
                changed.len(),
                changed.join(", ")
            ))
        }
    }
}

fn render_idl(idl: &idl::Idl) -> Result<String, Vec<String>> {
    serde_json::to_string_pretty(idl)
        .map(|s| s + "\n")
        .map_err(|e| vec![format!("{IDL_OUTPUT}: {e}")])
}

/// `snake_case` to `camelCase`.
fn camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut up = false;
    for c in s.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// `snake_case` or `CamelCase` to `PascalCase`.
fn pascal(s: &str) -> String {
    let c = camel(s);
    let mut chars = c.chars();
    chars
        .next()
        .map_or_else(String::new, |f| f.to_uppercase().chain(chars).collect())
}

/// `SCREAMING_SNAKE` or `CamelCase` to `lowerCamelCase`: `SAMPLE_ROWS_MAX` → `sampleRowsMax`,
/// `InvalidArgument` → `invalidArgument`.
fn camel_lower(s: &str) -> String {
    let snake = if s.contains('_') || s.chars().all(|c| !c.is_lowercase()) {
        s.to_lowercase()
    } else {
        let mut c = s.chars();
        c.next()
            .map_or_else(String::new, |f| f.to_lowercase().chain(c).collect())
    };
    camel(&snake)
}

/// `CamelCase` to `snake_case`.
fn snake(s: &str) -> String {
    screaming(s).to_lowercase()
}

/// `CamelCase` to `SCREAMING_SNAKE_CASE`.
fn screaming(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(c.to_uppercase());
    }
    out
}

/// A doc comment's lines as MDX for the reference pages: paragraphs joined, `- ` items kept
/// as a list, a `# Errors` heading turned into a bold lead-in, decision references such as
/// `(D-141, D-274)` dropped (they point into the repository, not the site), and `<`, `{` and
/// `}` escaped outside code spans so MDX reads them as text.
fn mdx(lines: &[String]) -> String {
    let mut blocks: Vec<String> = Vec::new();
    let mut block: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut lead: Option<String> = None;
    let end_block = |block: &mut Vec<String>, cur: &mut String, blocks: &mut Vec<String>| {
        if !cur.is_empty() {
            block.push(std::mem::take(cur));
        }
        if !block.is_empty() {
            blocks.push(block.join("\n"));
            block.clear();
        }
    };
    for line in lines {
        let t = line.trim_end();
        if t.is_empty() {
            end_block(&mut block, &mut cur, &mut blocks);
        } else if let Some(h) = t.strip_prefix("# ") {
            end_block(&mut block, &mut cur, &mut blocks);
            lead = Some(format!("**{h}.** "));
        } else if t.starts_with("- ") {
            if !cur.is_empty() {
                block.push(std::mem::take(&mut cur));
            }
            cur.push_str(t);
        } else {
            if cur.is_empty() {
                if let Some(l) = lead.take() {
                    cur.push_str(&l);
                }
            } else {
                cur.push(' ');
            }
            cur.push_str(t.trim_start());
        }
    }
    end_block(&mut block, &mut cur, &mut blocks);
    escape_mdx(&drop_decisions(&blocks.join("\n\n")))
}

/// The first sentence of a doc comment, as MDX: what an index row says about an entry.
fn summary(lines: &[String]) -> String {
    let first = mdx(lines);
    let para = first.split("\n\n").next().unwrap_or_default();
    let mut in_code = false;
    for (i, c) in para.char_indices() {
        match c {
            '`' => in_code = !in_code,
            '.' if !in_code && (i + 1 == para.len() || para[i + 1..].starts_with(' ')) => {
                return para[..=i].to_owned();
            }
            _ => {}
        }
    }
    para.to_owned()
}

/// Drops ` (D-141, D-274)`: a parenthesis that holds nothing but decision IDs.
fn drop_decisions(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("(D-") {
        let close = rest[at..].find(')').map(|c| at + c);
        let inner = close.map(|c| &rest[at + 1..c]);
        let only_ids = inner.is_some_and(|s| {
            s.split(", ").all(|id| {
                id.strip_prefix("D-")
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            })
        });
        match close {
            Some(c) if only_ids => {
                out.push_str(rest[..at].strip_suffix(' ').unwrap_or(&rest[..at]));
                rest = &rest[c + 1..];
            }
            _ => {
                out.push_str(&rest[..at + 3]);
                rest = &rest[at + 3..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Escapes `<`, `{` and `}` outside code spans.
pub(crate) fn escape_mdx(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut in_code = false;
    for c in text.chars() {
        match c {
            '`' => {
                in_code = !in_code;
                out.push(c);
            }
            '<' if !in_code => out.push_str("&lt;"),
            '{' | '}' if !in_code => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

fn environment(root: &Path) -> Result<Environment<'static>, String> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    let syntax = SyntaxConfig::builder()
        .trim_blocks(true)
        .lstrip_blocks(true)
        .keep_trailing_newline(true)
        .build()
        .map_err(|e| format!("{e:#}"))?;
    env.set_syntax(syntax);
    env.add_filter("camel", |s: &str| camel(s));
    env.add_filter("pascal", |s: &str| pascal(s));
    env.add_filter("screaming", |s: &str| screaming(s));
    env.add_filter("snake", |s: &str| snake(s));
    env.add_filter("camel_lower", |s: &str| camel_lower(s));
    env.add_filter("mdx", |lines: Vec<String>| mdx(&lines));
    env.add_filter("summary", |lines: Vec<String>| summary(&lines));
    env.add_filter("quoted", |s: &str| {
        serde_json::to_string(s).unwrap_or_default()
    });
    let dir = root.join("xtask/templates");
    let entries = fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if Path::new(&name).extension().is_some_and(|x| x == "j2") {
            let text = fs::read_to_string(entry.path()).map_err(|e| format!("{name}: {e}"))?;
            env.add_template_owned(name, text)
                .map_err(|e| format!("{e:#}"))?;
        }
    }
    Ok(env)
}

/// Formats source through `tool` (rustfmt or gofmt) on stdin, returning its stdout.
/// Folds each run of blank lines into one, and drops a blank line before a closing brace.
fn fold_blank_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = false;
    for line in text.lines() {
        if line.trim().is_empty() {
            blank = true;
            continue;
        }
        if blank && !out.is_empty() && !line.trim_start().starts_with('}') {
            out.push('\n');
        }
        blank = false;
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn format(tool: &str, args: &[&str], text: &str) -> Result<String, String> {
    let mut child = Command::new(tool)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{tool}: {e}"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| format!("{tool}: no stdin"))?
        .write_all(text.as_bytes())
        .map_err(|e| format!("{tool}: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("{tool}: {e}"))?;
    if out.status.success() {
        String::from_utf8(out.stdout).map_err(|e| format!("{tool}: {e}"))
    } else {
        Err(format!("{tool}: {}", String::from_utf8_lossy(&out.stderr)))
    }
}

#[cfg(test)]
mod tests {
    use super::{camel, camel_lower, fold_blank_lines, pascal, screaming};

    #[test]
    fn blank_lines() {
        assert_eq!(fold_blank_lines("a\n\n\n\nb\n\n}\n"), "a\n\nb\n}\n");
        assert_eq!(fold_blank_lines("\n\na\n"), "a\n");
    }

    #[test]
    fn cases() {
        assert_eq!(camel("sample_rows"), "sampleRows");
        assert_eq!(pascal("sample_rows"), "SampleRows");
        assert_eq!(pascal("InvalidArgument"), "InvalidArgument");
        assert_eq!(screaming("InvalidArgument"), "INVALID_ARGUMENT");
        assert_eq!(super::snake("PlaceEngine"), "place_engine");
        assert_eq!(camel_lower("SAMPLE_ROWS_MAX"), "sampleRowsMax");
        assert_eq!(camel_lower("InvalidArgument"), "invalidArgument");
        assert_eq!(camel_lower("MAX"), "max");
    }

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }

    #[test]
    fn doc_to_mdx() {
        let doc = lines(
            "Open packs (D-141, D-274). Every method\ntakes `&self` and a `Vec<u8>`.\n\n# Errors\n\n`Corrupt` if x < 1 {no}.\n\nRows:\n- `a`: one,\n  two;\n- `b` (D-9x).",
        );
        assert_eq!(
            super::mdx(&doc),
            "Open packs. Every method takes `&self` and a `Vec<u8>`.\n\n**Errors.** `Corrupt` if x &lt; 1 \\{no\\}.\n\nRows:\n- `a`: one, two;\n- `b` (D-9x)."
        );
        assert_eq!(super::summary(&doc), "Open packs.");
        assert_eq!(
            super::summary(&lines("Uses `a.b` here. Then")),
            "Uses `a.b` here."
        );
        assert_eq!(super::summary(&lines("No stop")), "No stop");
    }
}
