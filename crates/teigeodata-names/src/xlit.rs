//! Transliteration of Latin names into an abugida (Devanagari and its kin)
//! ([research note](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/nepali-names.md),
//! D-215, D-222).
//!
//! The engine names no language. A [`Profile`] holds the Latin units and the target letters
//! each may stand for, with costs, from a data file (`data/xlit/<lang>-Latn-<Script>.yaml`,
//! schema `teigeodata/xlit/v1`), plus a country lexicon's generic terms. A [`Model`] is a
//! character n-gram language model of the target script with a word memory, both trained by
//! the forge from names of one licence class ([`ModelBuilder`]). Per word:
//!
//! 1. A phrase or word of the profile's `words` table (a designator, an irregular exonym), a
//!    lexicon term, or a word of the model's memory is written as stored.
//! 2. A word that ends in a generic lexicon term (at least three letters, leaving a stem of
//!    at least three) is the stem's transliteration followed by the term.
//! 3. Otherwise a beam search runs over the lattice of the word's Latin units. A consonant
//!    after a bare consonant either keeps the inherent vowel between them (cost `schwa`) or
//!    joins it with the virama (cost `conjunct`); a vowel is a sign after a consonant and an
//!    independent letter elsewhere; a nasal sign follows a vowel. Each hypothesis costs its
//!    options plus `lm_weight` times the model's negative log probability of every letter it
//!    writes, and the word's end. Hypotheses that the model cannot tell apart recombine.
//!
//! Output is deterministic and holds only the target script and single spaces.
//!
//! ```
//! use teigeodata_names::xlit::{Entry, ModelBuilder, Profile, ProfileData, VowelEntry, VowelOption};
//!
//! let mut data = ProfileData::default();
//! data.virama = '्';
//! data.consonants = vec![
//!     Entry { latin: "k".into(), options: vec![("क".into(), 0.0)] },
//!     Entry { latin: "l".into(), options: vec![("ल".into(), 0.0)] },
//! ];
//! data.vowels = vec![VowelEntry { latin: "a".into(), options: vec![
//!     VowelOption { independent: "अ".into(), sign: String::new(), cost: 0.0, end: None },
//!     VowelOption { independent: "आ".into(), sign: "ा".into(), cost: 1.0, end: None },
//! ] }];
//! let profile = Profile::new(&data, &[]).unwrap();
//! let mut lm = ModelBuilder::new(&profile, 3).unwrap();
//! lm.add_text("काल कला");
//! let model = lm.build(&profile);
//! let mut out = String::new();
//! teigeodata_names::xlit::transliterate(&profile, &model, "Kal", &mut out);
//! assert_eq!(out, "कल");
//! ```

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use unicode_normalization::UnicodeNormalization;

/// One Latin unit and the target letters it may stand for, each with a cost.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Entry {
    /// Lower-case ASCII letters (`th`).
    pub latin: String,
    /// (target text, cost); a consonant's text is a consonant or a conjunct (थ, क्ष).
    pub options: Vec<(String, f32)>,
}

/// One way to write a Latin vowel.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct VowelOption {
    /// The independent letter, written at the start of a word or after a vowel (आ).
    pub independent: String,
    /// The sign written after a consonant (ा); empty for the inherent vowel.
    #[cfg_attr(feature = "serde", serde(default))]
    pub sign: String,
    /// The cost.
    #[cfg_attr(feature = "serde", serde(default))]
    pub cost: f32,
    /// The cost when the vowel ends the word, if it differs (a final inherent vowel is
    /// silent, so a written final vowel is usually long).
    #[cfg_attr(feature = "serde", serde(default))]
    pub end: Option<f32>,
}

/// A Latin vowel unit.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct VowelEntry {
    /// Lower-case ASCII letters (`aa`).
    pub latin: String,
    /// The ways to write it.
    pub options: Vec<VowelOption>,
}

/// The costs that are not tied to one unit.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Costs {
    /// A consonant after a bare consonant, keeping the unwritten inherent vowel between them.
    pub schwa: f32,
    /// A consonant joined to a bare consonant by the virama.
    pub conjunct: f32,
    /// A virama after the word's last consonant.
    pub final_virama: f32,
    /// A Latin character no unit covers, dropped.
    pub skip: f32,
    /// The weight of the language model's cost.
    pub lm_weight: f32,
    /// Hypotheses kept per Latin position.
    pub beam: u16,
}

impl Default for Costs {
    fn default() -> Self {
        Self {
            schwa: 0.0,
            conjunct: 0.0,
            final_virama: 4.0,
            skip: 8.0,
            lm_weight: 1.0,
            beam: 16,
        }
    }
}

/// A profile as written in its data file.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct ProfileData {
    /// The sign that removes the inherent vowel (Devanagari ्). Its 128-code-point block is
    /// the target script's block, which the model covers.
    pub virama: char,
    /// The script's digit zero; ASCII digits are written with it and the nine after it.
    #[cfg_attr(feature = "serde", serde(default))]
    pub zero: Option<char>,
    /// The costs that are not tied to one unit.
    #[cfg_attr(feature = "serde", serde(default))]
    pub costs: Costs,
    /// Consonant units.
    pub consonants: Vec<Entry>,
    /// Vowel units.
    pub vowels: Vec<VowelEntry>,
    /// Nasal units: signs written after a vowel (ं, ँ).
    #[cfg_attr(feature = "serde", serde(default))]
    pub nasals: Vec<Entry>,
    /// Whole words or phrases (Latin words separated by one space) written as given:
    /// designators that are translated, not transliterated, and irregular exonyms.
    #[cfg_attr(feature = "serde", serde(default))]
    pub words: Vec<(String, String)>,
    /// Examples (Latin, target) that must come out as given with a trained model; the forge
    /// checks them.
    #[cfg_attr(feature = "serde", serde(default))]
    pub examples: Vec<(String, String)>,
}

