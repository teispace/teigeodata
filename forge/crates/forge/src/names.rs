//! Names: canonical display names, lexicon rendering and romanisation (phase 3 step 3.6,
//! [note](../../../../docs/02-research/notes/names-canonical.md)).
//!
//! ```text
//! forge romanise <cc> <lang>-<Script> < names.txt
//! ```
//!
//! A romanisation profile is `data/romanise/<lang>-<Script>.yaml` (schema
//! `teigeodata/romanise/v1`); its morphemes are the native forms in that script of the
//! country's accepted lexicon terms (`data/countries/<cc>/lexicon.yaml`), written in their
//! canonical form. A country lists the profiles its names use in `profile.yaml`
//! (`romanise: [ne-Deva]`). `forge romanise` reads one name per line on standard input and
//! writes its romanisation per line, for review and measurement.
//!
//! [`Namer::choose`] picks a place's display name (names-canonical §5.2): the majority of its
//! records' Latin names after lexicon rendering (ties by source order), else the romanisation
//! of its first name in a script the country romanises, else nothing (the caller's fallback).

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, Write};
use std::path::Path;

use serde::Deserialize;
use teigeodata_names::romanise::{Profile, ProfileFile};
use teigeodata_names::{join_key, script_code, strip_accents};

/// The parts of a lexicon this module reads; the rest is checked by `check-data`.
#[derive(Deserialize)]
struct Lexicon {
    terms: Vec<Term>,
}

#[derive(Deserialize)]
struct Term {
    canonical: String,
    #[serde(default)]
    native: Vec<Native>,
    #[serde(default)]
    variants: Vec<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize)]
struct Native {
    script: String,
    text: String,
}

/// Country `cc`'s native-script generics, folded, each with its Latin spellings (canonical
/// first, then the variants), folded: the query rules of mixed-script search (D-120). Longest
/// native form first.
pub(crate) fn native_generics(root: &Path, cc: &str) -> Result<Vec<(String, Vec<String>)>, String> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for t in accepted_terms(root, cc)? {
        let spellings: Vec<String> = std::iter::once(&t.canonical)
            .chain(&t.variants)
            .map(|s| teigeodata_names::fold(s))
            .filter(|s| !s.is_empty())
            .collect();
        for n in &t.native {
            let native = teigeodata_names::fold(&n.text);
            if !native.is_empty() && !spellings.is_empty() {
                out.push((native, spellings.clone()));
            }
        }
    }
    out.sort_by(|a, b| {
        b.0.chars()
            .count()
            .cmp(&a.0.chars().count())
            .then(a.0.cmp(&b.0))
    });
    out.dedup_by(|a, b| a.0 == b.0);
    Ok(out)
}

/// Country `cc`'s accepted lexicon terms; none when it has no lexicon.
fn accepted_terms(root: &Path, cc: &str) -> Result<Vec<Term>, String> {
    let lex = root
        .join("data/countries")
        .join(cc.to_ascii_lowercase())
        .join("lexicon.yaml");
    if !lex.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&lex).map_err(|e| format!("{}: {e}", lex.display()))?;
    let l: Lexicon =
        serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", lex.display()))?;
    Ok(l.terms
        .into_iter()
        .filter(|t| t.status.as_deref().is_none_or(|s| s == "accepted"))
        .collect())
}

/// Country `cc`'s accepted lexicon terms for transliteration into `script`: each Latin
/// spelling (canonical, then variants) with the term's first native
/// form in that script. Generic terms may end a compound word.
///
/// # Errors
/// A bad lexicon.
pub(crate) fn xlit_terms(
    root: &Path,
    cc: &str,
    script: &str,
) -> Result<Vec<teigeodata_names::xlit::Term>, String> {
    let mut out = Vec::new();
    for t in accepted_terms(root, cc)? {
        let Some(native) = t.native.iter().find(|n| n.script == script) else {
            continue;
        };
        let suffix = t.kind.as_deref() == Some("generic");
        for latin in std::iter::once(&t.canonical).chain(&t.variants) {
            out.push(teigeodata_names::xlit::Term {
                latin: latin.clone(),
                native: native.text.clone(),
                suffix,
            });
        }
    }
    Ok(out)
}

