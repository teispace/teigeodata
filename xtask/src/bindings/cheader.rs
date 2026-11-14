//! The header cross-check: the declarations cbindgen reads from the compiled ABI crate must
//! equal those in the header we generate from the IDL. Both come from the same API, by
//! different routes, so a template bug that changes a signature shows up as a difference.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

/// Runs cbindgen on the ABI crate and compares its declarations with `ours`.
pub(crate) fn check(root: &Path, ours: &str) -> Result<(), Vec<String>> {
    let crate_dir = root.join("crates/teigeodata-ffi");
    let out = Command::new("cbindgen")
        .arg("--config")
        .arg(crate_dir.join("cbindgen.toml"))
        .args(["--crate", "teigeodata-ffi", "--quiet"])
        .arg(&crate_dir)
        .output()
        .map_err(|e| {
            vec![format!(
                "cbindgen: {e}; install it with `cargo install cbindgen --version 0.29.4 --locked`"
            )]
        })?;
    if !out.status.success() {
        return Err(vec![format!(
            "cbindgen: {}",
            String::from_utf8_lossy(&out.stderr)
        )]);
    }
    let theirs = String::from_utf8_lossy(&out.stdout);
    compare(&declarations(ours), &declarations(&theirs))
}

fn compare(
    ours: &BTreeMap<String, String>,
    theirs: &BTreeMap<String, String>,
) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    for (name, decl) in theirs {
        match ours.get(name) {
            Some(d) if d == decl => {}
            Some(d) => problems.push(format!(
                "teigeodata.h declares `{d}` but the library has `{decl}`"
            )),
            None => problems.push(format!("teigeodata.h lacks `{decl}`")),
        }
    }
    for (name, decl) in ours {
        if !theirs.contains_key(name) {
            problems.push(format!(
                "teigeodata.h declares `{decl}`, absent from the library"
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Every `tg_` function, `tg_callback_` typedef and `TG_` constant, keyed by name, as one
/// normalised line: comments dropped, whitespace collapsed, Rust type names mapped to C.
fn declarations(header: &str) -> BTreeMap<String, String> {
    let mut text = String::with_capacity(header.len());
    let mut rest = header;
    // Drop comments.
    while let Some(start) = rest.find("/*") {
        text.push_str(rest.get(..start).unwrap_or_default());
        rest = rest
            .get(start..)
            .and_then(|r| r.find("*/").and_then(|end| r.get(end + 2..)))
            .unwrap_or_default();
    }
    text.push_str(rest);
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if let ["#define", name, value] = words.as_slice()
            && name.starts_with("TG_")
        {
            let value = value.trim_end_matches(['u', 'l']);
            out.insert((*name).to_owned(), format!("#define {name} {value}"));
        }
    }
    for stmt in text.split(';') {
        let stmt = normalise(stmt);
        let Some(open) = stmt.find('(') else {
            continue;
        };
        // A function-pointer typedef is named inside `(*name)`; a function before its `(`.
        let name = match stmt.find("(*") {
            Some(star) if star == open => stmt
                .get(star + 2..)
                .and_then(|t| t.split(')').next())
                .unwrap_or_default(),
            _ => stmt
                .get(..open)
                .unwrap_or_default()
                .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
                .find(|s| !s.is_empty())
                .unwrap_or_default(),
        };
        if name.starts_with("tg_") && !stmt.contains('{') {
            out.insert(name.to_owned(), stmt);
        }
    }
    out
}

/// One line, single spaces, `T *x` pointer style, and cbindgen's Rust names in C form.
fn normalise(stmt: &str) -> String {
    let joined = stmt
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut s = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    for (from, to) in [
        ("( ", "("),
        (" )", ")"),
        (" ,", ","),
        ("* ", "*"),
        (" *", " *"),
    ] {
        s = s.replace(from, to);
    }
    s = s.replace("Ctx ", "void *");
    // cbindgen names our Rust types (`TgBytes`, a handle's `TgEngine`); the header uses their
    // C names (`tg_bytes`, `tg_engine`). Callback types are mapped below.
    let mut from = 0;
    while let Some(at) = s.get(from..).and_then(|t| t.find("Tg")).map(|i| i + from) {
        let tail = s.get(at + 2..).unwrap_or_default();
        let id: String = tail
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect();
        let starts_word = at == 0
            || !s
                .get(..at)
                .is_some_and(|h| h.ends_with(|c: char| c.is_alphanumeric() || c == '_'));
        if id.starts_with("Callback") || id.is_empty() || !starts_word {
            from = at + 2;
            continue;
        }
        let snake = id.chars().fold(String::new(), |mut acc, c| {
            if c.is_uppercase() && !acc.is_empty() {
                acc.push('_');
            }
            acc.extend(c.to_lowercase());
            acc
        });
        s = format!(
            "{}tg_{snake}{}",
            s.get(..at).unwrap_or_default(),
            tail.get(id.len()..).unwrap_or_default()
        );
        from = at + 3 + snake.len();
    }
    s = s.replace("(void)", "()").replace("()", "(void)");
    while let Some(at) = s.find("TgCallback") {
        let tail = s.get(at + "TgCallback".len()..).unwrap_or_default();
        let id: String = tail
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect();
        let snake = id.chars().fold(String::new(), |mut acc, c| {
            if c.is_uppercase() && !acc.is_empty() {
                acc.push('_');
            }
            acc.extend(c.to_lowercase());
            acc
        });
        s = format!(
            "{}tg_callback_{snake}{}",
            s.get(..at).unwrap_or_default(),
            tail.get(id.len()..).unwrap_or_default()
        );
    }
    s.trim().trim_start_matches("typedef ").to_owned()
}

#[cfg(test)]
mod tests {
    use super::{compare, declarations};

    const OURS: &str = "/* doc */\n#define TG_MAX 10u\ntypedef bool (*tg_callback_u32)(void *ctx, uint32_t a0);\nint32_t tg_f(const uint8_t *text, size_t text_len,\n    tg_bytes *out);\nint32_t tg_noop(void);\n";
    const THEIRS: &str = "#define TG_MAX 10\ntypedef bool (*TgCallbackU32)(Ctx ctx, uint32_t a0);\n/**\n * doc\n */\nint32_t tg_f(const uint8_t *text, size_t text_len, TgBytes *out);\nint32_t tg_noop(void);\n";

    #[test]
    fn same_declarations_by_two_routes_match() {
        let (a, b) = (declarations(OURS), declarations(THEIRS));
        assert_eq!(a.len(), 4, "{a:?}");
        assert_eq!(compare(&a, &b), Ok(()));
    }

    #[test]
    fn a_changed_signature_is_reported() {
        let ours = OURS.replace("size_t text_len", "uint32_t text_len");
        assert!(compare(&declarations(&ours), &declarations(THEIRS)).is_err());
        let missing = THEIRS.replace("int32_t tg_noop(void);\n", "");
        assert!(compare(&declarations(OURS), &declarations(&missing)).is_err());
        let value = OURS.replace("TG_MAX 10u", "TG_MAX 11u");
        assert!(compare(&declarations(&value), &declarations(THEIRS)).is_err());
    }
}