/// A profile's data file, `data/xlit/<language>-<source>-<script>.yaml`.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct ProfileFile {
    /// `teigeodata/xlit/v1`.
    pub schema: String,
    /// The target language subtag (`ne`).
    pub language: String,
    /// The ISO 15924 code of the input script (`Latn`).
    pub source: String,
    /// The ISO 15924 code of the target script (`Deva`).
    pub script: String,
    /// The tables.
    pub profile: ProfileData,
}

impl ProfileFile {
    /// The schema this module reads.
    pub const SCHEMA: &'static str = "teigeodata/xlit/v1";

    /// The file's ID, `<language>-<source>-<script>`, which is its file name.
    #[must_use]
    pub fn id(&self) -> String {
        alloc::format!("{}-{}-{}", self.language, self.source, self.script)
    }

    /// Checks the schema and that the file is named after its ID, then compiles the profile
    /// with `terms`.
    ///
    /// # Errors
    /// A wrong schema or name, or any error of [`Profile::new`].
    pub fn compile(&self, name: &str, terms: &[Term]) -> Result<Profile, String> {
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
        Profile::new(&self.profile, terms)
    }
}

/// A lexicon term: a Latin spelling and its native form in the target script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Term {
    /// A Latin spelling (canonical or variant).
    pub latin: String,
    /// The native form.
    pub native: String,
    /// A generic term, which may end a compound word (Lahachowk).
    pub suffix: bool,
}

/// The shortest term matched as a suffix, and the shortest stem it may leave.
const MIN_SUFFIX: usize = 3;
const MIN_STEM: usize = 3;
/// The longest n-gram a model holds (eight symbols packed in a `u64`).
pub const MAX_ORDER: usize = 8;

#[derive(Clone, Debug)]
enum Choice {
    Consonant {
        text: String,
        cost: f32,
    },
    Vowel {
        independent: String,
        sign: String,
        cost: f32,
        end: f32,
    },
    Nasal {
        text: String,
        cost: f32,
    },
}

/// A compiled profile with its lexicon terms.
#[derive(Clone, Debug)]
pub struct Profile {
    virama: char,
    block: u32,
    zero: Option<char>,
    costs: Costs,
    /// Latin unit → its choices.
    units: BTreeMap<String, Vec<Choice>>,
    longest: usize,
    /// Phrases (folded Latin words) → target text, most words first.
    phrases: Vec<(Vec<String>, String)>,
    /// Whole lexicon words → native form.
    terms: BTreeMap<String, String>,
    /// Generic terms that may end a word, longest first.
    suffixes: Vec<(String, String)>,
    examples: Vec<(String, String)>,
}

fn nfc(s: &str) -> String {
    s.nfc().collect()
}

fn check_latin(s: &str, what: &str) -> Result<(), String> {
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase()) {
        Ok(())
    } else {
        Err(alloc::format!(
            "{what} {s:?} must be lower-case ASCII letters"
        ))
    }
}

fn check_cost(c: f32, what: &str) -> Result<(), String> {
    if c.is_finite() && c >= 0.0 {
        Ok(())
    } else {
        Err(alloc::format!(
            "{what}: cost {c} must be finite and not negative"
        ))
    }
}

/// Whether every character of `s` is in the 128-code-point `block`.
fn in_block(block: u32, s: &str) -> bool {
    s.chars().all(|c| u32::from(c) & !0x7F == block)
}

/// `s` in NFC, checked to be letters of the target block (and not empty unless `empty`).
fn target(block: u32, s: &str, what: &str, empty: bool) -> Result<String, String> {
    let t = nfc(s);
    if (t.is_empty() && !empty) || !in_block(block, &t) {
        return Err(alloc::format!(
            "{what} {s:?} must be letters of the target block"
        ));
    }
    Ok(t)
}

/// Every Latin unit's choices: consonants, vowels and nasals.
fn unit_table(data: &ProfileData, block: u32) -> Result<BTreeMap<String, Vec<Choice>>, String> {
    let mut units: BTreeMap<String, Vec<Choice>> = BTreeMap::new();
    for e in &data.consonants {
        check_latin(&e.latin, "consonant")?;
        if e.options.is_empty() {
            return Err(alloc::format!("consonant {:?} has no options", e.latin));
        }
        for (text, cost) in &e.options {
            check_cost(*cost, &e.latin)?;
            let text = target(block, text, "consonant", false)?;
            units
                .entry(e.latin.clone())
                .or_default()
                .push(Choice::Consonant { text, cost: *cost });
        }
    }
    for e in &data.vowels {
        check_latin(&e.latin, "vowel")?;
        if e.options.is_empty() {
            return Err(alloc::format!("vowel {:?} has no options", e.latin));
        }
        for o in &e.options {
            check_cost(o.cost, &e.latin)?;
            let end = o.end.unwrap_or(o.cost);
            check_cost(end, &e.latin)?;
            units
                .entry(e.latin.clone())
                .or_default()
                .push(Choice::Vowel {
                    independent: target(block, &o.independent, "independent vowel", false)?,
                    sign: target(block, &o.sign, "vowel sign", true)?,
                    cost: o.cost,
                    end,
                });
        }
    }
    for e in &data.nasals {
        check_latin(&e.latin, "nasal")?;
        for (text, cost) in &e.options {
            check_cost(*cost, &e.latin)?;
            units
                .entry(e.latin.clone())
                .or_default()
                .push(Choice::Nasal {
                    text: target(block, text, "nasal", false)?,
                    cost: *cost,
                });
        }
    }
    Ok(units)
}

/// Whole lexicon words and the generic terms that may end a word, longest first.
type Terms = (BTreeMap<String, String>, Vec<(String, String)>);

fn term_tables(terms: &[Term], block: u32) -> Result<Terms, String> {
    let mut whole = BTreeMap::new();
    let mut suffixes = Vec::new();
    for t in terms {
        let latin = crate::fold(&t.latin).replace(' ', "");
        let native = nfc(&t.native);
        if latin.is_empty() || !latin.bytes().all(|b| b.is_ascii_alphanumeric()) {
            continue;
        }
        if native.is_empty() || !in_block(block, &native) {
            return Err(alloc::format!(
                "term {:?}: {:?} is not in the target script",
                t.latin,
                t.native
            ));
        }
        whole.entry(latin.clone()).or_insert_with(|| native.clone());
        if t.suffix && latin.len() >= MIN_SUFFIX {
            suffixes.push((latin, native));
        }
    }
    suffixes.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.cmp(b)));
    suffixes.dedup_by(|a, b| a.0 == b.0);
    Ok((whole, suffixes))
}

