//! Phase 0 spike S3: shared loading and text folding for the golden-set generator and the
//! search evaluation. World-general (principle P16): nothing here names a country. Country
//! rules come from `data/countries/<cc>/profile.yaml`; script rules are per Unicode script.
//!
//! Sources: a `GeoNames` country file `<CC>.txt` (CC-BY 4.0) and, where the country has one, an
//! OCHA COD-AB attribute workbook (licence per dataset; Nepal v02 is CC BY-IGO). Records keep
//! their source key (`geonames:<id>`, `codab:<pcode>`); records of the same entity across
//! sources share a group.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    missing_docs,
    missing_debug_implementations,
    reason = "a measurement spike over pinned local inputs"
)]

use std::collections::BTreeMap;

use calamine::{Data, Reader, Xlsx, open_workbook};
use serde::Deserialize;
use unicode_normalization::UnicodeNormalization;
use unicode_script::{Script, UnicodeScript};

// ---------- country profile (the subset this spike reads; full schema: xtask check-data) ----

#[derive(Deserialize)]
pub struct Profile {
    pub country: String,
    pub iso3: String,
    #[serde(default)]
    pub scripts: Vec<String>,
    #[serde(default)]
    pub unit_types: Vec<UnitType>,
    #[serde(default)]
    pub droppable_suffixes: Vec<ProfileTerm>,
    #[serde(default)]
    pub sources: BTreeMap<String, SourceRules>,
}

#[derive(Deserialize)]
pub struct UnitType {
    pub id: String,
    pub level: u8,
    #[serde(default)]
    pub abbreviations: Vec<ProfileTerm>,
    #[serde(default)]
    pub source_codes: BTreeMap<String, String>,
}

#[derive(Deserialize)]
pub struct ProfileTerm {
    pub text: String,
}

#[derive(Deserialize)]
pub struct SourceRules {
    pub unit_type_code: Option<CodeSlice>,
}

#[derive(Deserialize)]
pub struct CodeSlice {
    pub field: String,
    pub offset: usize,
    pub length: usize,
}

#[must_use]
pub fn profile(path: &str) -> Profile {
    serde_saphyr::from_str(&std::fs::read_to_string(path).expect("read profile"))
        .expect("parse profile")
}

// ---------- records ----------

/// One searchable record.
#[derive(Clone, Debug)]
pub struct Rec {
    pub key: String,
    pub name: String,
    pub alts: Vec<String>,
    pub fcode: String,
    pub district: String,
    pub importance: u32,
    pub current: bool,
    /// The entity group: records of the same entity across sources share it.
    pub group: usize,
    /// The profile's unit type, for admin units from a source that encodes it.
    pub unit_type: Option<String>,
}

// ---------- folding (world-general, per script) ----------

/// Scripts whose combining marks are optional accents, dropped for search (é → e, й → и).
/// Everywhere else marks carry meaning (Indic vowel signs, Arabic harakat, Thai vowels and tone
/// marks) and only script-specific folds apply.
fn accents_optional(s: Script) -> bool {
    matches!(s, Script::Latin | Script::Greek | Script::Cyrillic)
}

/// Script-level search folds for Brahmic scripts: drop the nukta, chandrabindu → anusvara.
/// One rule for every Brahmic script that has the signs; no country is named.
enum Fold {
    Keep,
    Drop,
    Map(char),
}

fn brahmic_fold(c: char) -> Fold {
    match c {
        // nukta: Devanagari, Bengali, Gurmukhi, Gujarati, Oriya, Telugu, Kannada
        '\u{093C}' | '\u{09BC}' | '\u{0A3C}' | '\u{0ABC}' | '\u{0B3C}' | '\u{0C3C}'
        | '\u{0CBC}' => Fold::Drop,
        // chandrabindu → anusvara
        '\u{0901}' => Fold::Map('\u{0902}'),
        '\u{0981}' => Fold::Map('\u{0982}'),
        '\u{0A81}' => Fold::Map('\u{0A82}'),
        '\u{0B01}' => Fold::Map('\u{0B02}'),
        '\u{0C01}' => Fold::Map('\u{0C02}'),
        _ => Fold::Keep,
    }
}

