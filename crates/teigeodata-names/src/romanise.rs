//! Readable romanisation of abugida names (Devanagari and its kin) into plain ASCII Latin
//! ([research note](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/names-canonical.md)).
//!
//! The engine names no language: a [`Profile`] holds a script's letters and a language's schwa
//! habits, from a data file (`data/romanise/<lang>-<Script>.yaml`, schema
//! `teigeodata/romanise/v1`), and the morphemes come from a country's lexicon. Per word:
//!
//! 1. A word that is a morpheme, or ends in one and leaves at least two characters, writes the
//!    morpheme in its canonical form and romanises the stem alone.
//! 2. The word becomes units: a consonant with its vowel (the inherent vowel, a vowel sign, or
//!    none after a virama), an independent vowel, a nasal sign, or anything else.
//! 3. The word-final inherent vowel is deleted unless a conjunct comes before it. Then, left
//!    to right, an inherent vowel with a vowel on its left and a consonant with a vowel on its
//!    right is deleted, unless that consonant is in `keep_before` (Ohala's `VC_CV` rule).
//! 4. Letters are written from the tables. A nasal sign before a consonant is n, or m before
//!    a labial; at the end of a word after a letter it is n; elsewhere an anusvara is n and a
//!    chandrabindu is dropped. A consonant
//!    in `assimilate` changes before the listed ones; a doubled consonant in `geminate_once`
//!    is written once.
//! 5. Each word is title-cased. The output is ASCII: anything the tables do not cover and is
//!    not ASCII is dropped.
//!
//! ```
//! use teigeodata_names::romanise::{Profile, ProfileData};
//!
//! let mut data = ProfileData::default();
//! data.inherent = "a".into();
//! data.virama = Some('्');
//! data.consonants = vec![("क".into(), "k".into()), ("ल".into(), "l".into()),
//!     ("प".into(), "p".into()), ("र".into(), "r".into()), ("त".into(), "t".into())];
//! data.signs = vec![("ि".into(), "i".into()), ("ु".into(), "u".into())];
//! let p = Profile::new(&data, &[]).unwrap();
//! assert_eq!(p.romanise("ललितपुर"), "Lalitpur");
//! ```

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use unicode_normalization::UnicodeNormalization;

/// A profile as written in its data file: every table as (letters, Latin) pairs.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct ProfileData {
    /// The inherent vowel's Latin form (`a`).
    pub inherent: String,
    /// The sign that removes the inherent vowel (Devanagari ् U+094D).
    pub virama: Option<char>,
    /// The sign that marks a borrowed sound (Devanagari ़ U+093C), part of consonant keys.
    pub nukta: Option<char>,
    /// The nasal sign written n, or m before a labial, wherever it stands (Devanagari ं).
    pub anusvara: Option<char>,
    /// The nasal sign written like the anusvara before a consonant and at the end of a word,
    /// and dropped before a vowel (ँ).
    pub chandrabindu: Option<char>,
    /// Consonants: one letter, or a letter and the nukta.
    pub consonants: Vec<(String, String)>,
    /// Independent vowels.
    #[cfg_attr(feature = "serde", serde(default))]
    pub vowels: Vec<(String, String)>,
    /// Vowel signs.
    pub signs: Vec<(String, String)>,
    /// Conjuncts written as a whole: consonant, virama, consonant (ज्ञ → gy).
    #[cfg_attr(feature = "serde", serde(default))]
    pub conjuncts: Vec<(String, String)>,
    /// Consonants before which an anusvara is written m.
    #[cfg_attr(feature = "serde", serde(default))]
    pub labials: Vec<String>,
    /// Consonants written once when doubled through a virama (aspirates: ख्ख → kh).
    #[cfg_attr(feature = "serde", serde(default))]
    pub geminate_once: Vec<String>,
    /// Consonants before which no medial inherent vowel is deleted (Nepali ह).
    #[cfg_attr(feature = "serde", serde(default))]
    pub keep_before: Vec<String>,
    /// A consonant with a virama written otherwise before some consonants (ङ before a velar).
    #[cfg_attr(feature = "serde", serde(default))]
    pub assimilate: Vec<Assimilate>,
    /// Misspellings repaired before anything else (Devanagari ा + े, which renders like ो).
    #[cfg_attr(feature = "serde", serde(default))]
    pub repairs: Vec<(String, String)>,
    /// The script's digit zero; it and the nine after it are written 0 to 9.
    #[cfg_attr(feature = "serde", serde(default))]
    pub zero: Option<char>,
    /// Examples that must romanise as given (no morphemes), checked with the profile.
    #[cfg_attr(feature = "serde", serde(default))]
    pub examples: Vec<(String, String)>,
}

