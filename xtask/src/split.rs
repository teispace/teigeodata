//! `split.yaml` (schema `teigeodata/split/v1`): the admin level a country's split build cuts at
//! and, at a level ISO 3166-2 does not code, each unit's address code (D-191, ADR-0010). Without
//! `units`, each unit's address comes from its ISO code in `iso3166.yaml` (D-197), which must
//! then exist. Checked by `check-data`; `forge make --split` also checks that every unit is a
//! place of the build at that level.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::lexicon::has_date;

const SCHEMA: &str = "teigeodata/split/v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Split {
    schema: String,
    level: u8,
    source: Option<String>,
    units: Option<BTreeMap<String, String>>,
}

/// Whether `code` can be the part of a pack address after `<cc>-`: lower-case ASCII letters,
/// digits and inner hyphens, at most 16 bytes.
fn is_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 16
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !code.starts_with('-')
        && !code.ends_with('-')
}

/// The problems of one `split.yaml`, and its number of units; `iso` is whether the country has
/// an `iso3166.yaml` to take the units from.
pub(crate) fn check_text(text: &str, iso: bool) -> (usize, Vec<String>) {
    let s: Split = match serde_saphyr::from_str(text) {
        Ok(s) => s,
        Err(e) => return (0, vec![e.to_string()]),
    };
    let mut out = Vec::new();
    if s.schema != SCHEMA {
        out.push(format!("schema must be {SCHEMA}"));
    }
    if !(1..=10).contains(&s.level) {
        out.push(format!(
            "level {} is not an admin level below the country",
            s.level
        ));
    }
    let (source, units) = match (s.source, s.units) {
        (None, None) => {
            if !iso {
                out.push("no units, and no iso3166.yaml to take them from".to_owned());
            }
            return (0, out);
        }
        (Some(source), Some(units)) => (source, units),
        _ => {
            out.push("source and units go together".to_owned());
            return (0, out);
        }
    };
    if !has_date(&source) {
        out.push("source must name where the codes come from, with the date read".to_owned());
    }
    if units.is_empty() {
        out.push("no units".to_owned());
    }
    let mut seen = BTreeSet::new();
    for (key, code) in &units {
        if key.trim().is_empty() {
            out.push("a unit with an empty key".to_owned());
        }
        if !is_code(code) {
            out.push(format!(
                "{key}: code {code:?} is not a lower-case address part"
            ));
        } else if !seen.insert(code) {
            out.push(format!("{key}: code {code:?} is used twice"));
        }
    }
    (units.len(), out)
}

#[cfg(test)]
mod tests {
    use super::check_text;

    const GOOD: &str = "schema: teigeodata/split/v1\nlevel: 1\nsource: ISO 3166-2 (2026-11-11)\nunits:\n  \"admin:A\": p1\n  \"admin:B\": p2\n";

    #[test]
    fn a_good_split_passes() {
        assert_eq!(check_text(GOOD, false), (2, Vec::new()));
        // Units from iso3166.yaml.
        let iso = "schema: teigeodata/split/v1\nlevel: 1\n";
        assert_eq!(check_text(iso, true), (0, Vec::new()));
    }

    #[test]
    fn every_bad_field_is_found() {
        for (bad, want) in [
            (GOOD.replace("split/v1", "split/v2"), "schema"),
            (GOOD.replace("level: 1", "level: 0"), "level 0"),
            (GOOD.replace(" (2026-11-11)", ""), "date read"),
            (GOOD.replace(": p2", ": p1"), "used twice"),
            (GOOD.replace(": p2", ": P2"), "address part"),
            (GOOD.replace(": p2", ": -p"), "address part"),
            (
                GOOD.replace("level: 1\n", "level: 1\nextra: 2\n"),
                "unknown field",
            ),
            (
                GOOD.replace("source: ISO 3166-2 (2026-11-11)\n", ""),
                "go together",
            ),
        ] {
            let (_, found) = check_text(&bad, true);
            assert!(found.iter().any(|f| f.contains(want)), "{want}: {found:?}");
        }
        // No units and no iso3166.yaml.
        let (_, found) = check_text("schema: teigeodata/split/v1\nlevel: 1\n", false);
        assert!(
            found.iter().any(|f| f.contains("no iso3166.yaml")),
            "{found:?}"
        );
    }
}