/// The search fold: NFKD; lower case; optional accents dropped by script; Brahmic folds;
/// ZWJ/ZWNJ dropped; tokens of letters, digits and in-script marks joined by single spaces;
/// recomposed to NFC.
#[must_use]
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    let mut base = Script::Common;
    for c in s.nfkd() {
        if matches!(c, '\u{200C}' | '\u{200D}') {
            continue;
        }
        let c = match brahmic_fold(c) {
            Fold::Map(r) => r,
            Fold::Drop => continue,
            Fold::Keep => c,
        };
        if unicode_normalization::char::is_combining_mark(c) {
            if !accents_optional(base) && !out.is_empty() && !space {
                out.push(c);
            }
            continue;
        }
        if c.is_alphanumeric() {
            base = c.script();
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.extend(c.to_lowercase());
        } else {
            space = true;
        }
    }
    out.nfc().collect()
}

#[must_use]
pub fn script_of(s: &str) -> Script {
    s.chars()
        .map(|c| c.script())
        .find(|x| !matches!(x, Script::Common | Script::Inherited))
        .unwrap_or(Script::Common)
}

/// True if the text is written in a script other than Latin (a native-script name).
#[must_use]
pub fn is_native_script(s: &str) -> bool {
    let sc = script_of(s);
    sc != Script::Latin && sc != Script::Common && s.chars().all(|c| !c.is_ascii_alphabetic())
}

fn fcode_importance(fcode: &str, population: u64) -> u32 {
    let base = match fcode {
        "PPLC" => 100,
        "ADM1" => 90,
        "PPLA" => 80,
        "ADM2" => 70,
        "PPLA2" => 60,
        "PPLA3" => 50,
        "ADM3" => 45,
        "PPLA4" | "ADM4" => 40,
        "PPL" | "PPLX" => 30,
        "PPLL" | "PPLF" => 10,
        f if f.ends_with('H') => 5,
        _ => 1,
    };
    let pop = if population > 0 {
        ((population as f64).log10() * 5.0) as u32
    } else {
        0
    };
    base + pop
}