/// A profile's data file, `data/romanise/<lang>-<Script>.yaml`.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct ProfileFile {
    /// `teigeodata/romanise/v1`.
    pub schema: String,
    /// The language subtag (`ne`).
    pub language: String,
    /// The ISO 15924 script code (`Deva`).
    pub script: String,
    /// The tables.
    pub profile: ProfileData,
}

impl ProfileFile {
    /// The schema this module reads.
    pub const SCHEMA: &'static str = "teigeodata/romanise/v1";

    /// The file's ID, `<language>-<script>`, which is its file name.
    #[must_use]
    pub fn id(&self) -> String {
        alloc::format!("{}-{}", self.language, self.script)
    }

    /// Checks the schema and that the file is named after its ID, then compiles the profile
    /// with `morphemes` (which runs its examples).
    ///
    /// # Errors
    /// A wrong schema or name, or any error of [`Profile::new`].
    pub fn compile(&self, name: &str, morphemes: &[(String, String)]) -> Result<Profile, String> {
        if self.schema != Self::SCHEMA {
            return Err(alloc::format!(
                "schema {:?}, expected {}",
                self.schema,
                Self::SCHEMA
            ));
        }
        if self.id() != name {
            return Err(alloc::format!(
                "the file for {} must be named {}.yaml",
                self.id(),
                self.id()
            ));
        }
        Profile::new(&self.profile, morphemes)
    }
}

/// A consonant written `latin` when a virama joins it to one of `before`.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Assimilate {
    /// The consonant.
    pub consonant: String,
    /// The consonants that follow it.
    pub before: Vec<String>,
    /// Its Latin form there.
    pub latin: String,
}

/// A compiled profile with its morphemes, ready to romanise.
#[derive(Clone, Debug)]
pub struct Profile {
    inherent: String,
    virama: Option<char>,
    zero: Option<char>,
    repairs: Vec<(String, String)>,
    anusvara: Option<char>,
    chandrabindu: Option<char>,
    /// Consonant keys (NFC) to their index; longest key first when matching.
    consonants: BTreeMap<String, u16>,
    latin: Vec<String>,
    vowels: BTreeMap<char, String>,
    signs: BTreeMap<char, String>,
    conjuncts: BTreeMap<(u16, u16), String>,
    labial: Vec<bool>,
    geminate_once: Vec<bool>,
    keep_before: Vec<bool>,
    assimilate: Vec<(u16, Vec<u16>, String)>,
    /// Morphemes (NFC) and their canonical Latin forms, longest first.
    morphemes: Vec<(String, String)>,
}

/// One unit of a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unit {
    /// A consonant and its vowel.
    C(u16, Vowel),
    /// An independent vowel.
    V(char),
    /// An anusvara (`true`) or chandrabindu (`false`).
    N(bool),
    /// Anything else.
    X(char),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Vowel {
    /// The inherent vowel.
    Inherent,
    /// No vowel: a virama, or an inherent vowel deleted.
    None,
    /// A vowel sign.
    Sign(char),
    /// A conjunct override's second consonant carries this vowel.
    Conjunct(u16, VowelOf),
}

/// The vowel of a conjunct's second consonant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VowelOf {
    Inherent,
    None,
    Sign(char),
}

fn one_char(s: &str, what: &str) -> Result<char, String> {
    let mut it = s.chars();
    match (it.next(), it.next()) {
        (Some(c), None) => Ok(c),
        _ => Err(alloc::format!("{what} {s:?} must be one character")),
    }
}

fn nfc(s: &str) -> String {
    s.nfc().collect()
}

fn ascii(s: &str, what: &str) -> Result<(), String> {
    if s.is_ascii() {
        Ok(())
    } else {
        Err(alloc::format!("{what} {s:?} is not ASCII"))
    }
}

/// Consonant keys (NFC) to their index, and each index's Latin form.
type Consonants = (BTreeMap<String, u16>, Vec<String>);

fn consonant_table(data: &ProfileData) -> Result<Consonants, String> {
    let mut keys = BTreeMap::new();
    let mut latin = Vec::new();
    for (letters, written) in &data.consonants {
        ascii(written, "consonant")?;
        let key = nfc(letters);
        let count = key.chars().count();
        let with_nukta = count == 2 && data.nukta.is_some_and(|x| key.ends_with(x));
        if count != 1 && !with_nukta {
            return Err(alloc::format!(
                "consonant {letters:?} must be a letter, or a letter and the nukta"
            ));
        }
        let id = u16::try_from(latin.len()).map_err(|_| "too many consonants".to_string())?;
        if keys.insert(key, id).is_some() {
            return Err(alloc::format!("consonant {letters:?} twice"));
        }
        latin.push(written.clone());
    }
    Ok((keys, latin))
}