impl Profile {
    /// Compiles `data` with a country's lexicon `terms`.
    ///
    /// # Errors
    /// A unit that is not lower-case ASCII or has no options, an empty or out-of-script
    /// target text, a cost that is negative or not finite, a beam of zero.
    pub fn new(data: &ProfileData, terms: &[Term]) -> Result<Self, String> {
        let block = u32::from(data.virama) & !0x7F;
        let c = &data.costs;
        for (v, what) in [
            (c.schwa, "schwa"),
            (c.conjunct, "conjunct"),
            (c.final_virama, "final_virama"),
            (c.skip, "skip"),
            (c.lm_weight, "lm_weight"),
        ] {
            check_cost(v, what)?;
        }
        if c.beam == 0 {
            return Err("beam must be at least 1".into());
        }
        let units = unit_table(data, block)?;
        let longest = units.keys().map(String::len).max().unwrap_or(0);
        let mut phrases = Vec::new();
        for (latin, native) in &data.words {
            let words: Vec<String> = crate::fold(latin)
                .split(' ')
                .filter(|w| !w.is_empty())
                .map(str::to_string)
                .collect();
            if words.is_empty() {
                return Err(alloc::format!("word {latin:?} is empty"));
            }
            target(block, &native.replace(' ', ""), "word", false)?;
            phrases.push((words, nfc(native)));
        }
        phrases.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        phrases.dedup_by(|a, b| a.0 == b.0);
        let (whole, suffixes) = term_tables(terms, block)?;
        Ok(Self {
            virama: data.virama,
            block,
            zero: data.zero,
            costs: data.costs.clone(),
            units,
            longest,
            phrases,
            terms: whole,
            suffixes,
            examples: data.examples.clone(),
        })
    }

    /// The examples the profile must reproduce with a trained model.
    #[must_use]
    pub fn examples(&self) -> &[(String, String)] {
        &self.examples
    }

    /// The examples `model` does not reproduce, as `input: got, expected`.
    #[must_use]
    pub fn failed_examples(&self, model: &Model) -> Vec<String> {
        let mut scratch = Scratch::default();
        let mut out = String::new();
        let mut failed = Vec::new();
        for (input, want) in &self.examples {
            transliterate_with(self, model, &mut scratch, input, &mut out);
            if out != nfc(want) {
                failed.push(alloc::format!("{input}: {out}, expected {want}"));
            }
        }
        failed
    }

    /// The same profile with other costs (for measurement: switching the model off, A/B).
    #[must_use]
    pub fn with_costs(&self, costs: Costs) -> Self {
        Self {
            costs,
            ..self.clone()
        }
    }

    /// The profile's costs.
    #[must_use]
    pub fn costs(&self) -> &Costs {
        &self.costs
    }

    /// A model symbol for `c`: 1 is a word boundary, 2..=129 the block, 130 anything else.
    fn sym(&self, c: char) -> u8 {
        let u = u32::from(c);
        if u & !0x7F == self.block {
            #[allow(clippy::cast_possible_truncation, reason = "masked to 7 bits")]
            let s = (u & 0x7F) as u8;
            s + 2
        } else {
            OTHER
        }
    }
}

const BOUNDARY: u8 = 1;
const OTHER: u8 = 130;

/// A character n-gram model of the target script (interpolated Witten-Bell, stored in
/// back-off form as costs, `-ln p`) and a word memory.
#[derive(Clone, Debug, Default)]
pub struct Model {
    order: usize,
    /// N-grams and contexts, symbols packed big-endian, sorted.
    keys: Vec<u64>,
    /// `-ln p` of the n-gram; infinite for a context that is not itself an n-gram.
    cost: Vec<f32>,
    /// `-ln` of the context's back-off weight.
    bow: Vec<f32>,
    /// The cost of a symbol never seen.
    unseen: f32,
    /// Folded Latin word → target word, sorted.
    words: Vec<(String, String)>,
}

impl Model {
    /// The n-gram order.
    #[must_use]
    pub fn order(&self) -> usize {
        self.order
    }

    /// The number of n-gram and context entries.
    #[must_use]
    pub fn entries(&self) -> usize {
        self.keys.len()
    }

    /// The number of words in the memory.
    #[must_use]
    pub fn memory_words(&self) -> usize {
        self.words.len()
    }

    /// The bytes the tables take: 16 per entry (key, cost, back-off) plus the memory's text
    /// with a 4-byte length per word.
    #[must_use]
    pub fn size_bytes(&self) -> usize {
        self.keys.len() * 16
            + self
                .words
                .iter()
                .map(|(l, n)| l.len() + n.len() + 4)
                .sum::<usize>()
    }

    fn find(&self, key: u64) -> Option<usize> {
        self.keys.binary_search(&key).ok()
    }

    /// The cost of symbol `s` after `ctx` (its last `order - 1` symbols are used).
    fn score(&self, ctx: &[u8], s: u8) -> f32 {
        let keep = self.order.saturating_sub(1).min(ctx.len());
        let ctx = ctx.get(ctx.len() - keep..).unwrap_or_default();
        let mut acc = 0.0;
        for k in (0..=ctx.len()).rev() {
            let h = pack(ctx.get(ctx.len() - k..).unwrap_or_default());
            if let Some(i) = self.find((h << 8) | u64::from(s))
                && let Some(&c) = self.cost.get(i)
                && c.is_finite()
            {
                return acc + c;
            }
            if k > 0
                && let Some(i) = self.find(h)
            {
                acc += self.bow.get(i).copied().unwrap_or(0.0);
            }
        }
        acc + self.unseen
    }