/// Loads a `GeoNames` country file and, optionally, a COD-AB workbook. COD-AB units at the
/// profile's unit-type level join a `GeoNames` record of the same admin level, folded name and
/// parent unit.
pub fn load(profile: &Profile, geonames: &str, codab_xlsx: Option<&str>) -> Vec<Rec> {
    let text = std::fs::read_to_string(geonames).expect("read GeoNames file");
    let rows: Vec<Vec<&str>> = text
        .lines()
        .map(|l| l.split('\t').collect())
        .filter(|f: &Vec<&str>| f.len() >= 19)
        .collect();
    // The parent shown as context: the ADM2 unit. GeoNames admin2 codes are country-specific
    // strings; map both the ADM2 row's own code and its ID.
    let mut adm2_name: BTreeMap<String, &str> = BTreeMap::new();
    for f in &rows {
        if f[7] == "ADM2" {
            adm2_name.insert(format!("{}.{}", f[10], f[11]), f[1]);
        }
    }
    let mut recs = Vec::new();
    for f in &rows {
        if !matches!(f[6], "P" | "A") {
            continue;
        }
        let fcode = f[7];
        let population: u64 = f[14].parse().unwrap_or(0);
        let district = if fcode == "ADM2" {
            f[1]
        } else {
            adm2_name
                .get(&format!("{}.{}", f[10], f[11]))
                .copied()
                .unwrap_or("")
        };
        let mut alts: Vec<String> = f[3]
            .split(',')
            .filter(|a| !a.is_empty() && *a != f[1])
            .map(str::to_string)
            .collect();
        alts.sort();
        alts.dedup();
        recs.push(Rec {
            key: format!("geonames:{}", f[0]),
            name: f[1].to_string(),
            alts,
            fcode: fcode.to_string(),
            district: district.to_string(),
            importance: fcode_importance(fcode, population),
            current: !fcode.ends_with('H'),
            group: 0,
            unit_type: None,
        });
    }
    for (i, r) in recs.iter_mut().enumerate() {
        r.group = i;
    }
    let (Some(path), Some(level)) = (codab_xlsx, profile.unit_types.first().map(|u| u.level))
    else {
        return recs;
    };
    let rules = profile
        .sources
        .get("codab")
        .and_then(|s| s.unit_type_code.as_ref());

    let mut wb: Xlsx<_> = open_workbook(path).expect("open COD-AB workbook");
    let sheet = format!("{}_admin{level}", profile.iso3.to_lowercase());
    let range = wb.worksheet_range(&sheet).expect("COD-AB sheet");
    let mut it = range.rows();
    let header: Vec<String> = it.next().expect("header").iter().map(cell).collect();
    let col = |n: &str| {
        header
            .iter()
            .position(|h| h == n)
            .unwrap_or_else(|| panic!("column {n}"))
    };
    let (c_name, c_pcode, c_parent) = (
        col(&format!("adm{level}_name")),
        col(&format!("adm{level}_pcode")),
        col("adm2_name"),
    );
    let code_col = rules.map(|r| col(&r.field));
    let by_code: BTreeMap<&str, &UnitType> = profile
        .unit_types
        .iter()
        .filter_map(|u| u.source_codes.get("codab").map(|c| (c.as_str(), u)))
        .collect();
    let base = recs.len();
    // Parents: COD-AB may spell them differently from GeoNames (Chitawan/Chitwan); match by
    // fold, else by the smallest edit distance ≤ 2.
    let gn_parents: Vec<String> = adm2_name.values().map(|s| fold(s)).collect();
    let map_parent = |d: &str| -> String {
        let f = fold(d);
        if gn_parents.contains(&f) {
            return f;
        }
        gn_parents
            .iter()
            .map(|g| (levenshtein(g, &f), g))
            .filter(|(k, _)| *k <= 2)
            .min()
            .map_or(f, |(_, g)| g.clone())
    };
    let gn_level = format!("ADM{level}");
    let mut same_level: BTreeMap<(String, String), usize> = BTreeMap::new();
    for r in &recs {
        if r.fcode == gn_level {
            same_level.insert((join_key(&r.name), fold(&r.district)), r.group);
        }
    }
    for (j, row) in it.enumerate() {
        let name = cell(&row[c_name]);
        let pcode = cell(&row[c_pcode]);
        let parent = cell(&row[c_parent]);
        let unit = match (rules, code_col) {
            (Some(r), Some(c)) => cell(&row[c])
                .get(r.offset..r.offset + r.length)
                .and_then(|code| by_code.get(code))
                .map(|u| u.id.clone()),
            _ => None,
        };
        // Importance by the unit type's position in the profile (first = most important).
        let rank = unit
            .as_ref()
            .and_then(|u| profile.unit_types.iter().position(|t| &t.id == u))
            .unwrap_or(profile.unit_types.len());
        let importance = 85u32.saturating_sub(10 * rank as u32).max(20);
        let group = same_level
            .get(&(join_key(&name), map_parent(&parent)))
            .copied()
            .unwrap_or(base + j);
        recs.push(Rec {
            key: format!("codab:{pcode}"),
            name,
            alts: Vec::new(),
            fcode: format!("CODAB{level}"),
            district: parent,
            importance,
            current: true,
            group,
            unit_type: unit,
        });
    }
    recs
}

/// The cross-source join key: the fold without spaces, since sources split compound names
/// differently ("Necha Salyan" and "Nechasalyan").
fn join_key(s: &str) -> String {
    fold(s).replace(' ', "")
}

fn cell(d: &Data) -> String {
    match d {
        Data::String(s) => s.trim().to_string(),
        Data::Float(f) => format!("{f}"),
        Data::Int(i) => i.to_string(),
        _ => String::new(),
    }
}

/// Levenshtein distance on chars.
#[must_use]
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// A tiny deterministic generator (xorshift64).
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
}

// ---------- lexicon (the subset this spike reads; full schema: xtask check-data) ----------

#[derive(Deserialize)]
pub struct Lexicon {
    pub terms: Vec<LexTerm>,
}

#[derive(Deserialize)]
pub struct LexTerm {
    pub canonical: String,
    #[serde(default)]
    pub variants: Vec<String>,
    pub kind: String,
    #[serde(default)]
    pub native: Vec<LexNative>,
}

#[derive(Deserialize)]
pub struct LexNative {
    pub text: String,
}

impl LexTerm {
    /// The canonical spelling first, then the variants.
    pub fn spellings(&self) -> impl Iterator<Item = &String> {
        std::iter::once(&self.canonical).chain(&self.variants)
    }
}

