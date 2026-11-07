//! `check-workspace`: keeps every lint table equal to the main workspace's.
//!
//! - The forge is a separate Cargo workspace (ADR-0001), so it cannot inherit the main table;
//!   its `[workspace.lints]` must be identical.
//! - Boundary crates (the C ABI crate and the `bindings/` workspace of language shims) need
//!   `unsafe` in their audited, generated modules. Their table must be the main one with a
//!   single change: `unsafe_code` lowered from `forbid` to `deny`, so a module can allow it
//!   with a reason (and nothing else can).

use std::fs;
use std::path::Path;

/// Where each copy of the lint table lives, and whether it is a boundary table.
const TABLES: &[(&str, &[&str], bool)] = &[
    ("forge/Cargo.toml", &["workspace", "lints"], false),
    ("bindings/Cargo.toml", &["workspace", "lints"], true),
    ("crates/teigeodata-ffi/Cargo.toml", &["lints"], true),
];

pub(crate) fn check(root: &Path) -> Result<String, Vec<String>> {
    let read = |rel: &str| -> Result<toml::Table, String> {
        let text = fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
        toml::from_str(&text).map_err(|e| format!("{rel}: {e}"))
    };
    let main = read("Cargo.toml").map_err(|e| vec![e])?;
    let main = at(&main, &["workspace", "lints"])
        .ok_or_else(|| vec!["Cargo.toml needs a [workspace.lints] table".to_owned()])?
        .clone();
    let mut problems = Vec::new();
    for (rel, path, boundary) in TABLES {
        match read(rel) {
            Ok(t) => {
                if let Err(p) = compare(&main, at(&t, path), *boundary) {
                    problems.push(format!("{rel}: {p}"));
                }
            }
            Err(e) => problems.push(e),
        }
    }
    if problems.is_empty() {
        Ok(format!(
            "workspace ok: {} lint tables match the main workspace",
            TABLES.len()
        ))
    } else {
        Err(problems)
    }
}

fn at<'a>(t: &'a toml::Table, path: &[&str]) -> Option<&'a toml::Value> {
    let (first, rest) = path.split_first()?;
    rest.iter().try_fold(t.get(*first)?, |v, k| v.get(k))
}

/// The main table as a boundary crate must copy it.
fn boundary(main: &toml::Value) -> Option<toml::Value> {
    let mut expected = main.clone();
    let rust = expected.get_mut("rust")?.as_table_mut()?;
    (rust.get("unsafe_code")?.as_str()? == "forbid").then_some(())?;
    rust.insert("unsafe_code".into(), "deny".into());
    Some(expected)
}

fn compare(
    main: &toml::Value,
    copy: Option<&toml::Value>,
    is_boundary: bool,
) -> Result<(), String> {
    let copy = copy.ok_or("no lint table")?;
    let expected = if is_boundary {
        boundary(main).ok_or("the main table must forbid unsafe_code")?
    } else {
        main.clone()
    };
    if *copy == expected {
        Ok(())
    } else if is_boundary {
        Err("lint table differs: copy the main table with only `unsafe_code = \"deny\"`".into())
    } else {
        Err("lint table differs from Cargo.toml: copy the main table".into())
    }
}

#[cfg(test)]
mod tests {
    use super::compare;

    const MAIN: &str = "[rust]\nunsafe_code = \"forbid\"\n[clippy]\npanic = \"warn\"\n";

    fn value(s: &str) -> toml::Value {
        toml::Value::Table(toml::from_str(s).unwrap())
    }

    #[test]
    fn equal_tables_pass_and_drift_fails() {
        let main = value(MAIN);
        assert!(compare(&main, Some(&main), false).is_ok());
        let drift = value(&MAIN.replace("panic = \"warn\"", "panic = \"allow\""));
        assert!(compare(&main, Some(&drift), false).is_err());
        assert!(compare(&main, None, false).is_err());
    }

    #[test]
    fn boundary_tables_may_only_lower_unsafe_to_deny() {
        let main = value(MAIN);
        let deny = value(&MAIN.replace("forbid", "deny"));
        assert!(compare(&main, Some(&deny), true).is_ok());
        // A boundary crate must not keep forbid (it could not allow its audited module),
        // must not allow unsafe everywhere, and must not change anything else.
        assert!(compare(&main, Some(&main), true).is_err());
        let allow = value(&MAIN.replace("forbid", "allow"));
        assert!(compare(&main, Some(&allow), true).is_err());
        let drift = value(&MAIN.replace("forbid", "deny").replace("warn", "allow"));
        assert!(compare(&main, Some(&drift), true).is_err());
        // The non-boundary forge may not lower it at all.
        assert!(compare(&main, Some(&deny), false).is_err());
    }
}
