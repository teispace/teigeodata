//! `data/sources.lock` (schema `teigeodata/sources/v1`, ADR-0035): every data source a build or
//! spike reads, pinned by URL, version, size and SHA-256, with its licence class
//! ([`CLEAN_ROOM.md`](../../CLEAN_ROOM.md) rule 1). The forge's `fetch` stage downloads exactly
//! these and refuses any byte that does not match.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

const SCHEMA: &str = "teigeodata/sources/v1";

/// The licence classes of `CLEAN_ROOM.md` rule 1.
const CLASSES: [&str; 10] = [
    "public-domain",
    "cc0",
    "cc-by",
    "cc-by-igo",
    "odc-by",
    "godl",
    "odbl",
    "share-alike",
    "non-commercial",
    "proprietary",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lock {
    schema: String,
    #[serde(default, rename = "source")]
    sources: Vec<Source>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    id: String,
    title: String,
    /// `http` (a file) or `sparql` (a query snapshot).
    kind: String,
    url: String,
    /// Our own copy (a release asset); required before a release build reads the source.
    #[serde(default)]
    mirror_url: Option<String>,
    /// The upstream replaces the file in place (no dated archive): only the mirror is stable.
    #[serde(default)]
    moving: bool,
    version: String,
    retrieved: String,
    sha256: String,
    size: u64,
    licence: String,
    licence_class: String,
    /// ISO 3166-1 alpha-2 codes of the countries covered, or `["*"]` for world sources.
    countries: Vec<String>,
    /// What reads it (a spike, a forge stage).
    used_by: String,
    /// `sparql` only: the query template and its parameters.
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    query_params: Option<toml::Table>,
}

/// Validates the lock text. `root` resolves query paths; `countries` are the country folders.
pub(crate) fn check_text(
    text: &str,
    root: &Path,
    countries: &BTreeSet<String>,
) -> (usize, Vec<String>) {
    let lock: Lock = match toml::from_str(text) {
        Ok(l) => l,
        Err(e) => return (0, vec![format!("not a valid lock file: {e}")]),
    };
    let mut p = Vec::new();
    if lock.schema != SCHEMA {
        p.push(format!("schema is `{}`, expected `{SCHEMA}`", lock.schema));
    }
    let mut ids = BTreeSet::new();
    for s in &lock.sources {
        let at = format!("source `{}`", s.id);
        if !ids.insert(s.id.as_str()) {
            p.push(format!("{at}: duplicate id"));
        }
        if s.id.is_empty()
            || !s
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            p.push(format!("{at}: id must be lower-case kebab-case"));
        }
        for (field, v) in [
            ("title", &s.title),
            ("version", &s.version),
            ("licence", &s.licence),
            ("used_by", &s.used_by),
        ] {
            if v.trim().is_empty() {
                p.push(format!("{at}: {field} is empty"));
            }
        }
        if !s.url.starts_with("https://") {
            p.push(format!("{at}: url must be https"));
        }
        if s.mirror_url
            .as_deref()
            .is_some_and(|m| !m.starts_with("https://"))
        {
            p.push(format!("{at}: mirror_url must be https"));
        }
        if s.url.contains("latest") && !s.moving {
            p.push(format!(
                "{at}: a `latest` URL moves; pin a dated file or set `moving = true`"
            ));
        }
        if !crate::lexicon::is_date(&s.retrieved) {
            p.push(format!("{at}: retrieved must be a YYYY-MM-DD date"));
        }
        if s.sha256.len() != 64
            || !s
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            p.push(format!("{at}: sha256 must be 64 lower-case hex digits"));
        }
        if s.sha256.bytes().all(|b| b == b'0') {
            p.push(format!(
                "{at}: not pinned (all-zero sha256): run `forge fetch --pin {}`",
                s.id
            ));
        }
        if s.size == 0 {
            p.push(format!("{at}: size must be the byte count"));
        }
        if !CLASSES.contains(&s.licence_class.as_str()) {
            p.push(format!(
                "{at}: licence_class `{}` is not one of CLEAN_ROOM.md rule 1",
                s.licence_class
            ));
        }
        if s.countries.is_empty() {
            p.push(format!("{at}: countries is empty (use [\"*\"] for world)"));
        }
        for c in &s.countries {
            if c != "*" && !countries.contains(&c.to_ascii_lowercase()) {
                p.push(format!("{at}: country `{c}` has no data/countries folder"));
            }
        }
        match (s.kind.as_str(), &s.query) {
            ("http", None) => {}
            ("http", Some(_)) => p.push(format!("{at}: an http source has no query")),
            ("sparql", Some(q)) => {
                if !root.join(q).is_file() {
                    p.push(format!("{at}: query file `{q}` not found"));
                }
                if s.query_params.is_none() {
                    p.push(format!("{at}: a sparql source needs query_params"));
                }
            }
            ("sparql", None) => p.push(format!("{at}: a sparql source needs a query")),
            (k, _) => p.push(format!("{at}: unknown kind `{k}`")),
        }
    }
    (lock.sources.len(), p)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::check_text;

    const GOOD: &str = r#"