/// The romanisation profile `<lang>-<Script>` with the morphemes of country `cc`'s lexicon.
fn profile_with(root: &Path, terms: &[Term], id: &str) -> Result<(String, Profile), String> {
    let path = root.join("data/romanise").join(format!("{id}.yaml"));
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file: ProfileFile =
        serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let morphemes: Vec<(String, String)> = terms
        .iter()
        .flat_map(|t| {
            t.native
                .iter()
                .filter(|n| n.script == file.script)
                .map(|n| (n.text.clone(), t.canonical.clone()))
        })
        .collect();
    let p = file
        .compile(id, &morphemes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((file.script, p))
}

/// The romanisation profile `id` with country `cc`'s lexicon morphemes.
///
/// # Errors
/// A missing or invalid profile or lexicon, or a profile that fails its own examples.
pub(crate) fn profile(root: &Path, cc: &str, id: &str) -> Result<Profile, String> {
    Ok(profile_with(root, &accepted_terms(root, cc)?, id)?.1)
}

/// How a display name was chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum How {
    /// The only Latin name among the records (after rendering).
    Latin,
    /// The majority of two or more distinct Latin names.
    Voted,
    /// Romanised from a native-script name.
    Romanised,
}

/// A chosen display name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Choice {
    pub(crate) text: String,
    pub(crate) how: How,
    /// The lexicon changed the winning source spelling.
    pub(crate) rendered: bool,
}

/// A country's naming data: its lexicon variants and romanisers.
pub(crate) struct Namer {
    /// Lower-case variant → (canonical spelling, may end a compound word), longest first.
    /// `not_respelled` forms are not here: a display name keeps them.
    variants: Vec<(String, (String, bool))>,
    /// (ISO 15924 script, profile), in the country profile's order.
    romanisers: Vec<(String, Profile)>,
}

/// The shortest variant matched as a suffix, and the shortest stem it may leave: shorter ones
/// match inside unrelated words.
const MIN_SUFFIX: usize = 3;
const MIN_STEM: usize = 3;