    fn word(&self, latin: &str) -> Option<&str> {
        self.words
            .binary_search_by(|(l, _)| l.as_str().cmp(latin))
            .ok()
            .and_then(|i| self.words.get(i))
            .map(|(_, n)| n.as_str())
    }
}

/// A letter or mark of the 128-code-point `block`.
fn is_word_char(block: u32, c: char) -> bool {
    u32::from(c) & !0x7F == block
        && (c.is_alphabetic() || unicode_normalization::char::is_combining_mark(c))
}

fn pack(syms: &[u8]) -> u64 {
    syms.iter().fold(0, |k, &s| (k << 8) | u64::from(s))
}

/// The length in symbols of a packed key.
fn key_len(key: u64) -> u32 {
    (64 - key.leading_zeros()).div_ceil(8)
}

/// Trains a [`Model`] for a profile's script.
#[derive(Clone, Debug)]
pub struct ModelBuilder {
    order: usize,
    block: u32,
    counts: BTreeMap<u64, u32>,
    pairs: BTreeMap<String, BTreeMap<String, u32>>,
    min_pair: u32,
    max_distance: f32,
}

impl ModelBuilder {
    /// A builder of order `order` (2 to [`MAX_ORDER`]) for `profile`'s script.
    ///
    /// # Errors
    /// An order out of range.
    pub fn new(profile: &Profile, order: usize) -> Result<Self, String> {
        if !(2..=MAX_ORDER).contains(&order) {
            return Err(alloc::format!("order {order} must be 2 to {MAX_ORDER}"));
        }
        Ok(Self {
            order,
            block: profile.block,
            counts: BTreeMap::new(),
            pairs: BTreeMap::new(),
            min_pair: 2,
            max_distance: 0.34,
        })
    }

    /// The fewest times a word pair must be seen to enter the memory (default 2: a word seen
    /// once is as likely a typo or a misalignment as a spelling).
    pub fn min_pair(&mut self, n: u32) {
        self.min_pair = n.max(1);
    }

    /// The largest edit distance, as a share of the target word's length, between a memory
    /// word and the engine's own output for it (default 0.34). A farther pair is another name
    /// or a translation, not a spelling.
    pub fn max_distance(&mut self, d: f32) {
        self.max_distance = d.max(0.0);
    }

    fn syms(&self, word: &str) -> Vec<u8> {
        let mut v = Vec::with_capacity(word.len() / 2 + 2);
        v.push(BOUNDARY);
        for c in word.chars() {
            let u = u32::from(c);
            #[allow(clippy::cast_possible_truncation, reason = "masked to 7 bits")]
            v.push(if u & !0x7F == self.block {
                (u & 0x7F) as u8 + 2
            } else {
                OTHER
            });
        }
        v.push(BOUNDARY);
        v
    }