schema = "teigeodata/sources/v1"

[[source]]
id = "geofabrik-np"
title = "OpenStreetMap Nepal extract (Geofabrik)"
kind = "http"
url = "https://download.geofabrik.de/asia/nepal-261105.osm.pbf"
version = "2026-11-05"
retrieved = "2026-11-06"
sha256 = "7f3a9c0e5b2d4f6a8c1e3b5d7f9a2c4e6b8d0f1a3c5e7b9d2f4a6c8e0b1d3f5a"
size = 414872957
licence = "ODbL-1.0"
licence_class = "odbl"
countries = ["NP"]
used_by = "spike S5"
"#;

    fn countries() -> BTreeSet<String> {
        ["np".to_string()].into_iter().collect()
    }

    #[test]
    fn accepts_a_valid_lock() {
        let (n, p) = check_text(GOOD, Path::new("."), &countries());
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(n, 1);
    }

    #[test]
    fn rejects_each_violation() {
        let cases = [
            (
                "id = \"geofabrik-np\"",
                "id = \"Geofabrik_NP\"",
                "kebab-case",
            ),
            (
                "url = \"https://download",
                "url = \"http://download",
                "must be https",
            ),
            (
                "nepal-261105.osm.pbf",
                "nepal-latest.osm.pbf",
                "`latest` URL moves",
            ),
            (
                "retrieved = \"2026-11-06\"",
                "retrieved = \"6 Nov\"",
                "YYYY-MM-DD",
            ),
            ("sha256 = \"7f3a", "sha256 = \"ABCD", "64 lower-case hex"),
            (
                "sha256 = \"7f3a9c0e5b2d4f6a8c1e3b5d7f9a2c4e6b8d0f1a3c5e7b9d2f4a6c8e0b1d3f5a\"",
                "sha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"",
                "not pinned",
            ),
            ("size = 414872957", "size = 0", "byte count"),
            (
                "licence_class = \"odbl\"",
                "licence_class = \"open\"",
                "CLEAN_ROOM.md rule 1",
            ),
            (
                "countries = [\"NP\"]",
                "countries = [\"XK\"]",
                "no data/countries",
            ),
            ("kind = \"http\"", "kind = \"ftp\"", "unknown kind"),
            ("kind = \"http\"", "kind = \"sparql\"", "needs a query"),
            (
                "used_by = \"spike S5\"",
                "used_by = \" \"",
                "used_by is empty",
            ),
            (
                "used_by = \"spike S5\"",
                "used_by = \"spike S5\"\nnotes = \"x\"",
                "not a valid lock",
            ),
        ];
        for (from, to, expect) in cases {
            assert!(GOOD.contains(from), "test setup: `{from}` not found");
            let bad = GOOD.replacen(from, to, 1);
            let (_, p) = check_text(&bad, Path::new("."), &countries());
            assert!(
                p.iter().any(|m| m.contains(expect)),
                "expected `{expect}` for `{to}`, got {p:?}"
            );
        }
        let dup = format!("{GOOD}{}", GOOD.split("\n\n").nth(1).unwrap_or_default());
        let (_, p) = check_text(&dup, Path::new("."), &countries());
        assert!(p.iter().any(|m| m.contains("duplicate id")), "{p:?}");
    }
}
