//! `check-workspace`: the forge is a separate Cargo workspace (ADR-0001), so it cannot inherit
//! the main workspace's lint table. This check keeps the two tables identical.

use std::fs;
use std::path::Path;

/// Compares `[workspace.lints]` of `Cargo.toml` and `forge/Cargo.toml`.
pub(crate) fn check(root: &Path) -> Result<String, Vec<String>> {
    let read = |rel: &str| -> Result<toml::Table, String> {
        let text = fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
        toml::from_str(&text).map_err(|e| format!("{rel}: {e}"))
    };
    let (main, forge) = match (read("Cargo.toml"), read("forge/Cargo.toml")) {
        (Ok(m), Ok(f)) => (m, f),
        (m, f) => return Err([m.err(), f.err()].into_iter().flatten().collect()),
    };
    compare(&main, &forge)
}

fn lints(t: &toml::Table) -> Option<&toml::Value> {
    t.get("workspace")?.get("lints")
}

fn compare(main: &toml::Table, forge: &toml::Table) -> Result<String, Vec<String>> {
    match (lints(main), lints(forge)) {
        (Some(m), Some(f)) if m == f => {
            Ok("workspace ok: forge lints match the main workspace".into())
        }
        (Some(_), Some(_)) => Err(vec![
            "forge/Cargo.toml [workspace.lints] differs from Cargo.toml: copy the main table"
                .into(),
        ]),
        _ => Err(vec![
            "both Cargo.toml files need a [workspace.lints] table".into(),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::compare;

    const MAIN: &str = "[workspace.lints.rust]\nunsafe_code = \"forbid\"\n[workspace.lints.clippy]\npanic = \"warn\"\n";

    #[test]
    fn equal_tables_pass_and_drift_fails() {
        let main: toml::Table = toml::from_str(MAIN).unwrap_or_default();
        assert!(compare(&main, &main).is_ok());
        let drift: toml::Table =
            toml::from_str(&MAIN.replace("panic = \"warn\"", "panic = \"allow\""))
                .unwrap_or_default();
        assert!(compare(&main, &drift).is_err());
        let none: toml::Table = toml::from_str("[workspace]\n").unwrap_or_default();
        assert!(compare(&main, &none).is_err());
    }
}