    /// Counts every word of `text` (runs of the script's letters and marks, NFC).
    pub fn add_text(&mut self, text: &str) {
        let text = nfc(text);
        let block = self.block;
        for w in text.split(|c: char| !is_word_char(block, c)) {
            if w.is_empty() {
                continue;
            }
            let s = self.syms(w);
            for end in 1..s.len() {
                for n in 1..=self.order.min(end + 1) {
                    if let Some(g) = s.get(end + 1 - n..=end) {
                        *self.counts.entry(pack(g)).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    /// Remembers the words of a name pair: the profile's phrases are matched on the Latin side
    /// first (and must appear as such on the target side), then each remaining folded Latin
    /// word is paired with the target word in its place, when the word counts agree. `weight`
    /// counts the pair as seen that many times (an official list outweighs a crowd label).
    pub fn add_pair(&mut self, profile: &Profile, latin: &str, native: &str, weight: u32) {
        let folded = crate::fold(latin);
        let tokens: Vec<&str> = folded.split(' ').filter(|w| !w.is_empty()).collect();
        let n = nfc(native);
        let nw: Vec<&str> = n.split_whitespace().collect();
        // (the Latin word, or none for a phrase; the target word it pairs with).
        let mut units: Vec<(Option<&str>, &str)> = Vec::new();
        let mut i = 0;
        let mut k = 0;
        while let Some(&t) = tokens.get(i) {
            if let Some((len, text)) = phrase_at(profile, &tokens, i) {
                let count = text.split_whitespace().count();
                let same = nw
                    .get(k..k + count)
                    .is_some_and(|w| w.iter().copied().eq(text.split_whitespace()));
                if !same {
                    return;
                }
                k += count;
                i += len;
            } else {
                let Some(&b) = nw.get(k) else {
                    return;
                };
                units.push((Some(t), b));
                k += 1;
                i += 1;
            }
        }
        if k != nw.len() {
            return;
        }
        for (a, b) in units {
            let Some(a) = a else {
                continue;
            };
            if !a.bytes().all(|c| c.is_ascii_lowercase())
                || !b.chars().all(|c| is_word_char(self.block, c))
            {
                continue;
            }
            *self
                .pairs
                .entry(a.to_string())
                .or_default()
                .entry(b.to_string())
                .or_insert(0) += weight;
        }
    }

    /// The trained model. The memory keeps each Latin word's most frequent target word (ties
    /// by text) when it was seen often enough, the engine with the trained model writes it
    /// otherwise, and the two are close (see [`Self::max_distance`]).
    #[must_use]
    pub fn build(self, profile: &Profile) -> Model {
        // Context totals C(h) and distinct followers T(h).
        let mut ctx: BTreeMap<u64, (u64, u64)> = BTreeMap::new();
        for (&g, &n) in &self.counts {
            let e = ctx.entry(g >> 8).or_insert((0, 0));
            e.0 += u64::from(n);
            e.1 += 1;
        }
        let vocab = self.counts.keys().filter(|&&g| g <= 0xFF).count() + 1;
        #[allow(clippy::cast_precision_loss, reason = "a symbol count")]
        let uniform = 1.0 / vocab as f64;
        // Probabilities by increasing order, so the lower order is ready.
        let mut prob: BTreeMap<u64, f64> = BTreeMap::new();
        let mut by_len: Vec<Vec<(u64, u32)>> = alloc::vec![Vec::new(); self.order + 1];
        for (&g, &n) in &self.counts {
            if let Some(v) = by_len.get_mut(key_len(g) as usize) {
                v.push((g, n));
            }
        }
        let lambda = |h: u64| -> (f64, f64) {
            let (c, t) = ctx.get(&h).copied().unwrap_or((0, 0));
            #[allow(clippy::cast_precision_loss, reason = "counts below 2^52")]
            (c as f64, t as f64)
        };
        for (len, grams) in by_len.iter().enumerate().skip(1) {
            for &(g, n) in grams {
                let h = g >> 8;
                let lower = if len == 1 {
                    uniform
                } else {
                    let mask = (1_u64 << (8 * (len - 1))) - 1;
                    prob.get(&(g & mask)).copied().unwrap_or(uniform)
                };
                let (c, t) = lambda(h);
                let p = (f64::from(n) + t * lower) / (c + t);
                prob.insert(g, p);
            }
        }
        let (c0, t0) = lambda(0);
        let unseen = -libm::log((t0 / (c0 + t0)) * uniform);
        let mut keys: Vec<u64> = prob.keys().copied().collect();
        for &h in ctx.keys() {
            if h != 0 && !prob.contains_key(&h) {
                keys.push(h);
            }
        }
        keys.sort_unstable();
        #[allow(clippy::cast_possible_truncation, reason = "costs are stored as f32")]
        let cost = keys
            .iter()
            .map(|k| {
                prob.get(k)
                    .map_or(f32::INFINITY, |p| (-libm::log(*p)) as f32)
            })
            .collect();
        #[allow(clippy::cast_possible_truncation, reason = "costs are stored as f32")]
        let bow = keys
            .iter()
            .map(|k| {
                let (c, t) = lambda(*k);
                if t > 0.0 {
                    (-libm::log(t / (c + t))) as f32
                } else {
                    0.0
                }
            })
            .collect();
        #[allow(clippy::cast_possible_truncation, reason = "a cost stored as f32")]
        let unseen = unseen as f32;
        let mut model = Model {
            order: self.order,
            keys,
            cost,
            bow,
            unseen,
            words: Vec::new(),
        };
        model.words = memory(
            self.pairs,
            self.min_pair,
            self.max_distance,
            profile,
            &model,
        );
        model
    }
}

/// The word memory: each Latin word's most frequent target word (ties by text), kept when
/// seen at least `min_pair` times, written otherwise by the engine with `model`, and within
/// `max_distance` of the engine's output.
fn memory(
    pairs: BTreeMap<String, BTreeMap<String, u32>>,
    min_pair: u32,
    max_distance: f32,
    profile: &Profile,
    model: &Model,
) -> Vec<(String, String)> {
    let mut words = Vec::new();
    let mut scratch = Scratch::default();
    let mut out = String::new();
    for (latin, natives) in pairs {
        let best = natives
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)));
        let Some((native, n)) = best else {
            continue;
        };
        if n < min_pair {
            continue;
        }
        out.clear();
        lattice(profile, model, &mut scratch, &latin, &mut out);
        if out == native {
            continue;
        }
        let a: Vec<char> = out.chars().collect();
        let b: Vec<char> = native.chars().collect();
        #[allow(clippy::cast_precision_loss, reason = "word lengths")]
        let d = distance(&a, &b) as f32 / b.len().max(1) as f32;
        if d <= max_distance {
            words.push((latin, native));
        }
    }
    words
}

/// The Levenshtein distance between two character strings.
#[must_use]
pub fn distance(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = alloc::vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        if let Some(c) = cur.first_mut() {
            *c = i + 1;
        }
        for (j, cb) in b.iter().enumerate() {
            let sub = prev.get(j).copied().unwrap_or(0) + usize::from(ca != cb);
            let del = prev.get(j + 1).copied().unwrap_or(0) + 1;
            let ins = cur.get(j).copied().unwrap_or(0) + 1;
            if let Some(c) = cur.get_mut(j + 1) {
                *c = sub.min(del).min(ins);
            }
        }
        core::mem::swap(&mut prev, &mut cur);
    }
    prev.get(b.len()).copied().unwrap_or(0)
}

/// What the last written letter leaves open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// The start of the word.
    Start,
    /// A consonant whose inherent vowel is not yet decided.
    Bare,
    /// A vowel (a sign, the inherent vowel, or an independent letter) or a digit.
    Vowel,
    /// A nasal sign.
    Nasal,
}

#[derive(Clone, Copy, Debug)]
struct Hyp {
    cost: f32,
    node: u32,
    kind: Kind,
    ctx: [u8; MAX_ORDER],
    len: u8,
}

impl Hyp {
    fn ctx(&self) -> &[u8] {
        self.ctx.get(..usize::from(self.len)).unwrap_or_default()
    }

    fn same_state(&self, o: &Self) -> bool {
        self.kind == o.kind && self.ctx() == o.ctx()
    }
}

#[derive(Clone, Copy, Debug)]
struct Node {
    parent: u32,
    start: u32,
    len: u32,
}

/// Reusable working memory for [`transliterate_with`].
#[derive(Clone, Debug, Default)]
pub struct Scratch {
    nodes: Vec<Node>,
    chars: Vec<char>,
    beams: Vec<Vec<Hyp>>,
    text: Vec<char>,
    word: String,
}

/// Transliterates `latin` into `out` (cleared first) with fresh working memory.
pub fn transliterate(profile: &Profile, model: &Model, latin: &str, out: &mut String) {
    transliterate_with(profile, model, &mut Scratch::default(), latin, out);
}

/// Transliterates `latin` into `out` (cleared first), reusing `scratch`.
pub fn transliterate_with(
    profile: &Profile,
    model: &Model,
    scratch: &mut Scratch,
    latin: &str,
    out: &mut String,
) {
    out.clear();
    let folded = crate::fold(latin);
    let tokens: Vec<&str> = folded.split(' ').filter(|w| !w.is_empty()).collect();
    let mut i = 0;
    while i < tokens.len() {
        let start = out.len();
        if !out.is_empty() {
            out.push(' ');
        }
        let body = out.len();
        if let Some((n, text)) = phrase_at(profile, &tokens, i) {
            out.push_str(text);
            i += n;
        } else {
            if let Some(t) = tokens.get(i) {
                word(profile, model, scratch, t, out);
            }
            i += 1;
        }
        if out.len() == body {
            out.truncate(start);
        }
    }
}