#[must_use]
pub fn lexicon(path: &str) -> Lexicon {
    serde_saphyr::from_str(&std::fs::read_to_string(path).expect("read lexicon"))
        .expect("parse lexicon")
}

#[cfg(test)]
mod tests {
    use super::fold;

    #[test]
    fn folds_are_script_aware() {
        // Latin, Greek and Cyrillic accents are optional.
        assert_eq!(fold("Pātan"), "patan");
        assert_eq!(fold("Dhangaḍhi̇̄"), "dhangadhi");
        assert_eq!(fold("Ἀθῆναι"), "αθηναι");
        assert_eq!(fold("Йошкар-Ола"), "иошкар ола");
        assert_eq!(fold("São Paulo"), "sao paulo");
        // Indic vowel signs and the virama are kept; nukta and chandrabindu fold.
        assert_eq!(fold("काठमाडौं"), "काठमाडौं");
        assert_eq!(fold("गाँउ"), "गांउ");
        assert_eq!(fold("ड़"), "ड");
        assert_eq!(fold("ক্ষ"), "ক্ষ");
        // Arabic and Thai marks are kept.
        assert_eq!(fold("مُحَمَّد"), "مُحَمَّد");
        assert_eq!(fold("ภูเก็ต"), "ภูเก็ต");
        // Han and Hangul pass through.
        assert_eq!(fold("東京"), "東京");
        assert_eq!(fold("서울"), "서울");
    }
}

// ---------- phonetic keys (a library of named algorithms; a country's profile picks them) ----

/// `indic-v0`: the phonetic key of names-and-search §5.3 for Indic place names written in
/// Latin or Devanagari. A candidate generator only: false merges are expected and ranking
/// sorts them out. Step order fixed by this spike: nasals are resolved before the non-initial
/// A is dropped (the note's hand examples needed both orders; pre-vowel nasals are real
/// consonants, so they stay).
#[must_use]
pub fn pkey_indic_v0(s: &str) -> String {
    pkey_indic(s, false)
}

/// `indic-v1` candidate: `indic-v0` plus a dropped word-final I or U after a consonant, so
/// Latin "Tole", "Kote" and "Pokhary" meet Devanagari टोल, कोट and पोखरी. Spike S3 A/B only.
#[must_use]
pub fn pkey_indic_v1(s: &str) -> String {
    pkey_indic(s, true)
}

fn pkey_indic(s: &str, drop_final_vowel: bool) -> String {
    let folded = fold(s);
    let mut classes: Vec<char> = Vec::new();
    for word in folded.split(' ') {
        // A word may mix scripts ("Anantaपुर"): classify each script run with its own rules.
        let is_deva = |c: char| ('\u{0900}'..='\u{097F}').contains(&c);
        let mut w = Vec::new();
        let mut run = String::new();
        let mut run_deva = false;
        for c in word.chars().chain(std::iter::once('\0')) {
            if c == '\0' || (!run.is_empty() && is_deva(c) != run_deva) {
                w.extend(if run_deva {
                    deva_classes(&run)
                } else {
                    latin_classes(&run)
                });
                run.clear();
            }
            if c != '\0' {
                run_deva = is_deva(c);
                run.push(c);
            }
        }
        w = resolve_nasals(&w);
        // Drop every A that is not word-initial.
        let mut out: Vec<char> = Vec::with_capacity(w.len());
        for (i, c) in w.iter().enumerate() {
            if *c == 'A' && i > 0 {
                continue;
            }
            out.push(*c);
        }
        if drop_final_vowel
            && out.len() > 2
            && matches!(out.last(), Some('I' | 'U'))
            && !is_vowel_class(out[out.len() - 2])
        {
            out.pop();
        }
        classes.extend(out);
    }
    classes.dedup();
    if classes.is_empty() {
        return folded;
    }
    classes.into_iter().collect()
}

fn is_vowel_class(c: char) -> bool {
    matches!(c, 'A' | 'I' | 'U')
}

