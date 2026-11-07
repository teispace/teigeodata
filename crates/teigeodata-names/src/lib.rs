//! Name normalisation and phonetic keys for teigeodata ([ADR-0014](https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/adr/ADR-0014-names-and-search.md)).
//!
//! One code path for every stage that compares names: the forge's matcher, the search index
//! and queries. World-general (principle P16): folds are per Unicode script, never per
//! country, and a country's profile picks its phonetic key by name.
//!
//! ```
//! use teigeodata_names::{PhoneticKey, fold, join_key};
//!
//! assert_eq!(fold("São Paulo"), "sao paulo");
//! assert_eq!(join_key("Lahan Bazar"), "lahanbazar");
//! let key = PhoneticKey::from_name("indic-v0").map(|k| k.key("Lahachowk"));
//! assert_eq!(key, PhoneticKey::from_name("indic-v0").map(|k| k.key("लहाचोक")));
//! ```

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use unicode_normalization::UnicodeNormalization;
use unicode_script::{Script, UnicodeScript};

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
///
/// ASCII text takes a fast path with the same result (NFKD and NFC are the identity on ASCII,
/// and it has no marks): about 10x faster, tested against the full path.
#[must_use]
pub fn fold(s: &str) -> String {
    if s.is_ascii() {
        return fold_ascii(s);
    }
    fold_full(s)
}

/// [`fold`] for ASCII: lower-cased runs of `[A-Za-z0-9]` joined by single spaces.
fn fold_ascii(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for token in s
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
    {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(token);
    }
    out.make_ascii_lowercase();
    out
}

/// [`fold`] for any text.
fn fold_full(s: &str) -> String {
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

/// The fold with spaces removed, so "Lahan Bazar" meets "Lahanbazar".
#[must_use]
pub fn join_key(s: &str) -> String {
    fold(s).replace(' ', "")
}

/// The search map a key belongs to (the SEARCH section's three maps, D-085).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SearchMap {
    /// The fold.
    Exact,
    /// The fold without spaces, and without a droppable suffix.
    Loose,
    /// The phonetic key.
    Phonetic,
}

/// Calls `out` with every search key of `name` (D-052, D-086): its fold (exact); the fold
/// without spaces, and again with each droppable suffix removed (loose); and its phonetic key
/// when the profile names one. `droppable` holds the profile's droppable suffixes, folded.
/// Keys may repeat or be empty; the SEARCH writer drops empty ones.
pub fn search_keys(
    name: &str,
    droppable: &[String],
    phonetic: Option<PhoneticKey>,
    mut out: impl FnMut(SearchMap, String),
) {
    let f = fold(name);
    out(SearchMap::Loose, f.replace(' ', ""));
    for d in droppable {
        if let Some(stem) = f.strip_suffix(d.as_str()) {
            out(SearchMap::Loose, stem.replace(' ', ""));
        }
    }
    if let Some(p) = phonetic {
        out(SearchMap::Phonetic, p.key(name));
    }
    out(SearchMap::Exact, f);
}

/// The first script of a text that is not Common or Inherited.
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

/// A named phonetic key algorithm. A country's profile names the one its names need
/// (`phonetic_key`); keys are candidate generators, so false merges are expected and ranking
/// or matching sorts them out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhoneticKey {
    /// `indic-v0`: Indic place names in Latin or Devanagari (names-and-search §5.3). Nasals
    /// are resolved before the non-initial A is dropped; Devanagari word-final schwa is
    /// deleted; a written velar nasal stays unless a consonant follows.
    IndicV0,
    /// `indic-v1`: `indic-v0` plus a dropped word-final I or U after a consonant ("Tole" meets
    /// टोल). Not adopted: spike S3 found it merges more distinct places than it joins.
    IndicV1,
}