fn phrase_at<'p>(profile: &'p Profile, tokens: &[&str], i: usize) -> Option<(usize, &'p str)> {
    profile.phrases.iter().find_map(|(words, text)| {
        let span = tokens.get(i..i + words.len())?;
        span.iter()
            .zip(words)
            .all(|(a, b)| *a == b.as_str())
            .then_some((words.len(), text.as_str()))
    })
}

/// One folded token into `out`.
fn word(profile: &Profile, model: &Model, scratch: &mut Scratch, w: &str, out: &mut String) {
    if !w.is_ascii() {
        return;
    }
    if let Some(z) = profile.zero
        && w.bytes().all(|b| b.is_ascii_digit())
    {
        out.extend(
            w.bytes()
                .filter_map(|d| char::from_u32(u32::from(z) + u32::from(d - b'0'))),
        );
        return;
    }
    if let Some(t) = fixed(profile, model, w) {
        out.push_str(t);
        return;
    }
    for (suffix, native) in &profile.suffixes {
        if let Some(stem) = w.strip_suffix(suffix.as_str())
            && stem.len() >= MIN_STEM
        {
            match fixed(profile, model, stem) {
                Some(t) => out.push_str(t),
                None => lattice(profile, model, scratch, stem, out),
            }
            out.push_str(native);
            return;
        }
    }
    lattice(profile, model, scratch, w, out);
}

/// A word written as stored: a one-word entry of the profile's `words`, a lexicon term, or a
/// word of the model's memory, in that order.
fn fixed<'a>(profile: &'a Profile, model: &'a Model, w: &str) -> Option<&'a str> {
    profile
        .phrases
        .iter()
        .find(|(words, _)| words.len() == 1 && words.first().is_some_and(|x| x == w))
        .map(|(_, t)| t.as_str())
        .or_else(|| profile.terms.get(w).map(String::as_str))
        .or_else(|| model.word(w))
}

/// Pushes `text` onto `hyp`'s path: writes its letters into the arena and scores them.
fn extend(
    profile: &Profile,
    model: &Model,
    scratch: &mut Scratch,
    hyp: &Hyp,
    virama: bool,
    text: &str,
) -> Hyp {
    let mut h = *hyp;
    #[allow(
        clippy::cast_possible_truncation,
        reason = "arena sizes stay far below 2^32"
    )]
    let start = scratch.chars.len() as u32;
    let order = model.order.clamp(1, MAX_ORDER);
    let push = |c: char, h: &mut Hyp, scratch: &mut Scratch| {
        let s = profile.sym(c);
        h.cost += profile.costs.lm_weight * model.score(h.ctx(), s);
        let keep = order - 1;
        if usize::from(h.len) == keep && keep > 0 {
            h.ctx.copy_within(1..keep, 0);
            if let Some(slot) = h.ctx.get_mut(keep - 1) {
                *slot = s;
            }
        } else if keep > 0 {
            if let Some(slot) = h.ctx.get_mut(usize::from(h.len)) {
                *slot = s;
            }
            h.len += 1;
        }
        scratch.chars.push(c);
    };
    if virama {
        push(profile.virama, &mut h, scratch);
    }
    for c in text.chars() {
        push(c, &mut h, scratch);
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "arena sizes stay far below 2^32"
    )]
    let len = scratch.chars.len() as u32 - start;
    #[allow(
        clippy::cast_possible_truncation,
        reason = "arena sizes stay far below 2^32"
    )]
    let id = scratch.nodes.len() as u32;
    scratch.nodes.push(Node {
        parent: hyp.node,
        start,
        len,
    });
    h.node = id;
    h
}

fn insert(beam: &mut Vec<Hyp>, h: Hyp, width: usize) {
    if let Some(o) = beam.iter_mut().find(|o| o.same_state(&h)) {
        if h.cost < o.cost {
            *o = h;
        }
        return;
    }
    beam.push(h);
    if beam.len() > width * 4 {
        prune(beam, width);
    }
}

fn prune(beam: &mut Vec<Hyp>, width: usize) {
    beam.sort_by(|a, b| a.cost.total_cmp(&b.cost));
    beam.truncate(width);
}

/// The beam search over one ASCII word.
fn lattice(profile: &Profile, model: &Model, scratch: &mut Scratch, w: &str, out: &mut String) {
    let n = w.len();
    let width = usize::from(profile.costs.beam);
    scratch.nodes.clear();
    scratch.chars.clear();
    scratch.nodes.push(Node {
        parent: u32::MAX,
        start: 0,
        len: 0,
    });
    for b in &mut scratch.beams {
        b.clear();
    }
    if scratch.beams.len() < n + 1 {
        scratch.beams.resize_with(n + 1, Vec::new);
    }
    let mut first = Hyp {
        cost: 0.0,
        node: 0,
        kind: Kind::Start,
        ctx: [0; MAX_ORDER],
        len: 0,
    };
    if model.order > 1
        && let Some(slot) = first.ctx.first_mut()
    {
        *slot = BOUNDARY;
        first.len = 1;
    }
    if let Some(b) = scratch.beams.get_mut(0) {
        b.push(first);
    }
    let mut here: Vec<Hyp> = Vec::new();
    for i in 0..n {
        here.clear();
        if let Some(b) = scratch.beams.get_mut(i) {
            prune(b, width);
            here.extend_from_slice(b);
        }
        for hyp in &here {
            // A dropped character keeps the state.
            let mut skip = *hyp;
            skip.cost += profile.costs.skip;
            if let Some(b) = scratch.beams.get_mut(i + 1) {
                insert(b, skip, width);
            }
            for l in 1..=profile.longest.min(n - i) {
                let Some(choices) = w.get(i..i + l).and_then(|u| profile.units.get(u)) else {
                    continue;
                };
                let j = i + l;
                for ch in choices {
                    expand(profile, model, scratch, hyp, ch, j == n, j, width);
                }
            }
        }
    }
    // The word's end.
    let mut best: Option<(f32, u32)> = None;
    let mut ends: Vec<Hyp> = scratch.beams.get(n).cloned().unwrap_or_default();
    prune(&mut ends, width);
    for hyp in &ends {
        let mut cands = Vec::with_capacity(2);
        cands.push((*hyp, 0.0_f32));
        if hyp.kind == Kind::Bare {
            let h = extend(profile, model, scratch, hyp, true, "");
            cands.push((h, profile.costs.final_virama));
        }
        for (h, extra) in cands {
            let total = h.cost + extra + profile.costs.lm_weight * model.score(h.ctx(), BOUNDARY);
            if best.is_none_or(|(c, _)| total < c) {
                best = Some((total, h.node));
            }
        }
    }
    let Some((_, mut node)) = best else {
        return;
    };
    scratch.text.clear();
    while let Some(nd) = scratch.nodes.get(node as usize) {
        for k in (nd.start..nd.start + nd.len).rev() {
            if let Some(&c) = scratch.chars.get(k as usize) {
                scratch.text.push(c);
            }
        }
        node = nd.parent;
    }
    scratch.word.clear();
    scratch.word.extend(scratch.text.iter().rev());
    out.extend(scratch.word.nfc());
}

