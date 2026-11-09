//! `check-data`: validates every country folder under `data/countries/<cc>/`.
//!
//! Countries are data, not code (principle P16). Each folder holds:
//!
//! - `profile.yaml` (required, schema `teigeodata/profile/v1`): languages, scripts, the admin
//!   ladder, unit types with their abbreviations and source codes, droppable suffixes, and how
//!   each source encodes unit types.
//! - `lexicon.yaml` (optional, schema `teigeodata/lexicon/v1`): generic place-name terms
//!   ([`crate::lexicon`]).
//! - `build.yaml` (optional, schema `teigeodata/build/v1`): how `forge make` builds the
//!   country's packs ([`crate::recipe`]).
//! - `golden-curated.yaml` (optional) and the generated `tests/golden/<cc>.yaml`: golden search
//!   queries ([`crate::golden`]).
//!
//! The same checks run for every country, so adding one is adding a folder. The source lock,
//! `data/sources.lock`, is checked here too ([`crate::sources`]).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::Deserialize;
use unicode_script::Script;

use crate::golden;
use crate::lexicon::{self, Evidence, EvidenceKind, Native, Status};
use crate::postal;
use crate::recipe;
use crate::sources;

const PROFILE_SCHEMA: &str = "teigeodata/profile/v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    country: String,
    /// ISO 3166-1 alpha-3 (some sources, such as COD-AB, key by it).
    iso3: String,
    name: String,
    /// The country's Wikidata item (`Q837`), for per-country source queries.
    #[serde(default)]
    wikidata: Option<String>,
    /// The phonetic key its names use, by algorithm name (`teigeodata_names::PhoneticKey`).
    #[serde(default)]
    phonetic_key: Option<String>,
    /// BCP 47 language codes, official ones first.
    languages: Vec<String>,
    /// ISO 15924 scripts in use.
    scripts: Vec<String>,
    admin_levels: Vec<AdminLevel>,
    #[serde(default)]
    unit_types: Vec<UnitType>,
    #[serde(default)]
    droppable_suffixes: Vec<Term>,
    /// Source adapter ID → how that source encodes things for this country.
    #[serde(default)]
    sources: BTreeMap<String, SourceRules>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminLevel {
    level: u8,
    english: String,
    #[serde(default)]
    native: Vec<Native>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnitType {
    id: String,
    level: u8,
    english: String,
    #[serde(default)]
    native: Vec<Native>,
    /// The number of units expected (checked against the data build).
    #[serde(default)]
    expected_count: Option<u32>,
    #[serde(default)]
    abbreviations: Vec<Term>,
    /// Source adapter ID → the code that source uses for this unit type.
    #[serde(default)]
    source_codes: BTreeMap<String, String>,
}

/// A short text with a review status and evidence (abbreviations, droppable suffixes).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Term {
    text: String,
    status: Status,
    #[serde(default)]
    evidence: Vec<Evidence>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRules {
    /// Where the unit-type code sits in a source field (for example a P-code digit).
    #[serde(default)]
    unit_type_code: Option<CodeSlice>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CodeSlice {
    field: String,
    offset: usize,
    length: usize,
}

/// Checks every country folder.
pub(crate) fn check(root: &Path) -> Result<String, Vec<String>> {
    let dir = root.join("data/countries");
    let mut countries: Vec<_> = match fs::read_dir(&dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect(),
        Err(e) => return Err(vec![format!("{}: {e}", dir.display())]),
    };
    countries.sort();
    let mut problems = Vec::new();
    let mut terms = 0usize;
    let mut queries = 0usize;
    let mut postal_codes = 0usize;
    let mut scheme_ids = BTreeSet::new();
    let lock_text = fs::read_to_string(root.join("data/sources.lock")).unwrap_or_default();
    let pinned = sources::ids(&lock_text);
    for path in &countries {
        let cc = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let at = format!("data/countries/{cc}");
        if cc.len() != 2 || !cc.bytes().all(|b| b.is_ascii_lowercase()) {
            problems.push(format!(
                "{at}: folder must be a lower-case ISO 3166-1 alpha-2 code"
            ));
            continue;
        }
        match fs::read_to_string(path.join("profile.yaml")) {
            Ok(text) => problems.extend(
                check_profile(cc, &text)
                    .into_iter()
                    .map(|p| format!("{at}/profile.yaml: {p}")),
            ),
            Err(e) => problems.push(format!("{at}/profile.yaml: {e}")),
        }
        if let Ok(text) = fs::read_to_string(path.join("build.yaml")) {
            problems.extend(
                recipe::check_text(&text, &pinned)
                    .into_iter()
                    .map(|p| format!("{at}/build.yaml: {p}")),
            );
        }
        let mut golden_queries = 0usize;
        for file in [
            path.join("golden-curated.yaml"),
            root.join(format!("tests/golden/{cc}.yaml")),
        ] {
            if let Ok(text) = fs::read_to_string(&file) {
                let shown = file
                    .strip_prefix(root)
                    .unwrap_or(&file)
                    .display()
                    .to_string();
                let (counts, found) = golden::check_text(cc, &text);
                golden_queries += counts.values().sum::<usize>();
                problems.extend(found.into_iter().map(|p| format!("{shown}: {p}")));
            }
        }
        queries += golden_queries;
        if let Ok(text) = fs::read_to_string(path.join("postal.yaml")) {
            let (n, found) = postal::check(path, &text, &pinned, &mut scheme_ids);
            postal_codes += n;
            problems.extend(found.into_iter().map(|p| format!("{at}/postal.yaml: {p}")));
        }
        let lex = path.join("lexicon.yaml");
        if lex.exists() {
            match fs::read_to_string(&lex) {
                Ok(text) => {
                    let (n, found) = lexicon::check_text(cc, &text);
                    terms += n;
                    problems.extend(found.into_iter().map(|p| format!("{at}/lexicon.yaml: {p}")));
                }
                Err(e) => problems.push(format!("{at}/lexicon.yaml: {e}")),
            }
        }
    }
    let folders: BTreeSet<String> = countries
        .iter()
        .filter_map(|p| p.file_name()?.to_str().map(str::to_string))
        .collect();
    let lock = root.join("data/sources.lock");
    let mut pinned = 0usize;
    match fs::read_to_string(&lock) {
        Ok(text) => {
            let (n, found) = sources::check_text(&text, root, &folders);
            pinned = n;
            problems.extend(found.into_iter().map(|p| format!("data/sources.lock: {p}")));
        }
        Err(e) => problems.push(format!("data/sources.lock: {e}")),
    }
    if problems.is_empty() {
        Ok(format!(
            "data ok: {} countries, {terms} lexicon terms, {queries} golden query entries, {postal_codes} postal codes, {pinned} pinned sources checked",
            countries.len()
        ))
    } else {
        Err(problems)
    }
}

/// Validates one profile; `cc` is the folder name.
fn check_profile(cc: &str, text: &str) -> Vec<String> {
    let p: Profile = match serde_saphyr::from_str(text) {
        Ok(p) => p,
        Err(e) => return vec![format!("not a valid profile: {e}")],
    };
    let mut out = Vec::new();
    if p.schema != PROFILE_SCHEMA {
        out.push(format!(
            "schema is `{}`, expected `{PROFILE_SCHEMA}`",
            p.schema
        ));
    }
    if p.country != cc.to_ascii_uppercase() {
        out.push(format!(
            "country `{}` must be `{}`",
            p.country,
            cc.to_ascii_uppercase()
        ));
    }
    if p.iso3.len() != 3 || !p.iso3.bytes().all(|b| b.is_ascii_uppercase()) {
        out.push(format!(
            "iso3 `{}` must be three upper-case letters",
            p.iso3
        ));
    }
    if p.name.trim().is_empty() {
        out.push("name is empty".to_string());
    }
    if let Some(q) = &p.wikidata
        && !(q.len() > 1
            && q.starts_with('Q')
            && q[1..].bytes().all(|b| b.is_ascii_digit())
            && !q[1..].starts_with('0'))
    {
        out.push(format!("wikidata `{q}` is not a Wikidata item ID (Q…)"));
    }
    if let Some(k) = &p.phonetic_key
        && teigeodata_names::PhoneticKey::from_name(k).is_none()
    {
        let known: Vec<&str> = teigeodata_names::PhoneticKey::ALL
            .iter()
            .map(|(n, _)| *n)
            .collect();
        out.push(format!("phonetic_key `{k}` is not one of {known:?}"));
    }
    if p.languages.is_empty() {
        out.push("languages is empty".to_string());
    }
    for l in &p.languages {
        if !is_language(l) {
            out.push(format!("`{l}` is not a BCP 47 language code"));
        }
    }
    for s in &p.scripts {
        if Script::from_short_name(s).is_none() {
            out.push(format!("`{s}` is not an ISO 15924 script code"));
        }
    }
    // The admin ladder: levels 1, 2, 3, … without gaps.
    for (i, l) in p.admin_levels.iter().enumerate() {
        if usize::from(l.level) != i + 1 {
            out.push(format!(
                "admin level {} is out of order (expected {})",
                l.level,
                i + 1
            ));
        }
        if l.english.trim().is_empty() {
            out.push(format!("admin level {} has no English name", l.level));
        }
        for n in &l.native {
            native(&format!("admin level {}", l.level), n, &p.scripts, &mut out);
        }
    }
    check_unit_types(&p, &mut out);
    check_sources(&p, &mut out);
    for s in &p.droppable_suffixes {
        term(&format!("droppable suffix `{}`", s.text), s, &mut out);
    }
    out
}

fn check_unit_types(p: &Profile, out: &mut Vec<String>) {
    let levels: BTreeSet<u8> = p.admin_levels.iter().map(|l| l.level).collect();
    let mut ids = BTreeSet::new();
    for u in &p.unit_types {
        let at = format!("unit type `{}`", u.id);
        if !ids.insert(u.id.as_str()) {
            out.push(format!("{at}: duplicate id"));
        }
        if u.id.is_empty()
            || !u
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'_' || b.is_ascii_digit())
        {
            out.push(format!("{at}: id must be lower_snake_case"));
        }
        if !levels.contains(&u.level) {
            out.push(format!("{at}: level {} is not in admin_levels", u.level));
        }
        if u.english.trim().is_empty() {
            out.push(format!("{at}: no English name"));
        }
        if u.expected_count == Some(0) {
            out.push(format!("{at}: expected_count 0; omit it instead"));
        }
        for n in &u.native {
            native(&at, n, &p.scripts, out);
        }
        for a in &u.abbreviations {
            term(&format!("{at} abbreviation `{}`", a.text), a, out);
        }
        for src in u.source_codes.keys() {
            if !p.sources.contains_key(src) {
                out.push(format!("{at}: source `{src}` is not declared in `sources`"));
            }
        }
    }
}

fn check_sources(p: &Profile, out: &mut Vec<String>) {
    // Every source that encodes unit types must give each unit type a distinct code.
    for (src, rules) in &p.sources {
        if let Some(slice) = &rules.unit_type_code {
            if slice.field.trim().is_empty() || slice.length == 0 {
                out.push(format!(
                    "source `{src}`: unit_type_code needs a field and a length"
                ));
            }
            let mut codes = BTreeMap::new();
            for u in &p.unit_types {
                match u.source_codes.get(src) {
                    Some(code) if code.chars().count() != slice.length => out.push(format!(
                        "unit type `{}`: `{src}` code `{code}` must have length {}",
                        u.id, slice.length
                    )),
                    Some(code) => {
                        if let Some(other) = codes.insert(code.clone(), u.id.clone()) {
                            out.push(format!(
                                "source `{src}`: code `{code}` is used by `{other}` and `{}`",
                                u.id
                            ));
                        }
                    }
                    None => {}
                }
            }
            if slice.offset > 64 {
                out.push(format!(
                    "source `{src}`: unit_type_code offset {} is implausible",
                    slice.offset
                ));
            }
        }
    }
}

fn native(at: &str, n: &Native, scripts: &[String], out: &mut Vec<String>) {
    lexicon::check_native(at, n, out);
    if !scripts.contains(&n.script) {
        out.push(format!(
            "{at}: script `{}` is not in the profile's scripts",
            n.script
        ));
    }
}

fn term(at: &str, t: &Term, out: &mut Vec<String>) {
    if t.text.trim().is_empty() || t.text.trim() != t.text {
        out.push(format!("{at}: empty or padded text"));
    }
    if t.status == Status::Accepted && t.evidence.is_empty() {
        out.push(format!("{at}: accepted entries need evidence"));
    }
    for e in &t.evidence {
        if e.detail.trim().is_empty() {
            out.push(format!("{at}: evidence without detail"));
        }
        if matches!(e.kind, EvidenceKind::Maintainer) && !lexicon::has_date(&e.detail) {
            out.push(format!("{at}: a maintainer ruling must carry its date"));
        }
    }
}

/// A primary language subtag (ISO 639, 2–3 letters), optionally with BCP 47 subtags.
fn is_language(s: &str) -> bool {
    let mut parts = s.split('-');
    let primary = parts.next().unwrap_or_default();
    (2..=3).contains(&primary.len())
        && primary.bytes().all(|b| b.is_ascii_lowercase())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

#[cfg(test)]
mod tests {
    use super::check_profile;

    const GOOD: &str = r#"
schema: teigeodata/profile/v1
country: NP
iso3: NPL
name: Nepal
languages: [ne, en]
scripts: [Deva, Latn]
admin_levels:
  - { level: 1, english: Province, native: [{ lang: ne, script: Deva, text: "प्रदेश" }] }
  - { level: 2, english: District }
  - { level: 3, english: Local level }
unit_types:
  - id: rural_municipality
    level: 3
    english: Rural Municipality
    expected_count: 460
    abbreviations: [{ text: RM, status: proposed }]
    source_codes: { codab: "4" }
  - id: municipality
    level: 3
    english: Municipality
    source_codes: { codab: "3" }
droppable_suffixes:
  - { text: Dham, status: proposed }
sources:
  codab: { unit_type_code: { field: adm3_pcode, offset: 6, length: 1 } }
"#;

    #[test]
    fn accepts_a_valid_profile() {
        let p = check_profile("np", GOOD);
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn rejects_each_violation() {
        let cases = [
            ("country: NP", "country: IN", "must be `NP`"),
            ("iso3: NPL", "iso3: np", "three upper-case"),
            (
                "name: Nepal",
                "name: Nepal\nwikidata: P837",
                "Wikidata item ID",
            ),
            (
                "name: Nepal",
                "name: Nepal\nphonetic_key: soundex",
                "phonetic_key `soundex`",
            ),
            ("languages: [ne, en]", "languages: [nepali, en]", "BCP 47"),
            (
                "scripts: [Deva, Latn]",
                "scripts: [Deva, Latin]",
                "ISO 15924",
            ),
            (
                "level: 2, english: District",
                "level: 4, english: District",
                "out of order",
            ),
            (
                "    level: 3\n    english: Municipality",
                "    level: 9\n    english: Municipality",
                "not in admin_levels",
            ),
            ("id: municipality", "id: rural_municipality", "duplicate id"),
            (
                "source_codes: { codab: \"3\" }",
                "source_codes: { codab: \"4\" }",
                "is used by",
            ),
            (
                "source_codes: { codab: \"3\" }",
                "source_codes: { codab: \"33\" }",
                "must have length",
            ),
            (
                "source_codes: { codab: \"3\" }",
                "source_codes: { osm: \"3\" }",
                "not declared",
            ),
            (
                "{ text: RM, status: proposed }",
                "{ text: RM, status: accepted }",
                "need evidence",
            ),
            ("text: \"प्रदेश\"", "text: \"প্রদেশ\"", "outside Deva"),
            (
                "name: Nepal",
                "name: Nepal\ncapital: Kathmandu",
                "not a valid profile",
            ),
        ];
        for (from, to, expect) in cases {
            assert!(GOOD.contains(from), "test setup: `{from}` not found");
            let bad = GOOD.replacen(from, to, 1);
            let p = check_profile("np", &bad);
            assert!(
                p.iter().any(|m| m.contains(expect)),
                "expected `{expect}` for `{to}`, got {p:?}"
            );
        }
    }
}