/// `Ŋ` (a written velar nasal) stays as N unless a consonant follows. Other nasals (N, M) before a consonant class
/// are dropped; a word-final nasal after a vowel is dropped; otherwise M → N except before B or
/// P.
fn resolve_nasals(w: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(w.len());
    for (i, &c) in w.iter().enumerate() {
        if c == 'Ŋ' && w.get(i + 1).is_none_or(|n| is_vowel_class(*n)) {
            // A velar nasal written as a full consonant (ङ, final "ng") stays, unless a
            // consonant follows (ङ्ग is homorganic, like any nasal before a consonant).
            out.push('N');
        } else if c == 'N' || c == 'M' || c == 'Ŋ' {
            let dropped = match w.get(i + 1) {
                Some(&n) => !is_vowel_class(n),
                None => i > 0 && is_vowel_class(w[i - 1]),
            };
            if !dropped {
                let labial = matches!(w.get(i + 1), Some('B' | 'P'));
                out.push(if c == 'M' && labial { 'M' } else { 'N' });
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn latin_classes(word: &str) -> Vec<char> {
    let b: Vec<char> = word.chars().filter(char::is_ascii_lowercase).collect();
    let n = b.len();
    let at = |i: usize| b.get(i).copied();
    let is_v = |c: Option<char>| matches!(c, Some('a' | 'e' | 'i' | 'o' | 'u'));
    let starts = |i: usize, p: &str| p.chars().enumerate().all(|(k, c)| at(i + k) == Some(c));
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let prev_vowel = out.last().is_some_and(|c| is_vowel_class(*c));
        let prev_consonant = out.last().is_some_and(|c| !is_vowel_class(*c));
        let (class, len): (Option<char>, usize) = if starts(i, "kchh") {
            (Some('C'), 4)
        } else if starts(i, "ksh") || starts(i, "chh") {
            (Some('C'), 3)
        } else if starts(i, "ch") {
            (Some('C'), 2)
        } else if starts(i, "ck") {
            (Some('K'), 2)
        } else if at(i) == Some('c') {
            (
                Some(if matches!(at(i + 1), Some('a' | 'o' | 'u')) {
                    'K'
                } else {
                    'C'
                }),
                1,
            )
        } else if at(i) == Some('x') {
            (Some('C'), 1)
        } else if starts(i, "sh") || starts(i, "ss") {
            (Some('S'), 2)
        } else if (starts(i, "gy") || starts(i, "jn") || starts(i, "gn")) && is_v(at(i + 2)) {
            (Some('G'), 2)
        } else if starts(i, "jh") {
            (Some('J'), 2)
        } else if starts(i, "ph")
            || starts(i, "bh")
            || starts(i, "kh")
            || starts(i, "gh")
            || starts(i, "th")
            || starts(i, "dh")
            || starts(i, "rh")
        {
            (
                Some(match at(i).unwrap_or('x') {
                    'p' => 'P',
                    'b' => 'B',
                    'k' => 'K',
                    'g' => 'G',
                    't' => 'T',
                    // 'd' and 'r' ("rh" as in Garhi is the flap ढ़)
                    _ => 'D',
                }),
                2,
            )
        } else if starts(i, "ng") && i + 2 == n {
            (Some('Ŋ'), 2)
        } else if starts(i, "aa") {
            (Some('A'), 2)
        } else if starts(i, "ee")
            || starts(i, "ii")
            || starts(i, "ai")
            || starts(i, "ei")
            || (starts(i, "ay") && i + 2 == n)
        {
            (Some('I'), 2)
        } else if starts(i, "oo")
            || starts(i, "uu")
            || starts(i, "au")
            || starts(i, "ou")
            || starts(i, "ow")
        {
            (Some('U'), 2)
        } else {
            let c = at(i).unwrap_or('x');
            let class = match c {
                'a' => Some('A'),
                'e' | 'i' => Some('I'),
                'o' | 'u' => Some('U'),
                'w' if prev_vowel => Some('U'),
                'w' | 'b' | 'v' => Some('B'),
                'f' | 'p' => Some('P'),
                'z' | 'j' => Some('J'),
                's' => Some('S'),
                'q' | 'k' => Some('K'),
                'g' => Some('G'),
                't' => Some('T'),
                'd' => Some('D'),
                'm' => Some('M'),
                'n' => Some('N'),
                'r' => Some('R'),
                'l' => Some('L'),
                // h and y after a consonant mark aspiration or a glide: dropped.
                'h' | 'y' if prev_consonant => None,
                'h' => Some('H'),
                'y' => Some('Y'),
                _ => None,
            };
            (class, 1)
        };
        if let Some(c) = class {
            out.push(c);
        }
        i += len;
    }
    out
}

fn deva_classes(word: &str) -> Vec<char> {
    let chars: Vec<char> = word.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // क्ष → C, ज्ञ → G
        if (c == 'क' || c == 'ज')
            && chars.get(i + 1) == Some(&'\u{094D}')
            && matches!(chars.get(i + 2), Some('ष' | 'ञ'))
        {
            out.push(if c == 'क' { 'C' } else { 'G' });
            i += 3;
            out.extend(inherent(&chars, i));
            continue;
        }
        let consonant = match c {
            'क' | 'ख' => Some('K'),
            'ग' | 'घ' => Some('G'),
            'ङ' => Some('Ŋ'),
            'ञ' | 'ण' | 'न' => Some('N'),
            'च' | 'छ' => Some('C'),
            'ज' | 'झ' => Some('J'),
            'ट' | 'ठ' | 'त' | 'थ' => Some('T'),
            'ड' | 'ढ' | 'द' | 'ध' => Some('D'),
            'प' | 'फ' => Some('P'),
            'ब' | 'भ' | 'व' => Some('B'),
            'म' => Some('M'),
            'य' => Some('Y'),
            'र' => Some('R'),
            'ल' => Some('L'),
            'श' | 'ष' | 'स' => Some('S'),
            'ह' => Some('H'),
            _ => None,
        };
        if let Some(k) = consonant {
            // y after a consonant is dropped, as in the Latin path.
            let after_consonant = i >= 2 && chars[i - 1] == '\u{094D}';
            if !(k == 'Y' && after_consonant) {
                out.push(k);
            }
            i += 1;
            out.extend(inherent(&chars, i));
            continue;
        }
        let v = match c {
            'अ' | 'आ' | 'ा' => Some(vec!['A']),
            'इ' | 'ई' | 'ए' | 'ऐ' | 'ि' | 'ी' | 'े' | 'ै' => Some(vec!['I']),
            'उ' | 'ऊ' | 'ओ' | 'औ' | 'ु' | 'ू' | 'ो' | 'ौ' => Some(vec!['U']),
            'ऋ' | 'ृ' => Some(vec!['R', 'I']),
            'ं' => Some(vec!['N']),
            _ => None,
        };
        if let Some(v) = v {
            out.extend(v);
        }
        i += 1;
    }
    out
}

/// The inherent vowel A after a consonant at `i - 1`, unless a virama or a vowel sign follows,
/// or the consonant ends the word (word-final schwa deletion, as Nepali and Hindi speak it).
fn inherent(chars: &[char], i: usize) -> Option<char> {
    match chars.get(i) {
        None | Some('\u{094D}') => None,
        Some(c) if ('\u{093E}'..='\u{094C}').contains(c) || *c == '\u{0943}' => None,
        _ => Some('A'),
    }
}

#[cfg(test)]
mod pkey_tests {
    use super::pkey_indic_v0 as k;

    #[test]
    fn variants_share_a_key() {
        let groups: [&[&str]; 8] = [
            &["Lahachowk", "Lahachok", "Lahachouk", "लहाचोक"],
            &["Kathmandu", "Kathamandu", "काठमाडौं"],
            &["Bazar", "Bajar", "Bazaar", "बजार"],
            &["Gaun", "Gaon", "Gau", "गाउँ"],
            &["Bhanjyang", "Bhanjang", "भञ्ज्याङ"],
            &["Deurali", "Dewrali", "Deorali", "देउराली"],
            &["Biratnagar", "Viratnagar", "विराटनगर"],
            &["Gadhi", "Garhi", "गढी", "गढ़ी"],
        ];
        for g in groups {
            let keys: Vec<String> = g.iter().map(|s| k(s)).collect();
            assert!(keys.windows(2).all(|w| w[0] == w[1]), "{g:?} → {keys:?}");
        }
    }

    #[test]
    fn distinct_places_differ() {
        assert_eq!(k("Anantapur"), k("Anantaपुर"));
        assert_eq!(k("Dhunga"), k("ढुङ्गा"));
        assert_ne!(k("Pokhara"), k("Pokhari"));
        assert_ne!(k("Lalitpur"), k("Lamjung"));
    }
}