#[allow(
    clippy::too_many_arguments,
    reason = "one step of the search, all its state"
)]
fn expand(
    profile: &Profile,
    model: &Model,
    scratch: &mut Scratch,
    hyp: &Hyp,
    ch: &Choice,
    at_end: bool,
    j: usize,
    width: usize,
) {
    let add = |h: Hyp, scratch: &mut Scratch| {
        if let Some(b) = scratch.beams.get_mut(j) {
            insert(b, h, width);
        }
    };
    match ch {
        Choice::Consonant { text, cost } => {
            if hyp.kind == Kind::Bare {
                let mut a = extend(profile, model, scratch, hyp, false, text);
                a.cost += cost + profile.costs.schwa;
                a.kind = Kind::Bare;
                add(a, scratch);
                let mut b = extend(profile, model, scratch, hyp, true, text);
                b.cost += cost + profile.costs.conjunct;
                b.kind = Kind::Bare;
                add(b, scratch);
            } else {
                let mut a = extend(profile, model, scratch, hyp, false, text);
                a.cost += cost;
                a.kind = Kind::Bare;
                add(a, scratch);
            }
        }
        Choice::Vowel {
            independent,
            sign,
            cost,
            end,
        } => {
            let text = if hyp.kind == Kind::Bare {
                sign
            } else {
                independent
            };
            let mut a = extend(profile, model, scratch, hyp, false, text);
            a.cost += if at_end { *end } else { *cost };
            a.kind = Kind::Vowel;
            add(a, scratch);
        }
        Choice::Nasal { text, cost } => {
            if matches!(hyp.kind, Kind::Vowel | Kind::Bare) {
                let mut a = extend(profile, model, scratch, hyp, false, text);
                a.cost += cost;
                a.kind = Kind::Nasal;
                add(a, scratch);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;

    use super::{
        Costs, Entry, Model, ModelBuilder, Profile, ProfileData, Term, VowelEntry, VowelOption,
        transliterate,
    };

    fn e(latin: &str, opts: &[(&str, f32)]) -> Entry {
        Entry {
            latin: latin.into(),
            options: opts.iter().map(|(t, c)| ((*t).into(), *c)).collect(),
        }
    }

    fn v(latin: &str, opts: &[(&str, &str, f32, Option<f32>)]) -> VowelEntry {
        VowelEntry {
            latin: latin.into(),
            options: opts
                .iter()
                .map(|(i, s, c, end)| VowelOption {
                    independent: (*i).into(),
                    sign: (*s).into(),
                    cost: *c,
                    end: *end,
                })
                .collect(),
        }
    }

    /// A small profile in the shape of the data file.
    fn data() -> ProfileData {
        ProfileData {
            virama: '्',
            zero: Some('०'),
            costs: Costs {
                schwa: 0.0,
                conjunct: 0.5,
                final_virama: 4.0,
                skip: 8.0,
                lm_weight: 1.0,
                beam: 16,
            },
            consonants: vec![
                e("k", &[("क", 0.0)]),
                e("kh", &[("ख", 0.0)]),
                e("g", &[("ग", 0.0)]),
                e("t", &[("त", 0.0), ("ट", 0.5)]),
                e("th", &[("थ", 0.0), ("ठ", 0.5)]),
                e("d", &[("द", 0.0), ("ड", 0.5)]),
                e("n", &[("न", 0.0)]),
                e("p", &[("प", 0.0)]),
                e("b", &[("ब", 0.0), ("व", 0.8)]),
                e("m", &[("म", 0.0)]),
                e("r", &[("र", 0.0)]),
                e("l", &[("ल", 0.0)]),
                e("h", &[("ह", 0.0)]),
                e("s", &[("स", 0.0)]),
            ],
            vowels: vec![
                v(
                    "a",
                    &[("अ", "", 0.0, Some(3.0)), ("आ", "ा", 1.0, Some(0.0))],
                ),
                v("aa", &[("आ", "ा", 0.0, None)]),
                v("i", &[("इ", "ि", 0.0, None), ("ई", "ी", 0.7, None)]),
                v("u", &[("उ", "ु", 0.0, None), ("ऊ", "ू", 0.7, None)]),
                v("o", &[("ओ", "ो", 0.0, None)]),
                v("au", &[("औ", "ौ", 0.0, None)]),
            ],
            nasals: vec![e("n", &[("ं", 1.0), ("ँ", 1.2)])],
            words: vec![("rural municipality".into(), "गाउँपालिका".into())],
            examples: Vec::new(),
        }
    }

    fn terms() -> Vec<Term> {
        vec![
            Term {
                latin: "chowk".into(),
                native: "चोक".into(),
                suffix: true,
            },
            Term {
                latin: "gaun".into(),
                native: "गाउँ".into(),
                suffix: true,
            },
        ]
    }

    /// A model trained on words that share letters with the examples, not the examples.
    fn model(p: &Profile) -> Model {
        let mut b = ModelBuilder::new(p, 5).expect("order");
        b.add_text(
            "ललित ललिता पुर पुरा नगर नगरी विराट विराज पोखरी खरा कपुर सुन्दरपुर \
             रामपुर लहान लहर तिलक कालीगण्डकी गोरखा धनकुटा बनेपा हेटौंडा",
        );
        b.build(p)
    }

    fn x(p: &Profile, m: &Model, s: &str) -> String {
        let mut out = String::new();
        transliterate(p, m, s, &mut out);
        out
    }

    #[test]
    fn golden_examples_with_a_small_model() {
        let p = Profile::new(&data(), &terms()).expect("profile");
        let m = model(&p);
        assert_eq!(x(&p, &m, "Lalitpur"), "ललितपुर");
        assert_eq!(x(&p, &m, "Pokhara"), "पोखरा");
        assert_eq!(x(&p, &m, "Gaun"), "गाउँ");
        assert_eq!(x(&p, &m, "Lahachowk"), "लहाचोक");
        assert_eq!(x(&p, &m, "Biratnagar"), "विराटनगर");
        assert_eq!(x(&p, &m, "Ramgaun Rural Municipality"), "रामगाउँ गाउँपालिका");
    }

    #[test]
    fn deterministic_and_in_script() {
        let p = Profile::new(&data(), &terms()).expect("profile");
        let m = model(&p);
        for s in [
            "Lalitpur 3",
            "Tilak-Kot",
            "Qxz",
            "",
            "  ",
            "São Paulo",
            "Ωmega",
            "ललित",
        ] {
            let a = x(&p, &m, s);
            assert_eq!(a, x(&p, &m, s));
            assert!(
                a.chars()
                    .all(|c| c == ' ' || ('\u{0900}'..='\u{097F}').contains(&c)),
                "{s}: {a}"
            );
            assert!(
                !a.starts_with(' ') && !a.ends_with(' ') && !a.contains("  "),
                "{s}: {a:?}"
            );
        }
        assert_eq!(x(&p, &m, "Lalitpur 3"), "ललितपुर ३");
    }

    #[test]
    fn the_model_decides_retroflex_and_vowel_length() {
        let p = Profile::new(&data(), &[]).expect("profile");
        let mut b = ModelBuilder::new(&p, 4).expect("order");
        b.add_text("टाट टाटी टाटी");
        let m = b.build(&p);
        // The profile prefers त and ि; the model overrules both.
        assert_eq!(x(&p, &m, "Tati"), "टाटी");
        let mut b = ModelBuilder::new(&p, 4).expect("order");
        b.add_text("तात ताति ताति");
        assert_eq!(x(&p, &b.build(&p), "Tati"), "ताति");
    }

    #[test]
    fn word_memory_from_pairs() {
        let p = Profile::new(&data(), &[]).expect("profile");
        let mut b = ModelBuilder::new(&p, 3).expect("order");
        b.max_distance(1.0);
        b.add_pair(&p, "Kathmandu Rural Municipality", "काठमाडौं गाउँपालिका", 1);
        b.add_pair(&p, "Kathmandu Valley", "काठमाडौं उपत्यका", 1);
        b.add_pair(&p, "Lalit", "गाउँपालिका", 1);
        let m = b.build(&p);
        // Kathmandu twice (once through a phrase) and close to the engine's own output;
        // Valley once; Lalit once and misaligned.
        assert_eq!(m.memory_words(), 1);
        assert_eq!(x(&p, &m, "Kathmandu"), "काठमाडौं");
    }

    #[test]
    fn a_corrupted_profile_misses_the_examples() {
        let mut d = data();
        // Planted fault: the long ा sign written as the ि sign.
        for ve in &mut d.vowels {
            for o in &mut ve.options {
                if o.sign == "ा" {
                    o.sign = "ि".into();
                }
            }
        }
        let p = Profile::new(&d, &terms()).expect("profile");
        let m = model(&p);
        assert_ne!(x(&p, &m, "Pokhara"), "पोखरा");
    }

    #[test]
    fn bad_profiles_are_refused() {
        let mut d = data();
        d.consonants.push(e("K", &[("क", 0.0)]));
        assert!(Profile::new(&d, &[]).is_err());
        let mut d = data();
        d.consonants.push(e("q", &[("q", 0.0)]));
        assert!(Profile::new(&d, &[]).is_err());
        let mut d = data();
        d.consonants.push(e("q", &[("क", -1.0)]));
        assert!(Profile::new(&d, &[]).is_err());
        let mut d = data();
        d.costs.beam = 0;
        assert!(Profile::new(&d, &[]).is_err());
        let p = Profile::new(&data(), &[]).expect("profile");
        assert!(ModelBuilder::new(&p, 9).is_err());
        assert!(ModelBuilder::new(&p, 1).is_err());
    }

    #[test]
    fn model_probabilities_sum_to_one() {
        let p = Profile::new(&data(), &[]).expect("profile");
        let mut b = ModelBuilder::new(&p, 3).expect("order");
        b.add_text("कमल कमला कलम मल");
        let m = b.build(&p);
        let ctx = [super::BOUNDARY, p.sym('क')];
        // The seen symbols and one unseen one (the unseen mass is one share).
        let seen: Vec<u8> = "कमलाकलम"
            .chars()
            .map(|c| p.sym(c))
            .chain([super::BOUNDARY])
            .collect::<alloc::collections::BTreeSet<u8>>()
            .into_iter()
            .collect();
        let mut total: f64 = seen
            .iter()
            .map(|&s| f64::from(-m.score(&ctx, s)).exp())
            .sum();
        total += f64::from(-m.score(&ctx, super::OTHER)).exp();
        assert!((total - 1.0).abs() < 1e-4, "{total}");
    }
}
