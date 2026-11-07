//! `bindings [--check]`: extracts the IDL from `teigeodata-api` and generates every binding
//! layer from it with the templates in `xtask/templates/` (spike S4, ADR-0016).
//!
//! Generated files are checked in. `--check` renders everything in memory and fails on any
//! difference, so a change to the API that was not regenerated, or a hand edit to a generated
//! file, fails CI.

mod idl;

use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use minijinja::syntax::SyntaxConfig;
use minijinja::value::Serde;
use minijinja::{Environment, UndefinedBehavior, Value};

/// A generated file: its template and where it goes. Rust outputs are passed through rustfmt
/// so that `cargo fmt --check` holds for them too.
struct Target {
    template: &'static str,
    output: &'static str,
    rustfmt: bool,
}

const TARGETS: &[Target] = &[
    Target {
        template: "ffi.rs.j2",
        output: "crates/teigeodata-ffi/src/generated.rs",
        rustfmt: true,
    },
    Target {
        template: "c.h.j2",
        output: "bindings/c/teigeodata.h",
        rustfmt: false,
    },
    Target {
        template: "python.rs.j2",
        output: "bindings/python/src/lib.rs",
        rustfmt: true,
    },
    Target {
        template: "python.pyi.j2",
        output: "bindings/python/teigeodata/_native.pyi",
        rustfmt: false,
    },
    Target {
        template: "node.rs.j2",
        output: "bindings/node/src/lib.rs",
        rustfmt: true,
    },
    Target {
        template: "node.d.ts.j2",
        output: "bindings/node/index.d.ts",
        rustfmt: false,
    },
];

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
            .and_then(|text| {
                if t.rustfmt {
                    rustfmt(root, &text)
                } else {
                    Ok(text)
                }
            });
        match rendered {
            Ok(text) => files.push((t.output, text)),
            Err(e) => problems.push(e),
        }
    }
    if !problems.is_empty() {
        return Err(problems);
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
        (false, false) => Ok(format!(
            "bindings: wrote {} of {n} files ({fns} functions): {}",
            changed.len(),
            changed.join(", ")
        )),
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

/// Formats Rust source through `rustfmt` on stdin, with the repository's rustfmt.toml.
fn rustfmt(root: &Path, text: &str) -> Result<String, String> {
    let mut child = Command::new("rustfmt")
        .arg("--config-path")
        .arg(root.join("rustfmt.toml"))
        .args(["--emit", "stdout", "--quiet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("rustfmt: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("rustfmt: no stdin")?
        .write_all(text.as_bytes())
        .map_err(|e| format!("rustfmt: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("rustfmt: {e}"))?;
    if out.status.success() {
        String::from_utf8(out.stdout).map_err(|e| format!("rustfmt: {e}"))
    } else {
        Err(format!("rustfmt: {}", String::from_utf8_lossy(&out.stderr)))
    }
}

#[cfg(test)]
mod tests {
    use super::{camel, pascal, screaming};

    #[test]
    fn cases() {
        assert_eq!(camel("sample_rows"), "sampleRows");
        assert_eq!(pascal("sample_rows"), "SampleRows");
        assert_eq!(pascal("InvalidArgument"), "InvalidArgument");
        assert_eq!(screaming("InvalidArgument"), "INVALID_ARGUMENT");
    }
}