impl Namer {
    /// Country `cc`'s lexicon and the romanisation profiles `ids`.
    ///
    /// # Errors
    /// A bad lexicon or profile.
    pub(crate) fn load(root: &Path, cc: &str, ids: &[String]) -> Result<Self, String> {
        let terms = accepted_terms(root, cc)?;
        // Only generic terms end compounds (Lahachok); an admin suffix is a word of its own.
        let mut variants: Vec<(String, (String, bool))> = terms
            .iter()
            .flat_map(|t| {
                let suffix = t.kind.as_deref() == Some("generic");
                t.variants
                    .iter()
                    .map(move |v| (v.to_lowercase(), (t.canonical.clone(), suffix)))
            })
            .collect();
        variants.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.cmp(b)));
        variants.dedup_by(|a, b| a.0 == b.0);
        let romanisers = ids
            .iter()
            .map(|id| profile_with(root, &terms, id))
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            variants,
            romanisers,
        })
    }

    /// `text` with each word that is a lexicon variant, or ends in one, written in the
    /// canonical spelling (Lahachok → Lahachowk, Laha Chok → Laha Chowk).
    /// Whitespace becomes single spaces.
    pub(crate) fn render(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + 4);
        for (i, word) in text.split_whitespace().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(&self.render_word(word));
        }
        out
    }

    fn render_word(&self, word: &str) -> String {
        let lower = word.to_lowercase();
        // Only when lower-casing keeps byte offsets (true for Latin names).
        if lower.len() != word.len() {
            return word.to_string();
        }
        for (v, (canon, _)) in &self.variants {
            if lower == *v {
                return canon.clone();
            }
        }
        for (v, (canon, suffix)) in &self.variants {
            let cut = word.len().saturating_sub(v.len());
            if *suffix
                && v.len() >= MIN_SUFFIX
                && cut >= MIN_STEM
                && lower.ends_with(v.as_str())
                && word.is_char_boundary(cut)
                && let Some(stem) = word.get(..cut)
            {
                return format!("{stem}{}", canon.to_lowercase());
            }
        }
        word.to_string()
    }

    /// The display name of a place from its records' names, `records` in source priority
    /// order, each with its candidate names (primary first). `fits` says whether a name can
    /// be stored.
    ///
    /// Latin names vote by their join key after rendering; a name that lists two (`A / B`,
    /// `A; B`) does not vote. The most votes win, then a name written without diacritics, then
    /// the earliest; the winner is shown without diacritics.
    pub(crate) fn choose(
        &self,
        records: &[Vec<&str>],
        fits: impl Fn(&str) -> bool,
    ) -> Option<Choice> {
        // join key → (votes, written plainly, first seen order, rendered text, changed by
        // the lexicon)
        let mut votes: BTreeMap<String, (usize, bool, usize, String, bool)> = BTreeMap::new();
        let mut order = 0;
        for names in records {
            let mut seen = Vec::new();
            for &n in names {
                if script_code(n) != Some("Latn") || n.contains(" / ") || n.contains(';') {
                    continue;
                }
                let plain_text = display_form(n);
                let r = self.render(&plain_text);
                if !fits(&r) {
                    continue;
                }
                let key = join_key(&r);
                if key.is_empty() || seen.contains(&key) {
                    continue;
                }
                seen.push(key.clone());
                let plain = n.is_ascii();
                let changed = r != plain_text.split_whitespace().collect::<Vec<_>>().join(" ");
                let e = votes.entry(key).or_insert((0, plain, order, r, changed));
                e.0 += 1;
                // A plain spelling of the same key replaces an accented one.
                if plain && !e.1 {
                    e.1 = true;
                }
                order += 1;
            }
        }
        if let Some((_, (_, _, _, text, rendered))) = votes.iter().max_by(|a, b| {
            (a.1.0, a.1.1)
                .cmp(&(b.1.0, b.1.1))
                .then_with(|| b.1.2.cmp(&a.1.2))
        }) {
            let how = if votes.len() > 1 {
                How::Voted
            } else {
                How::Latin
            };
            return Some(Choice {
                text: text.clone(),
                how,
                rendered: *rendered,
            });
        }
        for names in records {
            for &n in names {
                if let Some(r) = self.romanise(n).filter(|r| fits(r)) {
                    return Some(Choice {
                        text: r,
                        how: How::Romanised,
                        rendered: false,
                    });
                }
            }
        }
        None
    }

    /// A name in Latin letters by the country's romaniser for its script; `None` when it has
    /// none for that script or the result is empty.
    pub(crate) fn romanise(&self, n: &str) -> Option<String> {
        let script = script_code(n)?;
        let (_, p) = self.romanisers.iter().find(|(s, _)| s == script)?;
        Some(p.romanise(n)).filter(|r| !r.is_empty())
    }
}

/// A Latin name as displayed: accents dropped and invisible format characters (zero-width
/// joiners and spaces, soft hyphens, byte-order marks) removed.
fn display_form(n: &str) -> String {
    strip_accents(n)
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '\u{00AD}' | '\u{200B}'..='\u{200F}' | '\u{2060}' | '\u{FEFF}'
            )
        })
        .collect()
}

