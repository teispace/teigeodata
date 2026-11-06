//! Golden query files (schema `teigeodata/golden/v0`, ADR-0032): the generated
//! `tests/golden/<cc>.yaml` and the hand-written `data/countries/<cc>/golden-curated.yaml`.
//!
//! This is the structural check that runs on every PR. The release gate (at least 500 queries,
//! at least 30 per class, and the recall thresholds) runs with the search evaluation.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use unicode_normalization::is_nfc;

const SCHEMA: &str = "teigeodata/golden/v0";

/// The query classes of names-and-search §5.7.
pub(crate) const CLASSES: [&str; 11] = [
    "exact_latin",
    "exact_native",
    "variant",
    "typo",
    "prefix",
    "abbreviation",
    "historical",
    "colloquial",
    "mixed_script",
    "with_context",
    "ambiguous",
];

/// Source-key schemes an expectation may use until our stable IDs exist.
const SCHEMES: [&str; 6] = ["tgd", "geonames", "codab", "osm", "wikidata", "wof"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GoldenFile {
    schema: String,
    country: String,
    queries: Vec<GoldenQuery>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GoldenQuery {
    id: String,
    class: String,
    query: String,
    expect: BTreeMap<String, u8>,
    max_rank: u8,
    source: String,
}

/// Validates one golden file. Returns the per-class counts and the problems found.
pub(crate) fn check_text(cc: &str, text: &str) -> (BTreeMap<String, usize>, Vec<String>) {
    let file: GoldenFile = match serde_saphyr::from_str(text) {
        Ok(f) => f,
        Err(e) => {
            return (
                BTreeMap::new(),
                vec![format!("not a valid golden file: {e}")],
            );
        }
    };
    let mut p = Vec::new();
    if file.schema != SCHEMA {
        p.push(format!("schema is `{}`, expected `{SCHEMA}`", file.schema));
    }
    if file.country != cc.to_ascii_uppercase() {
        p.push(format!(
            "country `{}` must be `{}`",
            file.country,
            cc.to_ascii_uppercase()
        ));
    }
    let prefix = format!("{cc}-");
    let mut ids = BTreeSet::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for q in &file.queries {
        let at = format!("query `{}`", q.id);
        if !ids.insert(q.id.as_str()) {
            p.push(format!("{at}: duplicate id"));
        }
        if !q.id.starts_with(&prefix) {
            p.push(format!("{at}: id must start with `{prefix}`"));
        }
        if !CLASSES.contains(&q.class.as_str()) {
            p.push(format!("{at}: unknown class `{}`", q.class));
        }
        if q.query.trim().is_empty() {
            p.push(format!("{at}: empty query"));
        }
        if !is_nfc(&q.query) {
            p.push(format!("{at}: query is not NFC"));
        }
        if q.expect.is_empty() {
            p.push(format!("{at}: no expected results"));
        }
        if !q.expect.values().any(|g| *g >= 2) {
            p.push(format!("{at}: needs at least one result with grade 2 or 3"));
        }
        for (key, grade) in &q.expect {
            let scheme = key.split_once(':').map(|(s, v)| (s, v.is_empty()));
            match scheme {
                Some((s, false)) if SCHEMES.contains(&s) => {}
                _ => p.push(format!(
                    "{at}: `{key}` is not `<scheme>:<id>` with a known scheme"
                )),
            }
            if !(1..=3).contains(grade) {
                p.push(format!("{at}: grade {grade} for `{key}` is not 1–3"));
            }
        }
        if !(1..=10).contains(&q.max_rank) {
            p.push(format!("{at}: max_rank {} is not 1–10", q.max_rank));
        }
        if q.source.trim().is_empty() {
            p.push(format!("{at}: every query needs a source"));
        }
        *counts.entry(q.class.clone()).or_default() += 1;
    }
    (counts, p)
}

#[cfg(test)]
mod tests {
    use super::check_text;

    const GOOD: &str = r#"
schema: teigeodata/golden/v0
country: NP
queries:
  - id: np-0001
    class: variant
    query: "Lahachok"
    expect: { "geonames:1283240": 3, "codab:NP0101301": 1 }
    max_rank: 5
    source: "maintainer ruling 2026-11-06"
"#;

    #[test]
    fn accepts_a_valid_file() {
        let (counts, p) = check_text("np", GOOD);
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(counts.get("variant"), Some(&1));
    }

    #[test]
    fn rejects_each_violation() {
        let cases = [
            ("country: NP", "country: IN", "must be `NP`"),
            ("id: np-0001", "id: in-0001", "must start with"),
            ("class: variant", "class: spelling", "unknown class"),
            (
                "\"geonames:1283240\": 3",
                "\"geonames:1283240\": 1",
                "grade 2 or 3",
            ),
            (
                "\"geonames:1283240\": 3",
                "\"gn:1283240\": 3",
                "known scheme",
            ),
            (
                "\"codab:NP0101301\": 1",
                "\"codab:NP0101301\": 4",
                "is not 1–3",
            ),
            ("max_rank: 5", "max_rank: 0", "max_rank 0"),
            (
                "source: \"maintainer ruling 2026-11-06\"",
                "source: \" \"",
                "needs a source",
            ),
            // U+0958 is a composition exclusion, so the precomposed form is not NFC.
            ("query: \"Lahachok\"", "query: \"\u{0958}\"", "not NFC"),
            (
                "max_rank: 5",
                "max_rank: 5\n    notes: x",
                "not a valid golden file",
            ),
        ];
        for (from, to, expect) in cases {
            assert!(GOOD.contains(from), "test setup: `{from}` not found");
            let bad = GOOD.replacen(from, to, 1);
            let (_, p) = check_text("np", &bad);
            assert!(
                p.iter().any(|m| m.contains(expect)),
                "expected `{expect}` for `{to}`, got {p:?}"
            );
        }
        let dup = format!(
            "{GOOD}{}",
            GOOD.split("queries:\n").nth(1).unwrap_or_default()
        );
        let (_, p) = check_text("np", &dup);
        assert!(p.iter().any(|m| m.contains("duplicate id")), "{p:?}");
    }
}
