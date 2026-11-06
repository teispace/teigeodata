//! Lexicon rules for `data/countries/<cc>/lexicon.yaml`, schema `teigeodata/lexicon/v1`
//! ([ADR-0031](../../docs/03-decisions/adr/ADR-0031-canonical-spelling.md)).
//!
//! Beyond the shape (unknown fields are errors), it checks what a reviewer would otherwise have
//! to catch by eye: ID form and uniqueness, NFC text, native text in its declared script, a
//! variant that collides with another term's canonical spelling, evidence on accepted terms, and
//! an English gloss on admin suffixes.

use std::collections::BTreeMap;

use serde::Deserialize;
use unicode_normalization::is_nfc;
use unicode_script::{Script, UnicodeScript};

const SCHEMA: &str = "teigeodata/lexicon/v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LexiconFile {
    schema: String,
    country: String,
    updated: String,
    terms: Vec<Term>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Term {
    id: String,
    canonical: String,
    #[serde(default)]
    native: Vec<Native>,
    #[serde(default)]
    variants: Vec<String>,
    kind: Kind,
    meaning: String,
    #[serde(default)]
    english: Option<String>,
    status: Status,
    #[serde(default)]
    evidence: Vec<Evidence>,
}

#[derive(Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Generic,
    AdminSuffix,
    StreetType,
    Qualifier,
}