fn char_table(rows: &[(String, String)], what: &str) -> Result<BTreeMap<char, String>, String> {
    let mut table = BTreeMap::new();
    for (letter, written) in rows {
        ascii(written, what)?;
        let c = one_char(&nfc(letter), what)?;
        if table.insert(c, written.clone()).is_some() {
            return Err(alloc::format!("{what} {letter:?} twice"));
        }
    }
    Ok(table)
}

fn id_of(keys: &BTreeMap<String, u16>, s: &str, what: &str) -> Result<u16, String> {
    keys.get(&nfc(s))
        .copied()
        .ok_or_else(|| alloc::format!("{what} {s:?} is not a consonant of the profile"))
}

fn flags(
    keys: &BTreeMap<String, u16>,
    len: usize,
    rows: &[String],
    what: &str,
) -> Result<Vec<bool>, String> {
    let mut on = alloc::vec![false; len];
    for row in rows {
        if let Some(slot) = on.get_mut(usize::from(id_of(keys, row, what)?)) {
            *slot = true;
        }
    }
    Ok(on)
}

fn conjunct_table(
    data: &ProfileData,
    keys: &BTreeMap<String, u16>,
) -> Result<BTreeMap<(u16, u16), String>, String> {
    let mut table = BTreeMap::new();
    for (letters, written) in &data.conjuncts {
        ascii(written, "conjunct")?;
        let Some(virama) = data.virama else {
            return Err("conjuncts need a virama".into());
        };
        let chars: Vec<char> = nfc(letters).chars().collect();
        let split = chars
            .iter()
            .position(|&c| c == virama)
            .ok_or_else(|| alloc::format!("conjunct {letters:?} has no virama"))?;
        let first: String = chars.iter().take(split).collect();
        let second: String = chars.iter().skip(split + 1).collect();
        table.insert(
            (
                id_of(keys, &first, "conjunct")?,
                id_of(keys, &second, "conjunct")?,
            ),
            written.clone(),
        );
    }
    Ok(table)
}

/// Morphemes in NFC with lower-case Latin forms, longest first, then by text.
fn morpheme_list(morphemes: &[(String, String)]) -> Result<Vec<(String, String)>, String> {
    let mut list: Vec<(String, String)> = Vec::new();
    for (native, canon) in morphemes {
        ascii(canon, "morpheme")?;
        let key = nfc(native);
        if key.is_empty() || canon.is_empty() {
            return Err("an empty morpheme".into());
        }
        list.push((key, canon.to_ascii_lowercase()));
    }
    list.sort_by(|a, b| {
        b.0.chars()
            .count()
            .cmp(&a.0.chars().count())
            .then_with(|| a.cmp(b))
    });
    list.dedup_by(|a, b| a.0 == b.0);
    Ok(list)
}

impl Profile {
    /// Compiles `data` with the country's `morphemes` (native form, canonical Latin form).
    ///
    /// # Errors
    /// A table entry that is empty, not one consonant, or names a consonant the table lacks;
    /// a Latin form that is not ASCII; an example that romanises otherwise.
    pub fn new(data: &ProfileData, morphemes: &[(String, String)]) -> Result<Self, String> {
        ascii(&data.inherent, "inherent")?;
        let (consonants, latin) = consonant_table(data)?;
        let count = latin.len();
        let mut assimilate = Vec::new();
        for rule in &data.assimilate {
            ascii(&rule.latin, "assimilate")?;
            let before = rule
                .before
                .iter()
                .map(|b| id_of(&consonants, b, "assimilate"))
                .collect::<Result<Vec<u16>, String>>()?;
            assimilate.push((
                id_of(&consonants, &rule.consonant, "assimilate")?,
                before,
                rule.latin.clone(),
            ));
        }
        let mut repairs = Vec::new();
        for (from, to) in &data.repairs {
            if from.is_empty() {
                return Err("an empty repair".into());
            }
            repairs.push((nfc(from), nfc(to)));
        }
        let profile = Self {
            inherent: data.inherent.clone(),
            virama: data.virama,
            zero: data.zero,
            repairs,
            anusvara: data.anusvara,
            chandrabindu: data.chandrabindu,
            vowels: char_table(&data.vowels, "vowel")?,
            signs: char_table(&data.signs, "vowel sign")?,
            conjuncts: conjunct_table(data, &consonants)?,
            labial: flags(&consonants, count, &data.labials, "labial")?,
            geminate_once: flags(&consonants, count, &data.geminate_once, "geminate")?,
            keep_before: flags(&consonants, count, &data.keep_before, "keep_before")?,
            assimilate,
            morphemes: morpheme_list(morphemes)?,
            consonants,
            latin,
        };
        for (input, want) in &data.examples {
            let got = profile.without_morphemes().romanise(input);
            if &got != want {
                return Err(alloc::format!(
                    "example {input:?}: {got:?}, expected {want:?}"
                ));
            }
        }
        Ok(profile)
    }

