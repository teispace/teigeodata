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

pub mod romanise;

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
        _ => vowel_fold(c).map_or(Fold::Keep, Fold::Map),
    }
}

/// Vowel length and the borrowed vowels, folded for search (D-220): long ई ऊ and ी ू to short
/// इ उ and ि ु, and the candra vowels ऑ ॉ ऍ ॅ to ओ ो ए े. Spelling varies most here
/// (न्यूयोर्क, न्युयोर्क, न्यूयॉर्क); the stored name keeps its spelling and ranking settles the
/// few names that differ only in length. One rule over the shared block offsets.
fn vowel_fold(c: char) -> Option<char> {
    let (block, k) = brahmic_offset(c)?;
    let to = match k {
        0x08 | 0x0A | 0x40 | 0x42 => k - 1,
        0x0D | 0x11 | 0x45 | 0x49 => k + 2,
        _ => return None,
    };
    char::from_u32(block + to)
}

/// Brahmic blocks whose letters sit at Devanagari's offsets (velar nasal at +0x19, virama at
/// +0x4D, anusvara at +0x02): Devanagari, Bengali, Gujarati, Oriya, Telugu, Kannada.
const NASAL_BLOCKS: [u32; 6] = [0x0900, 0x0980, 0x0A80, 0x0B00, 0x0C00, 0x0C80];

/// The block and offset of a code point in one of [`NASAL_BLOCKS`].
fn brahmic_offset(c: char) -> Option<(u32, u32)> {
    let u = u32::from(c);
    NASAL_BLOCKS
        .iter()
        .find(|&&b| (b..b + 0x80).contains(&u))
        .map(|&b| (b, u - b))
}