#[derive(Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Accepted,
    Proposed,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Native {
    pub(crate) lang: String,
    pub(crate) script: String,
    pub(crate) text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Evidence {
    pub(crate) kind: EvidenceKind,
    pub(crate) detail: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EvidenceKind {
    /// Counts of spellings in use (OSM, signage surveys).
    Usage,
    /// A ruling by the maintainer, with its date.
    Maintainer,
    /// An official gazetteer, gazette or government list.
    Official,
    /// Photographed signage.
    Signage,
    /// A dictionary or published standard.
    Dictionary,
}

/// Validates one file's text; `stem` is the file name without `.yaml` (the country code).
/// Returns the number of terms read and the problems found.
pub(crate) fn check_text(stem: &str, text: &str) -> (usize, Vec<String>) {
    let file: LexiconFile = match serde_saphyr::from_str(text) {
        Ok(f) => f,
        Err(e) => return (0, vec![format!("not a valid lexicon: {e}")]),
    };
    let mut p = Vec::new();
    if file.schema != SCHEMA {
        p.push(format!("schema is `{}`, expected `{SCHEMA}`", file.schema));
    }
    let cc = stem.to_ascii_uppercase();
    if file.country != cc || stem.len() != 2 || !stem.bytes().all(|b| b.is_ascii_lowercase()) {
        p.push(format!(
            "country `{}` must be the upper-case ISO code of the file name `{stem}.yaml`",
            file.country
        ));
    }
    if !is_date(&file.updated) {
        p.push(format!("updated `{}` is not YYYY-MM-DD", file.updated));
    }

    let prefix = format!("{stem}.");
    let mut ids: BTreeMap<&str, usize> = BTreeMap::new();
    // Every canonical spelling, folded, to the term that owns it.
    let canon: BTreeMap<String, &str> = file
        .terms
        .iter()
        .map(|t| (fold(&t.canonical), t.id.as_str()))
        .collect();
    for (i, t) in file.terms.iter().enumerate() {
        let at = format!("term `{}`", t.id);
        if let Some(first) = ids.insert(&t.id, i) {
            p.push(format!(
                "{at}: duplicate id (first at position {})",
                first + 1
            ));
        }
        if !t.id.starts_with(&prefix)
            || !t.id[prefix.len()..]
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            || t.id.len() == prefix.len()
        {
            p.push(format!("{at}: id must be `{prefix}<lowercase_ascii>`"));
        }
        for (field, value) in [("canonical", &t.canonical), ("meaning", &t.meaning)] {
            if value.trim().is_empty() || value.trim() != value {
                p.push(format!("{at}: {field} is empty or has surrounding spaces"));
            }
        }
        if !t.canonical.chars().all(is_latin_name_char) {
            p.push(format!(
                "{at}: canonical `{}` must be Latin script",
                t.canonical
            ));
        }
        check_variants(&at, t, &canon, &mut p);
        for n in &t.native {
            check_native(&at, n, &mut p);
        }
        for s in std::iter::once(&t.canonical)
            .chain(&t.variants)
            .chain(&t.english)
        {
            if !is_nfc(s) {
                p.push(format!("{at}: `{s}` is not NFC"));
            }
        }
        if t.kind == Kind::AdminSuffix && t.english.is_none() {
            p.push(format!("{at}: admin_suffix needs an `english` gloss"));
        }
        if t.kind != Kind::AdminSuffix && t.english.is_some() {
            p.push(format!("{at}: `english` is only for admin_suffix terms"));
        }
        if t.status == Status::Accepted && t.evidence.is_empty() {
            p.push(format!("{at}: accepted terms need evidence"));
        }
        for e in &t.evidence {
            if e.detail.trim().is_empty() {
                p.push(format!("{at}: evidence without detail"));
            }
            if matches!(e.kind, EvidenceKind::Maintainer) && !has_date(&e.detail) {
                p.push(format!(
                    "{at}: a maintainer ruling must carry its date (YYYY-MM-DD)"
                ));
            }
        }
    }
    (file.terms.len(), p)
}

fn check_variants(at: &str, t: &Term, canon: &BTreeMap<String, &str>, p: &mut Vec<String>) {
    for v in &t.variants {
        if fold(v) == fold(&t.canonical) {
            p.push(format!("{at}: variant `{v}` equals the canonical spelling"));
        } else if let Some(owner) = canon.get(&fold(v)) {
            p.push(format!(
                "{at}: variant `{v}` is the canonical spelling of `{owner}`"
            ));
        }
    }
    let mut seen = Vec::new();
    for v in &t.variants {
        let f = fold(v);
        if seen.contains(&f) {
            p.push(format!("{at}: variant `{v}` is listed twice"));
        }
        seen.push(f);
    }
}

pub(crate) fn check_native(at: &str, n: &Native, p: &mut Vec<String>) {
    let lang_ok = (2..=3).contains(&n.lang.len()) && n.lang.bytes().all(|b| b.is_ascii_lowercase());
    if !lang_ok {
        p.push(format!("{at}: lang `{}` is not an ISO 639 code", n.lang));
    }
    // Any ISO 15924 script, by the Unicode Script property (world-general, principle P16).
    let Some(script) = Script::from_short_name(&n.script) else {
        p.push(format!(
            "{at}: `{}` is not an ISO 15924 script code",
            n.script
        ));
        return;
    };
    if n.text.trim().is_empty() {
        p.push(format!("{at}: empty native text"));
    }
    if !is_nfc(&n.text) {
        p.push(format!("{at}: native `{}` is not NFC", n.text));
    }
    let foreign: Vec<char> = n
        .text
        .chars()
        .filter(|&c| {
            let s = c.script();
            !is_neutral(c) && s != script && s != Script::Common && s != Script::Inherited
        })
        .collect();
    if !foreign.is_empty() {
        p.push(format!(
            "{at}: native `{}` has characters outside {}: {foreign:?}",
            n.text, n.script
        ));
    }
}

/// Characters allowed in any script: spaces, joiners and common punctuation.
fn is_neutral(c: char) -> bool {
    matches!(
        c,
        ' ' | '-' | '\'' | '.' | '(' | ')' | '\u{200C}' | '\u{200D}'
    )
}

fn is_latin_name_char(c: char) -> bool {
    c.is_ascii_alphabetic()
        || matches!(c, ' ' | '-' | '\'' | '.')
        || ('\u{00C0}'..='\u{024F}').contains(&c)
        || ('\u{1E00}'..='\u{1EFF}').contains(&c)
}

/// Case-insensitive comparison key.
fn fold(s: &str) -> String {
    s.trim().to_lowercase()
}

pub(crate) fn has_date(s: &str) -> bool {
    s.char_indices()
        .any(|(i, _)| s.get(i..i + 10).is_some_and(is_date))
}

pub(crate) fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.get(4) == Some(&b'-')
        && b.get(7) == Some(&b'-')
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::check_text;

    const GOOD: &str = r#"
schema: teigeodata/lexicon/v1
country: NP
updated: 2026-11-06
terms:
  - id: np.chowk
    canonical: "Chowk"
    native:
      - { lang: ne, script: Deva, text: "चोक" }
    variants: ["Chok"]
    kind: generic
    meaning: "square"
    status: accepted
    evidence:
      - { kind: maintainer, detail: "2026-11-06: Lahachowk, not Lahachok" }
  - id: np.nagarpalika
    canonical: "Nagarpalika"
    native: []
    variants: []
    kind: admin_suffix
    meaning: "municipality"
    english: "Municipality"
    status: proposed
"#;

    #[test]
    fn accepts_a_valid_file() {
        let (n, p) = check_text("np", GOOD);
        assert_eq!(n, 2);
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn rejects_each_violation() {
        let cases = [
            ("country: NP", "country: IN", "upper-case ISO code"),
            ("id: np.chowk", "id: np.Chowk", "id must be"),
            ("text: \"चोक\"", "text: \"চোক\"", "outside Deva"),
            ("script: Deva", "script: Dvna", "not an ISO 15924"),
            (
                "variants: [\"Chok\"]",
                "variants: [\"Nagarpalika\"]",
                "canonical spelling of",
            ),
            (
                "variants: [\"Chok\"]",
                "variants: [\"chowk\"]",
                "equals the canonical",
            ),
            (
                "    english: \"Municipality\"\n",
                "",
                "needs an `english` gloss",
            ),
            (
                "    status: accepted\n    evidence:\n      - { kind: maintainer, detail: \"2026-11-06: Lahachowk, not Lahachok\" }\n",
                "    status: accepted\n",
                "need evidence",
            ),
            ("kind: generic", "kind: generik", "not a valid lexicon"),
            (
                "meaning: \"square\"",
                "meaning: \"square\"\n    colour: red",
                "not a valid lexicon",
            ),
            (
                "canonical: \"Chowk\"",
                "canonical: \"चोक\"",
                "must be Latin",
            ),
            // U+0958 (क़) is a composition exclusion: NFC is क + nukta, so the precomposed form is not NFC.
            ("text: \"चोक\"", "text: \"\u{0958}\"", "not NFC"),
            ("2026-11-06: Lahachowk", "Lahachowk", "carry its date"),
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
    }

    #[test]
    fn duplicate_ids_are_reported() {
        let bad = GOOD.replacen("id: np.nagarpalika", "id: np.chowk", 1);
        let (_, p) = check_text("np", &bad);
        assert!(p.iter().any(|m| m.contains("duplicate id")), "{p:?}");
    }
}