    fn without_morphemes(&self) -> Self {
        Self {
            morphemes: Vec::new(),
            ..self.clone()
        }
    }

    /// `name` in readable ASCII Latin, each word title-cased. Words are separated by one space.
    #[must_use]
    pub fn romanise(&self, name: &str) -> String {
        let mut text = nfc(name);
        for (from, to) in &self.repairs {
            if text.contains(from.as_str()) {
                text = nfc(&text.replace(from.as_str(), to));
            }
        }
        let mut out = String::new();
        for w in text.split_whitespace() {
            let mut word = String::new();
            self.word(w, &mut word);
            if word.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push(' ');
            }
            let mut first = true;
            for c in word.chars() {
                if first && c.is_ascii_alphabetic() {
                    out.push(c.to_ascii_uppercase());
                    first = false;
                } else {
                    out.push(c);
                }
            }
        }
        out
    }

    /// Romanises one NFC word into `out`, in lower case. A stem left by a morpheme is
    /// romanised the same way, so compounds of several morphemes split fully.
    fn word(&self, w: &str, out: &mut String) {
        for (native, canon) in &self.morphemes {
            if w == native {
                out.push_str(canon);
                return;
            }
        }
        for (native, canon) in &self.morphemes {
            if let Some(stem) = w.strip_suffix(native.as_str())
                && stem.chars().count() >= 2
            {
                self.word(stem, out);
                out.push_str(canon);
                return;
            }
        }
        self.letters(w, out);
    }

    fn consonant_at(&self, s: &str) -> Option<(u16, usize)> {
        let mut it = s.char_indices();
        let (_, c0) = it.next()?;
        let one = c0.len_utf8();
        if let Some((_, c1)) = it.next() {
            let two = one + c1.len_utf8();
            if let Some(&id) = s.get(..two).and_then(|k| self.consonants.get(k)) {
                return Some((id, two));
            }
        }
        s.get(..one)
            .and_then(|k| self.consonants.get(k))
            .map(|&id| (id, one))
    }

    fn units(&self, w: &str) -> Vec<Unit> {
        let mut units = Vec::new();
        let mut rest = w;
        while let Some(c) = rest.chars().next() {
            if let Some((id, len)) = self.consonant_at(rest) {
                rest = rest.get(len..).unwrap_or("");
                let vowel = match rest.chars().next() {
                    Some(v) if Some(v) == self.virama => {
                        rest = rest.get(v.len_utf8()..).unwrap_or("");
                        Vowel::None
                    }
                    Some(v) if self.signs.contains_key(&v) => {
                        rest = rest.get(v.len_utf8()..).unwrap_or("");
                        Vowel::Sign(v)
                    }
                    _ => Vowel::Inherent,
                };
                units.push(Unit::C(id, vowel));
                continue;
            }
            rest = rest.get(c.len_utf8()..).unwrap_or("");
            units.push(if self.vowels.contains_key(&c) {
                Unit::V(c)
            } else if Some(c) == self.anusvara {
                Unit::N(true)
            } else if Some(c) == self.chandrabindu {
                Unit::N(false)
            } else {
                Unit::X(c)
            });
        }
        // Conjunct overrides: C(a, none) C(b, v) → one unit.
        let mut merged: Vec<Unit> = Vec::with_capacity(units.len());
        let mut i = 0;
        while let Some(&u) = units.get(i) {
            if let (Unit::C(a, Vowel::None), Some(&Unit::C(b, v))) = (u, units.get(i + 1))
                && self.conjuncts.contains_key(&(a, b))
                && !matches!(v, Vowel::Conjunct(..))
            {
                let of = match v {
                    Vowel::None => VowelOf::None,
                    Vowel::Sign(s) => VowelOf::Sign(s),
                    _ => VowelOf::Inherent,
                };
                merged.push(Unit::C(a, Vowel::Conjunct(b, of)));
                i += 2;
                continue;
            }
            merged.push(u);
            i += 1;
        }
        merged
    }

    fn letters(&self, word: &str, out: &mut String) {
        let mut units = self.units(word);
        self.delete_schwa(&mut units);
        for (k, unit) in units.iter().enumerate() {
            let prev = k.checked_sub(1).and_then(|p| units.get(p)).copied();
            self.write(*unit, prev, units.get(k + 1).copied(), out);
        }
    }

    fn is(table: &[bool], id: u16) -> bool {
        table.get(usize::from(id)).copied().unwrap_or(false)
    }

    /// Deletes the inherent vowels that are not pronounced.
    fn delete_schwa(&self, units: &mut [Unit]) {
        let count = units.len();
        // Word-final, unless it ends a conjunct (a written-whole conjunct included).
        if count >= 2
            && let Some(&Unit::C(id, Vowel::Inherent)) = units.last()
            && !matches!(units.get(count - 2), Some(Unit::C(_, Vowel::None)))
            && let Some(last) = units.last_mut()
        {
            *last = Unit::C(id, Vowel::None);
        }
        // Medial, left to right: V C _ C V.
        for k in 1..count.saturating_sub(1) {
            let Some(&Unit::C(id, Vowel::Inherent)) = units.get(k) else {
                continue;
            };
            let left = match units.get(k - 1) {
                Some(Unit::V(_)) => true,
                Some(Unit::C(_, vowel)) => has_vowel(*vowel),
                _ => false,
            };
            // A conjunct on the right starts with a virama, so it has no vowel there.
            let right = match units.get(k + 1) {
                Some(Unit::C(next, vowel)) => {
                    matches!(vowel, Vowel::Inherent | Vowel::Sign(_))
                        && !Self::is(&self.keep_before, *next)
                }
                _ => false,
            };
            if left
                && right
                && let Some(slot) = units.get_mut(k)
            {
                *slot = Unit::C(id, Vowel::None);
            }
        }
    }

    fn sign(&self, sign: char) -> &str {
        self.signs.get(&sign).map_or("", String::as_str)
    }

    /// Writes one unit, given its neighbours.
    fn write(&self, unit: Unit, prev: Option<Unit>, next: Option<Unit>, out: &mut String) {
        match unit {
            Unit::C(id, Vowel::Conjunct(second, vowel)) => {
                if let Some(written) = self.conjuncts.get(&(id, second)) {
                    out.push_str(written);
                }
                match vowel {
                    VowelOf::Inherent => out.push_str(&self.inherent),
                    VowelOf::Sign(sign) => out.push_str(self.sign(sign)),
                    VowelOf::None => {}
                }
            }
            Unit::C(id, vowel) => {
                let next_id = match next {
                    Some(Unit::C(b, _)) => Some(b),
                    _ => None,
                };
                if vowel == Vowel::None && next_id == Some(id) && Self::is(&self.geminate_once, id)
                {
                    return;
                }
                let mut written = self.latin.get(usize::from(id)).map_or("", String::as_str);
                if vowel == Vowel::None
                    && let Some(b) = next_id
                {
                    for (c, before, latin) in &self.assimilate {
                        if *c == id && before.contains(&b) {
                            written = latin;
                        }
                    }
                }
                out.push_str(written);
                match vowel {
                    Vowel::Inherent => out.push_str(&self.inherent),
                    Vowel::Sign(sign) => out.push_str(self.sign(sign)),
                    Vowel::None | Vowel::Conjunct(..) => {}
                }
            }
            Unit::V(c) => out.push_str(self.vowels.get(&c).map_or("", String::as_str)),
            Unit::N(anusvara) => match next {
                Some(Unit::C(b, _)) => out.push(if Self::is(&self.labial, b) { 'm' } else { 'n' }),
                _ if anusvara => out.push('n'),
                None if matches!(prev, Some(Unit::C(..) | Unit::V(_))) => out.push('n'),
                _ => {}
            },
            Unit::X(c) => {
                let digit = self
                    .zero
                    .and_then(|z| u32::from(c).checked_sub(u32::from(z)))
                    .filter(|&d| d < 10)
                    .and_then(|d| char::from_digit(d, 10));
                if let Some(d) = digit {
                    out.push(d);
                } else if c.is_ascii() {
                    out.push(c.to_ascii_lowercase());
                }
            }
        }
    }
}

const fn has_vowel(v: Vowel) -> bool {
    !matches!(v, Vowel::None | Vowel::Conjunct(_, VowelOf::None))
}