/// The anusvara a written nasal conjunct folds to, if it does: `nasal` + virama, then `next`.
/// The five class nasals (ङ ञ ण न म) before a stop of their own class are the anusvara
/// written out (हिन्दी = हिंदी); the velar ङ is also written for the anusvara before any
/// consonant (मङ्सिर = मंसिर, हङ्सपुर = हंसपुर); and a nasal with a virama at the end of a
/// word is a half-typed conjunct, so a typed prefix meets the folded name. Other clusters stay
/// (न्य in न्यूयोर्क, कन्या). One rule per script, no country.
fn nasal_conjunct(nasal: char, virama: char, next: Option<char>) -> Option<char> {
    let (block, n) = brahmic_offset(nasal)?;
    if !matches!(n, 0x19 | 0x1E | 0x23 | 0x28 | 0x2E)
        || brahmic_offset(virama) != Some((block, 0x4D))
    {
        return None;
    }
    let anusvara = char::from_u32(block + 0x02)?;
    let Some(next) = next else {
        return Some(anusvara);
    };
    match brahmic_offset(next) {
        // a stop of the nasal's own class (the four before it)
        Some((b, k)) if b == block && (n - 4..n).contains(&k) => Some(anusvara),
        // the velar nasal before any consonant
        Some((b, k)) if b == block && n == 0x19 && (0x15..=0x39).contains(&k) => Some(anusvara),
        Some((b, _)) if b == block => None,
        // not a letter of this script: the end of the word
        _ if next.is_alphanumeric() || unicode_normalization::char::is_combining_mark(next) => None,
        _ => Some(anusvara),
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
    let mut chars: Vec<char> = Vec::with_capacity(s.len());
    for c in s.nfkd() {
        if matches!(c, '\u{200C}' | '\u{200D}') {
            continue;
        }
        match brahmic_fold(c) {
            Fold::Map(r) => chars.push(r),
            Fold::Drop => {}
            Fold::Keep => chars.push(c),
        }
    }
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    let mut base = Script::Common;
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        i += 1;
        if unicode_normalization::char::is_combining_mark(c) {
            if !accents_optional(base) && !out.is_empty() && !space {
                out.push(c);
            }
            continue;
        }
        if c.is_alphanumeric() {
            // A nasal conjunct after a letter of the word folds to the anusvara (D-220).
            if !out.is_empty()
                && !space
                && let Some(a) = chars
                    .get(i)
                    .and_then(|&v| nasal_conjunct(c, v, chars.get(i + 1).copied()))
            {
                out.push(a);
                i += 1;
                continue;
            }
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

/// The fewest code points of a substring key (D-199): the query minimum of substring search.
pub const SUBSTRING_MIN: usize = 3;

/// Calls `out` with every substring key of `folded`, a name folded with [`fold`] (D-199): for
/// each token (split on the fold's single spaces), every suffix of [`SUBSTRING_MIN`] or more
/// code points that does not start on a combining mark (`General_Category` M: a vowel sign, a
/// virama, an anusvara), except the first token's whole text: an exact prefix range already
/// finds a query at the start of the name. One rule for every script (P16). Keys may repeat
/// across names; each is a slice of `folded`, so nothing is allocated.
pub fn substring_keys<'f>(folded: &'f str, mut out: impl FnMut(&'f str)) {
    for (i, token) in folded.split(' ').enumerate() {
        // The token's code points from the end, so a suffix's length is known at its start.
        let mut chars = 0;
        for (at, c) in token.char_indices().rev() {
            chars += 1;
            if chars < SUBSTRING_MIN
                || (i == 0 && at == 0)
                || unicode_normalization::char::is_combining_mark(c)
            {
                continue;
            }
            if let Some(suffix) = token.get(at..) {
                out(suffix);
            }
        }
    }
}

/// The first script of a text that is not Common or Inherited.
#[must_use]
pub fn script_of(s: &str) -> Script {
    s.chars()
        .map(|c| c.script())
        .find(|x| !matches!(x, Script::Common | Script::Inherited))
        .unwrap_or(Script::Common)
}

/// `s` with the optional accents of Latin, Greek and Cyrillic removed (NFD, the combining
/// marks dropped, NFC), case and everything else kept: a display form without diacritics.
///
/// ```
/// use teigeodata_names::strip_accents;
///
/// assert_eq!(strip_accents("Ghā̃sā"), "Ghasa");
/// assert_eq!(strip_accents("São Paulo"), "Sao Paulo");
/// assert_eq!(strip_accents("काठमाडौं"), "काठमाडौं");
/// ```
#[must_use]
pub fn strip_accents(s: &str) -> String {
    if s.is_ascii() {
        return s.into();
    }
    let mut out = String::with_capacity(s.len());
    let mut base = Script::Common;
    for c in s.nfd() {
        let sc = c.script();
        if sc == Script::Inherited {
            if accents_optional(base) {
                continue;
            }
        } else {
            base = sc;
        }
        out.push(c);
    }
    out.nfc().collect()
}

/// The ISO 15924 code of the one script a text is written in (`Latn`, `Deva`), ignoring
/// Common and Inherited characters; `None` when it has letters of two scripts or of none.
///
/// ```
/// use teigeodata_names::script_code;
///
/// assert_eq!(script_code("Laha Chowk 2"), Some("Latn"));
/// assert_eq!(script_code("लहाचोक"), Some("Deva"));
/// assert_eq!(script_code("Laha चोक"), None);
/// assert_eq!(script_code("(2)"), None);
/// ```
#[must_use]
pub fn script_code(s: &str) -> Option<&'static str> {
    let mut found = None;
    for c in s.chars() {
        let x = c.script();
        if matches!(x, Script::Common | Script::Inherited) {
            continue;
        }
        match found {
            None => found = Some(x),
            Some(f) if f != x => return None,
            Some(_) => {}
        }
    }
    found.map(Script::short_name)
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
        let mut buf = KeyBuf::default();
        let mut out = String::new();
        self.key_into(&fold(s), &mut buf, &mut out);
        out
    }

    /// The key of `folded`, a name already folded ([`fold`]), into `out` (cleared first).
    /// Reuses `buf`, so a warm call allocates nothing; the same key as [`PhoneticKey::key`].
    pub fn key_into(self, folded: &str, buf: &mut KeyBuf, out: &mut String) {
        indic(folded, matches!(self, Self::IndicV1), buf, out);
    }
}

/// Reusable buffers of [`PhoneticKey::key_into`].
#[derive(Debug, Default)]
pub struct KeyBuf {
    classes: Vec<char>,
    run: String,
    word: Vec<char>,
    nasals: Vec<char>,
    kept: Vec<char>,
    letters: Vec<char>,
}

fn indic(folded: &str, drop_final_vowel: bool, buf: &mut KeyBuf, key: &mut String) {
    key.clear();
    let KeyBuf {
        classes,
        run,
        word: w,
        nasals,
        kept: out,
        letters,
    } = buf;
    classes.clear();
    for word in folded.split(' ') {
        // A word may mix scripts ("Anantaपुर"): classify each script run with its own rules.
        let is_deva = |c: char| ('\u{0900}'..='\u{097F}').contains(&c);
        w.clear();
        run.clear();
        let mut run_deva = false;
        for c in word.chars().chain(core::iter::once('\0')) {
            if c == '\0' || (!run.is_empty() && is_deva(c) != run_deva) {
                if run_deva {
                    deva_classes(run, letters, w);
                } else {
                    latin_classes(run, letters, w);
                }
                run.clear();
            }
            if c != '\0' {
                run_deva = is_deva(c);
                run.push(c);
            }
        }
        resolve_nasals(w, nasals);
        // Drop every A that is not word-initial.
        out.clear();
        for (i, c) in nasals.iter().enumerate() {
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
        classes.extend_from_slice(out);
    }
    classes.dedup();
    if classes.is_empty() {
        key.push_str(folded);
    } else {
        key.extend(classes.iter());
    }
}

fn is_vowel_class(c: char) -> bool {
    matches!(c, 'A' | 'I' | 'U')
}

/// `Ŋ` (a written velar nasal) stays as N unless a consonant follows. Other nasals (N, M) before a consonant class
/// are dropped; a word-final nasal after a vowel is dropped; otherwise M → N except before B or
/// P.
fn resolve_nasals(w: &[char], out: &mut Vec<char>) {
    out.clear();
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
}

/// Appends the classes of a Latin run to `all`; `b` is a scratch buffer.
fn latin_classes(word: &str, b: &mut Vec<char>, all: &mut Vec<char>) {
    b.clear();
    b.extend(word.chars().filter(char::is_ascii_lowercase));
    let b = &*b;
    let n = b.len();
    let at = |i: usize| b.get(i).copied();
    let is_v = |c: Option<char>| matches!(c, Some('a' | 'e' | 'i' | 'o' | 'u'));
    let starts = |i: usize, p: &str| p.chars().enumerate().all(|(k, c)| at(i + k) == Some(c));
    // The run's own classes start here: a letter looks back only within its run.
    let base = all.len();
    let mut i = 0;
    while i < n {
        let last = all.get(base..).and_then(<[char]>::last);
        let prev_vowel = last.is_some_and(|c| is_vowel_class(*c));
        let prev_consonant = last.is_some_and(|c| !is_vowel_class(*c));
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
            all.push(c);
        }
        i += len;
    }
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

/// Appends the classes of a Devanagari run to `out`; `chars` is a scratch buffer.
fn deva_classes(word: &str, chars: &mut Vec<char>, out: &mut Vec<char>) {
    chars.clear();
    chars.extend(word.chars());
    let chars = &*chars;
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        // क्ष → C, ज्ञ → G
        if (c == 'क' || c == 'ज')
            && chars.get(i + 1) == Some(&'\u{094D}')
            && matches!(chars.get(i + 2), Some('ष' | 'ञ'))
        {
            out.push(if c == 'क' { 'C' } else { 'G' });
            i += 3;
            out.extend(inherent(chars, i));
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
            out.extend(inherent(chars, i));
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
        // A written nasal conjunct is the anusvara (D-220): before a stop of its own class,
        // the velar before any consonant, and half-typed at a word's end. Other clusters and
        // a nasal starting a word stay.
        assert_eq!(fold("हिन्दी"), fold("हिंदी"));
        assert_eq!(fold("मङ्सिर"), fold("मंसिर"));
        assert_eq!(fold("हङ्सपुर"), "हंसपुर");
        assert_eq!(fold("सम्पत"), "संपत");
        assert_eq!(fold("हङ्"), "हं");
        assert_eq!(fold("हिन् x"), "हिं x");
        assert_eq!(fold("न्यूयोर्क"), "न्युयोर्क");
        assert_eq!(fold("कन्या"), "कन्या");
        assert_eq!(fold("सम्राट"), "सम्राट");
        assert_eq!(fold("বন্দর"), "বংদর");
        // Vowel length and the candra vowels fold.
        assert_eq!(fold("न्यूयोर्क"), fold("न्युयोर्क"));
        assert_eq!(fold("न्यूयॉर्क"), fold("न्युयोर्क"));
        assert_eq!(fold("दीपावली"), "दिपावलि");
        assert_eq!(fold("ऊर्जा ईलाम ऑफ ऍप"), "उर्जा इलाम ओफ एप");
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

#[cfg(test)]
mod substring_key_tests {
    use super::{SUBSTRING_MIN, fold, substring_keys};
    use alloc::string::String;
    use alloc::vec::Vec;

    fn keys(name: &str) -> Vec<String> {
        let f = fold(name);
        let mut out = Vec::new();
        substring_keys(&f, |k| out.push(String::from(k)));
        out.sort();
        out
    }

    #[test]
    fn suffixes_of_three_code_points_not_starting_on_a_mark() {
        assert_eq!(
            keys("Kathmandu"),
            [
                "andu", "athmandu", "du", "hmandu", "mandu", "ndu", "thmandu"
            ]
            .into_iter()
            .filter(|k| k.chars().count() >= SUBSTRING_MIN)
            .collect::<Vec<_>>()
        );
        // The second word's whole text is a key; the first's is not.
        let k = keys("Pokhara Lekhnath");
        assert!(k.iter().any(|x| x == "lekhnath") && k.iter().any(|x| x == "khara"));
        assert!(!k.iter().any(|x| x == "pokhara" || x == "pokhara lekhnath"));
        // काठमाडौं is क ा ठ म ा ड ौ ं: starts on the marks ा ौ ं are skipped.
        let k = keys("काठमाडौं");
        assert!(k.iter().any(|x| x == "माडौं") && k.iter().any(|x| x == "ठमाडौं"));
        assert!(
            !k.iter()
                .any(|x| x.starts_with('\u{093e}') || x.starts_with('\u{094c}'))
        );
        assert!(!k.iter().any(|x| x == "काठमाडौं"));
        // A start after a virama is kept: the rule is about marks, not clusters.
        assert!(keys("Xक्षत्र").iter().any(|x| x == "षत्र"));
        // Too short for any key.
        assert_eq!(keys("ab cd"), Vec::<String>::new());
    }

    /// The tiers together find what a scan finds (the note's gate 5): for a query of 3 or more
    /// code points without a space, not starting on a mark, a name holds it exactly when its
    /// fold starts with it (the exact prefix range) or a substring key does.
    #[test]
    fn exact_prefix_and_substring_keys_agree_with_a_scan() {
        const PARTS: [&str; 10] = [
            "ka", "th", "\u{e9}", "man", "du", " ", "\u{915}", "\u{93e}", "\u{920}", "\u{94d}",
        ];
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = |n: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            usize::try_from(state % n as u64).unwrap_or(0)
        };
        let names: Vec<String> = (0..300)
            .map(|_| {
                let parts = 1 + next(6);
                (0..parts)
                    .map(|_| PARTS[next(PARTS.len())])
                    .collect::<String>()
            })
            .map(|n| fold(&n))
            .filter(|f| !f.is_empty())
            .collect();
        let mut found: Vec<Vec<&str>> = Vec::new();
        for f in &names {
            let mut k = Vec::new();
            substring_keys(f, |s| k.push(s));
            found.push(k);
        }
        let mut queries = 0;
        for f in &names {
            let chars: Vec<(usize, char)> = f.char_indices().collect();
            for (a, &(at, c)) in chars.iter().enumerate() {
                if unicode_normalization::char::is_combining_mark(c) {
                    continue;
                }
                for len in SUBSTRING_MIN..=SUBSTRING_MIN + 2 {
                    let end = chars.get(a + len).map_or(f.len(), |e| e.0);
                    if a + len > chars.len() {
                        break;
                    }
                    let q = &f[at..end];
                    if q.contains(' ') {
                        continue;
                    }
                    queries += 1;
                    for (n, keys) in names.iter().zip(&found) {
                        let tiers = n.starts_with(q) || keys.iter().any(|k| k.starts_with(q));
                        assert_eq!(tiers, n.contains(q), "{q:?} in {n:?}");
                    }
                }
            }
            if queries > 400 {
                break;
            }
        }
        assert!(queries > 100, "{queries}");
    }
}

#[cfg(test)]
mod key_buf_tests {
    use super::{KeyBuf, PhoneticKey, fold};
    use alloc::string::String;

    /// One buffer reused across names of both scripts, mixed runs and empty keys gives the key
    /// of a fresh call every time: nothing leaks from one name into the next.
    #[test]
    fn a_reused_buffer_gives_the_fresh_key() {
        let names = [
            "Kathmandu",
            "काठमाडौं",
            "Anantaपुर",
            "Bhimsenगाउँ",
            "",
            "123",
            "Laxmipur Kodraha",
            "ङ",
            "Chhimkeshwori",
            "क्षेत्रपाटी",
            "Gyanodaya",
            "Ama",
            "a",
            "Mahendranagar Bazaar",
            "!!",
        ];
        for key in [PhoneticKey::IndicV0, PhoneticKey::IndicV1] {
            let mut buf = KeyBuf::default();
            let mut out = String::from("stale");
            for round in 0..2 {
                for n in names.iter().chain(names.iter().rev()) {
                    key.key_into(&fold(n), &mut buf, &mut out);
                    assert_eq!(out, key.key(n), "{key:?} {n:?} round {round}");
                }
            }
        }
    }
}