impl PhoneticKey {
    /// Every algorithm, by its profile name.
    pub const ALL: [(&'static str, Self); 2] =
        [("indic-v0", Self::IndicV0), ("indic-v1", Self::IndicV1)];

    /// The algorithm a profile names, if it exists.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().find(|(n, _)| *n == name).map(|(_, k)| *k)
    }

    /// The key of a name.
    #[must_use]
    pub fn key(self, s: &str) -> String {
        match self {
            Self::IndicV0 => indic(s, false),
            Self::IndicV1 => indic(s, true),
        }
    }
}

fn indic(s: &str, drop_final_vowel: bool) -> String {
    let folded = fold(s);
    let mut classes: Vec<char> = Vec::new();
    for word in folded.split(' ') {
        // A word may mix scripts ("Anantaपुर"): classify each script run with its own rules.
        let is_deva = |c: char| ('\u{0900}'..='\u{097F}').contains(&c);
        let mut w = Vec::new();
        let mut run = String::new();
        let mut run_deva = false;
        for c in word.chars().chain(core::iter::once('\0')) {
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
            && out
                .len()
                .checked_sub(2)
                .and_then(|i| out.get(i))
                .is_some_and(|c| !is_vowel_class(*c))
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
                None => i
                    .checked_sub(1)
                    .and_then(|p| w.get(p))
                    .is_some_and(|p| is_vowel_class(*p)),
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
            (
                latin_letter(at(i).unwrap_or('x'), prev_vowel, prev_consonant),
                1,
            )
        };
        if let Some(c) = class {
            out.push(c);
        }
        i += len;
    }
    out
}

/// The class of one Latin letter; `h` and `y` after a consonant mark aspiration or a glide.
fn latin_letter(c: char, prev_vowel: bool, prev_consonant: bool) -> Option<char> {
    match c {
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
    }
}

fn deva_classes(word: &str) -> Vec<char> {
    let chars: Vec<char> = word.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
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
            let after_consonant =
                i >= 2 && i.checked_sub(1).and_then(|p| chars.get(p)) == Some(&'\u{094D}');
            if !(k == 'Y' && after_consonant) {
                out.push(k);
            }
            i += 1;
            out.extend(inherent(&chars, i));
            continue;
        }
        let v = match c {
            'अ' | 'आ' | 'ा' => Some(&['A'][..]),
            'इ' | 'ई' | 'ए' | 'ऐ' | 'ि' | 'ी' | 'े' | 'ै' => Some(&['I'][..]),
            'उ' | 'ऊ' | 'ओ' | 'औ' | 'ु' | 'ू' | 'ो' | 'ौ' => Some(&['U'][..]),
            'ऋ' | 'ृ' => Some(&['R', 'I'][..]),
            'ं' => Some(&['N'][..]),
            _ => None,
        };
        if let Some(v) = v {
            out.extend_from_slice(v);
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
mod fold_tests {
    use super::{fold, fold_ascii, fold_full};
    use alloc::string::String;

    /// The ASCII fast path equals the full fold: every string of one or two ASCII characters,
    /// and 200,000 pseudo-random ASCII strings up to 24 characters.
    #[test]
    fn ascii_fast_path_equals_the_full_fold() {
        let mut buf = String::new();
        for a in 0u8..128 {
            for b in 0u8..=128 {
                buf.clear();
                buf.push(char::from(a));
                if b < 128 {
                    buf.push(char::from(b));
                }
                assert_eq!(fold_ascii(&buf), fold_full(&buf), "{buf:?}");
            }
        }
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        for _ in 0..200_000 {
            buf.clear();
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let len = usize::try_from(state % 25).unwrap_or(0);
            for k in 0..len {
                // Bias towards letters, digits, spaces and punctuation names use.
                let r = (state >> (k * 2 % 56)).to_le_bytes()[0];
                let c = match r % 8 {
                    0 => b' ',
                    1 => b"-.,'/()&"[usize::from(r / 8 % 8)],
                    2 => b'0' + r / 8 % 10,
                    _ => (r / 8) % 128,
                };
                buf.push(char::from(c));
            }
            assert_eq!(fold_ascii(&buf), fold_full(&buf), "{buf:?}");
        }
        assert_eq!(fold("  St. Mary's--ROAD 12 "), "st mary s road 12");
    }

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

#[cfg(test)]
mod pkey_tests {
    use super::PhoneticKey;
    use alloc::string::String;
    use alloc::vec::Vec;

    fn k(s: &str) -> String {
        PhoneticKey::IndicV0.key(s)
    }

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
            assert!(
                keys.windows(2).all(|w| w.first() == w.get(1)),
                "{g:?} → {keys:?}"
            );
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

#[cfg(test)]
mod search_key_tests {
    use super::{PhoneticKey, SearchMap, search_keys};
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;

    fn keys(name: &str, droppable: &[&str], p: Option<PhoneticKey>) -> Vec<(SearchMap, String)> {
        let d: Vec<String> = droppable.iter().map(ToString::to_string).collect();
        let mut out = Vec::new();
        search_keys(name, &d, p, |m, k| out.push((m, k)));
        out.sort();
        out
    }

    #[test]
    fn every_map_gets_its_keys() {
        let got = keys(
            "Lahan Bazar",
            &["bazar", "chowk"],
            Some(PhoneticKey::IndicV0),
        );
        let s = |m, k: &str| (m, k.to_string());
        assert_eq!(
            got,
            [
                s(SearchMap::Exact, "lahan bazar"),
                s(SearchMap::Loose, "lahan"),
                s(SearchMap::Loose, "lahanbazar"),
                s(
                    SearchMap::Phonetic,
                    &PhoneticKey::IndicV0.key("Lahan Bazar")
                ),
            ]
        );
        // No phonetic key without a profile key; no stem when no suffix matches.
        assert_eq!(
            keys("Pokhara", &["bazar"], None),
            [
                s(SearchMap::Exact, "pokhara"),
                s(SearchMap::Loose, "pokhara")
            ]
        );
    }
}
