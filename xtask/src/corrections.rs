//! Correction files, `data/countries/<cc>/corrections/<id>.yaml` (schema
//! `teigeodata/correction/v1`, ADR-0036). The forge applies them (`build::corrections`); this
//! checks their form on every commit: the schema, the id is the file name and unique, the op
//! and exactly its fields, the guard, evidence with a known licence class and https URLs, and
//! the dates.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::sources::CLASSES;

const SCHEMA: &str = "teigeodata/correction/v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema: String,
    id: String,
    target: String,
    op: String,
    #[serde(default)]
    name: Option<Name>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    lat: Option<f64>,
    #[serde(default)]
    lon: Option<f64>,
    #[serde(default)]
    population: Option<i64>,
    #[serde(default)]
    other: Option<String>,
    guard: Guard,
    evidence: Vec<Evidence>,
    created: String,
    author: String,
    review: Review,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Name {
    text: String,
    #[serde(default)]
    lang: Option<String>,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Guard {
    record: String,
    field: String,
    value_hash: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    source: String,
    #[serde(default)]
    url: Option<String>,
    licence_class: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Review {
    by: String,
    on: String,
}

/// Checks a country's corrections folder; returns how many files it holds and the problems.
pub(crate) fn check(country: &Path) -> (usize, Vec<String>) {
    let dir = country.join("corrections");
    let Ok(rd) = fs::read_dir(&dir) else {
        return (0, Vec::new());
    };
    let mut files: Vec<_> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
    files.sort();
    let mut problems = Vec::new();
    let mut ids = BTreeSet::new();
    for f in &files {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let Some(stem) = name.strip_suffix(".yaml") else {
            problems.push(format!("corrections/{name}: not a .yaml file"));
            continue;
        };
        let found = match fs::read_to_string(f) {
            Ok(text) => check_text(stem, &text),
            Err(e) => vec![e.to_string()],
        };
        if !ids.insert(stem.to_string()) {
            problems.push(format!("corrections/{name}: duplicate id"));
        }
        problems.extend(
            found
                .into_iter()
                .map(|p| format!("corrections/{name}: {p}")),
        );
    }
    (files.len(), problems)
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// The problems of one correction file named `<stem>.yaml`.
pub(crate) fn check_text(stem: &str, text: &str) -> Vec<String> {
    let f: File = match serde_saphyr::from_str(text) {
        Ok(f) => f,
        Err(e) => return vec![e.to_string()],
    };
    let mut p = Vec::new();
    if f.schema != SCHEMA {
        p.push(format!("schema must be {SCHEMA}"));
    }
    if f.id != stem {
        p.push(format!("id {} is not the file name", f.id));
    }
    for r in [&f.target, &f.guard.record] {
        if r.split_once(':')
            .is_none_or(|(s, i)| s.is_empty() || i.is_empty())
        {
            p.push(format!("{r} is not a source record (<source>:<id>)"));
        }
    }
    let given = [
        ("name", f.name.is_some()),
        ("text", f.text.is_some()),
        ("lat", f.lat.is_some()),
        ("lon", f.lon.is_some()),
        ("population", f.population.is_some()),
        ("other", f.other.is_some()),
    ];
    let uses: &[&str] = match f.op.as_str() {
        "add-name" => &["name"],
        "remove-name" => &["text"],
        "set-point" => &["lat", "lon"],
        "set-population" => &["population"],
        "same-as" => &["other"],
        o => {
            p.push(format!("op {o} is unknown"));
            &[]
        }
    };
    // An unknown op is reported once, not once per field.
    for (field, set) in given.into_iter().filter(|_| !uses.is_empty()) {
        if set != uses.contains(&field) {
            let verb = if set { "takes no" } else { "needs" };
            p.push(format!("op {} {verb} {field}", f.op));
        }
    }
    if let Some(n) = &f.name {
        if n.text.trim().is_empty() {
            p.push("an empty name".into());
        }
        let types = [
            "official",
            "local",
            "alt",
            "colloquial",
            "historical",
            "former-official",
        ];
        if !types.contains(&n.kind.as_str()) {
            p.push(format!("name type {} is unknown", n.kind));
        }
        if n.lang.as_deref().is_some_and(str::is_empty) {
            p.push("an empty lang".into());
        }
    }
    if let (Some(lat), Some(lon)) = (f.lat, f.lon)
        && !(lat.abs() <= 90.0 && lon.abs() <= 180.0)
    {
        p.push("point out of range".into());
    }
    if f.population.is_some_and(|n| n < 0) {
        p.push("negative population".into());
    }
    check_guard_and_evidence(&f, &mut p);
    p
}

/// The guard, the evidence, the dates and the people.
fn check_guard_and_evidence(f: &File, p: &mut Vec<String>) {
    if !matches!(f.guard.field.as_str(), "name" | "point" | "population") {
        p.push(format!("guard field {} is unknown", f.guard.field));
    }
    if f.guard.value_hash.len() != 64 || !f.guard.value_hash.bytes().all(|b| b.is_ascii_hexdigit())
    {
        p.push("guard value_hash is not 64 hex digits (forge correction-guard prints it)".into());
    }
    if f.evidence.is_empty() {
        p.push("no evidence".into());
    }
    for e in &f.evidence {
        if e.source.trim().is_empty() {
            p.push("evidence without a source".into());
        }
        if !CLASSES.contains(&e.licence_class.as_str()) {
            p.push(format!(
                "licence_class {} is not one of CLEAN_ROOM.md rule 1",
                e.licence_class
            ));
        }
        if e.url.as_deref().is_some_and(|u| !u.starts_with("https://")) {
            p.push("evidence URLs are https".into());
        }
    }
    if !is_date(&f.created) || !is_date(&f.review.on) {
        p.push("dates are YYYY-MM-DD".into());
    }
    if f.author.trim().is_empty() || f.review.by.trim().is_empty() {
        p.push("author and review.by are required".into());
    }
}

#[cfg(test)]
mod tests {
    use super::check_text;

    const OK: &str = "schema: teigeodata/correction/v1
id: np-0001
target: geonames:1283240
op: add-name
name: { text: \"𑐫𑐾𑑃\", lang: new, type: local }
guard: { record: geonames:1283240, field: name, value_hash: 0000000000000000000000000000000000000000000000000000000000000000 }
evidence:
  - { source: \"Wikipedia\", url: \"https://en.wikipedia.org/wiki/Kathmandu\", licence_class: share-alike }
created: 2026-11-09
author: a
review: { by: b, on: 2026-11-09 }
";

    #[test]
    fn a_good_file_passes_and_each_fault_is_named() {
        assert_eq!(check_text("np-0001", OK), Vec::<String>::new());
        let cases = [
            ("np-0002", OK.to_string(), "file name"),
            (
                "np-0001",
                OK.replace("op: add-name", "op: rename"),
                "unknown",
            ),
            (
                "np-0001",
                OK.replace("op: add-name", "op: add-name\nother: wikidata:Q1"),
                "takes no other",
            ),
            (
                "np-0001",
                OK.replace(
                    "op: add-name\nname: { text: \"𑐫𑐾𑑃\", lang: new, type: local }",
                    "op: same-as",
                ),
                "needs other",
            ),
            (
                "np-0001",
                OK.replace("share-alike", "cc-by-sa"),
                "licence_class",
            ),
            ("np-0001", OK.replace("https://", "http://"), "https"),
            (
                "np-0001",
                OK.replace("value_hash: 0000", "value_hash: 00"),
                "64 hex",
            ),
            (
                "np-0001",
                OK.replace("created: 2026-11-09", "created: 9 Nov 2026"),
                "YYYY-MM-DD",
            ),
            (
                "np-0001",
                OK.replace("type: local", "type: nickname"),
                "name type",
            ),
            (
                "np-0001",
                OK.replace("author: a", "author: a\nauther: x"),
                "unknown field",
            ),
        ];
        for (stem, text, want) in cases {
            let found = check_text(stem, &text);
            assert!(found.iter().any(|p| p.contains(want)), "{want}: {found:?}");
        }
    }
}
