//! `iso3166.yaml` (schema `teigeodata/iso3166/v1`): the ISO 3166 codes of a country's row and
//! units (D-197, [ISO 3166 codes](../../docs/02-research/notes/iso-3166-codes.md) §5.2).
//! Checked by `check-data`; `forge build` also checks that every key is a place of the build
//! and that every current level-1 unit has a code or a reason (gate G-iso).

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use teigeodata_core::IsoCode;

use crate::lexicon::{has_date, is_date};

const SCHEMA: &str = "teigeodata/iso3166/v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema: String,
    country: String,
    source: String,
    units: BTreeMap<String, String>,
    #[serde(default)]
    renamed: Vec<Renamed>,
    #[serde(default)]
    no_code: BTreeMap<String, String>,
    #[serde(default)]
    former: BTreeMap<String, Former>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Renamed {
    from: String,
    to: String,
    on: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Former {
    code: String,
    until: String,
}

/// A code of this country's subdivisions: valid, not a country's, prefixed by `country`.
fn unit_code(text: &str, country: Option<IsoCode>) -> Result<IsoCode, String> {
    let c: IsoCode = text.parse().map_err(|e| format!("{text:?}: {e}"))?;
    if c.is_country() {
        return Err(format!("{text}: a country's code, not a unit's"));
    }
    match country {
        Some(k) if c.country() != k.country() => {
            Err(format!("{text}: not prefixed by the country {k}"))
        }
        _ => Ok(c),
    }
}

/// The problems of the `iso3166.yaml` of the country `cc` (its folder, lower case), and its
/// number of units.
pub(crate) fn check_text(cc: &str, text: &str) -> (usize, Vec<String>) {
    let f: File = match serde_saphyr::from_str(text) {
        Ok(f) => f,
        Err(e) => return (0, vec![e.to_string()]),
    };
    let mut out = Vec::new();
    if f.schema != SCHEMA {
        out.push(format!("schema must be {SCHEMA}"));
    }
    let country = match f.country.parse::<IsoCode>() {
        Ok(c) if c.is_country() && c.as_str().eq_ignore_ascii_case(cc) => Some(c),
        Ok(c) if c.is_country() => {
            out.push(format!("country {c} is not the folder's {cc}"));
            Some(c)
        }
        _ => {
            out.push(format!(
                "country {:?} is not an ISO 3166-1 alpha-2 code",
                f.country
            ));
            None
        }
    };
    if !has_date(&f.source) {
        out.push("source must name the ISO change checked and where, with the date".to_owned());
    }
    let mut seen = BTreeSet::new();
    for (key, code) in &f.units {
        if key.trim().is_empty() {
            out.push("a unit with an empty key".to_owned());
        }
        match unit_code(code, country) {
            Ok(c) if !seen.insert(c) => out.push(format!("{key}: code {c} is used twice")),
            Ok(_) => {}
            Err(e) => out.push(format!("{key}: {e}")),
        }
    }
    for (key, reason) in &f.no_code {
        if f.units.contains_key(key) {
            out.push(format!("{key}: in units and in no_code"));
        }
        if reason.trim().is_empty() {
            out.push(format!("{key}: no_code needs a reason"));
        }
    }
    let mut stale = BTreeSet::new();
    for r in &f.renamed {
        let (from, to) = (r.from.parse::<IsoCode>(), r.to.parse::<IsoCode>());
        match (from, to) {
            (Ok(a), Ok(b)) if a == b => out.push(format!("renamed {a}: to itself")),
            (Ok(a), Ok(_)) if seen.contains(&a) => {
                out.push(format!("renamed {a}: a current unit's code"));
            }
            (Ok(a), Ok(_)) if !stale.insert(a) => out.push(format!("renamed {a}: twice")),
            (Ok(_), Ok(_)) => {}
            _ => out.push(format!(
                "renamed {:?} → {:?}: not ISO 3166 codes",
                r.from, r.to
            )),
        }
        if !is_date(&r.on) {
            out.push(format!("renamed {}: `on` must be a date", r.from));
        }
    }
    for (key, former) in &f.former {
        if f.units.contains_key(key) {
            out.push(format!("{key}: in units and in former"));
        }
        if let Err(e) = unit_code(&former.code, country) {
            out.push(format!("former {key}: {e}"));
        }
        if !is_date(&former.until) {
            out.push(format!("former {key}: `until` must be a date"));
        }
    }
    (f.units.len(), out)
}

#[cfg(test)]
mod tests {
    use super::check_text;

    const GOOD: &str = "schema: teigeodata/iso3166/v1\ncountry: NP\nsource: \"ISO 3166-2:NP, OBP change 2023-11-23 (checked 2026-11-11)\"\nunits:\n  \"admin:A\": NP-P1\n  \"admin:B\": NP-P2\nrenamed: []\nno_code: {}\nformer: {}\n";

    #[test]
    fn a_good_file_passes() {
        assert_eq!(check_text("np", GOOD), (2, Vec::new()));
        // The optional lists may be left out, and filled.
        let full = GOOD
            .replace(
                "renamed: []",
                "renamed: [{ from: NP-BA, to: NP-P3, on: 2022-11-29 }]",
            )
            .replace(
                "no_code: {}",
                "no_code: { \"admin:C\": \"a disputed area\" }",
            )
            .replace(
                "former: {}",
                "former: { \"admin:Z\": { code: NP-BA, until: 2022-11-29 } }",
            );
        assert_eq!(check_text("np", &full), (2, Vec::new()));
        let short = GOOD.replace("renamed: []\nno_code: {}\nformer: {}\n", "");
        assert_eq!(check_text("np", &short), (2, Vec::new()));
    }

    /// Planted faults: each bad field is found.
    #[test]
    fn every_bad_field_is_found() {
        for (bad, want) in [
            (GOOD.replace("iso3166/v1", "iso3166/v2"), "schema"),
            (GOOD.replace("country: NP", "country: IN"), "folder"),
            (GOOD.replace("country: NP", "country: NPL"), "alpha-2"),
            (GOOD.replace("country: NP", "country: NP-P1"), "alpha-2"),
            (
                GOOD.replace(" (checked 2026-11-11)", "")
                    .replace(" 2023-11-23", ""),
                "date",
            ),
            (GOOD.replace(": NP-P2", ": NP-P1"), "used twice"),
            (GOOD.replace(": NP-P2", ": IN-TN"), "prefixed"),
            (GOOD.replace(": NP-P2", ": NP-P1234"), "admin:B"),
            (GOOD.replace(": NP-P2", ": NP"), "a country's code"),
            (GOOD.replace(": NP-P2", ": XK-01"), "user-assigned"),
            (GOOD.replace("\"admin:B\"", "\" \""), "empty key"),
            (
                GOOD.replace(
                    "renamed: []",
                    "renamed: [{ from: NP-P1, to: NP-P3, on: 2022-11-29 }]",
                ),
                "current unit",
            ),
            (
                GOOD.replace(
                    "renamed: []",
                    "renamed: [{ from: NP-BA, to: NP-BA, on: 2022-11-29 }]",
                ),
                "itself",
            ),
            (
                GOOD.replace(
                    "renamed: []",
                    "renamed: [{ from: NP-BA, to: NP-P3, on: 2022 }]",
                ),
                "`on`",
            ),
            (
                GOOD.replace("no_code: {}", "no_code: { \"admin:A\": x }"),
                "in units and in no_code",
            ),
            (
                GOOD.replace("no_code: {}", "no_code: { \"admin:C\": \" \" }"),
                "reason",
            ),
            (
                GOOD.replace(
                    "former: {}",
                    "former: { \"admin:Z\": { code: IN-OR, until: 2023-11-23 } }",
                ),
                "prefixed",
            ),
            (
                GOOD.replace(
                    "former: {}",
                    "former: { \"admin:Z\": { code: NP-BA, until: soon } }",
                ),
                "`until`",
            ),
            (
                GOOD.replace("former: {}", "former: {}\nextra: 1"),
                "unknown field",
            ),
        ] {
            let (_, found) = check_text("np", &bad);
            assert!(found.iter().any(|f| f.contains(want)), "{want}: {found:?}");
        }
    }
}