/// `forge romanise <cc> <lang>-<Script>`: standard input to standard output, line by line.
///
/// # Errors
/// A bad profile, or failing to read or write.
pub(crate) fn run(root: &Path, cc: &str, id: &str) -> Result<String, String> {
    let p = profile(root, cc, id)?;
    let stdin = std::io::stdin();
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    let mut n = 0_usize;
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        writeln!(out, "{}", p.romanise(&line)).map_err(|e| e.to_string())?;
        n += 1;
    }
    out.flush().map_err(|e| e.to_string())?;
    Ok(format!(
        "romanised {n} names with {id} and the {cc} lexicon"
    ))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{How, Namer};

    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
    }

    #[test]
    fn the_nepali_profile_loads_and_uses_the_lexicon() {
        let p = super::profile(&root(), "np", "ne-Deva").expect("ne-Deva loads");
        assert_eq!(p.romanise("लहाचोक"), "Lahachowk");
        assert_eq!(p.romanise("सारदानगर"), "Sardanagar");
        assert_eq!(p.romanise("पिपल चौतारा टोल"), "Pipal Chautara Tole");
        assert_eq!(p.romanise("चोगे गाउँ"), "Choge Gaun");
    }

    #[test]
    fn rendering_uses_the_canonical_spelling() {
        let n = Namer::load(&root(), "np", &[]).expect("lexicon loads");
        assert_eq!(n.render("Lahachok"), "Lahachowk");
        assert_eq!(n.render("Laha Chok"), "Laha Chowk");
        assert_eq!(n.render("Lahan Bajar"), "Lahan Bazar");
        assert_eq!(n.render("Chok"), "Chowk");
        // A stem under three letters, or no variant: unchanged.
        assert_eq!(n.render("Achok"), "Achok");
        assert_eq!(n.render("Pokhara"), "Pokhara");
        assert_eq!(n.render("Lahachowk"), "Lahachowk");
        // A different word is kept; an admin suffix is respelled only as a word of its own.
        assert_eq!(n.render("Rampura"), "Rampura");
        assert_eq!(n.render("Janki Tola"), "Janki Tola");
        assert_eq!(n.render("Bhimwada"), "Bhimwada");
        assert_eq!(n.render("Wada 3"), "Ward 3");
    }

    #[test]
    fn the_majority_of_latin_names_wins_then_romanisation() {
        let n = Namer::load(&root(), "np", &["ne-Deva".into()]).expect("namer loads");
        let fits = |_: &str| true;
        // Devanagari first by priority, but a Latin name exists: the Latin name wins.
        let c = n
            .choose(&[vec!["लहाचोक", "Lahachok"], vec!["Lahachowk"]], fits)
            .expect("a name");
        assert_eq!((c.text.as_str(), c.how), ("Lahachowk", How::Latin));
        assert!(c.rendered);
        // Two spellings: the one more records use, ties by source order.
        let c = n
            .choose(
                &[vec!["Kewalpur"], vec!["Kevalpur"], vec!["Kevalpur"]],
                fits,
            )
            .expect("a name");
        assert_eq!((c.text.as_str(), c.how), ("Kevalpur", How::Voted));
        let c = n
            .choose(&[vec!["Kewalpur"], vec!["Kevalpur"]], fits)
            .expect("a name");
        assert_eq!(c.text, "Kewalpur");
        // Spaces do not split a vote; the first spelling is shown.
        let c = n
            .choose(&[vec!["Laha Chowk"], vec!["Lahachowk"]], fits)
            .expect("a name");
        assert_eq!((c.text.as_str(), c.how), ("Laha Chowk", How::Latin));
        // Only Devanagari: romanised.
        let c = n.choose(&[vec!["बागलुङ"]], fits).expect("a name");
        assert_eq!((c.text.as_str(), c.how), ("Baglung", How::Romanised));
        // Diacritics: the plain spelling wins a tie and accents are dropped.
        let c = n
            .choose(&[vec!["Ghā̃sā"], vec!["Ghansa"]], fits)
            .expect("a name");
        assert_eq!(c.text, "Ghansa");
        let c = n.choose(&[vec!["Ghā̃sā"]], fits).expect("a name");
        assert_eq!((c.text.as_str(), c.rendered), ("Ghasa", false));
        let c = n
            .choose(&[vec!["Tamang \u{200D}Chok"]], fits)
            .expect("a name");
        assert_eq!((c.text.as_str(), c.rendered), ("Tamang Chowk", true));
        // Two names in one do not vote; whitespace collapses.
        let c = n
            .choose(&[vec!["Amjilosa / Amjilesah", "Amjilosa"]], fits)
            .expect("a name");
        assert_eq!(c.text, "Amjilosa");
        assert_eq!(n.render("Parsa  Bajar "), "Parsa Bazar");
        // Nothing usable: none (the caller falls back).
        assert_eq!(n.choose(&[vec!["得订"]], fits), None);
        assert_eq!(n.choose(&[vec!["Lahachowk"]], |_| false), None);
    }
}
