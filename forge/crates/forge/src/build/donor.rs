//! Tier 4 of the native-names chain (nepali-names §5.1, D-236): a place with no name in a
//! target language and script takes a related language's label in that script, a name a person
//! wrote, before the transliterator writes one (tier 6). Countries name their donors by data
//! (`name_donors` in `profile.yaml`, P16), in order of preference.
//!
//! A label is cut of the profile's type words (नगरपालिका, जिल्ला) and taken only when it
//! romanises close to the place's canonical Latin name, by the same measure as the official
//! names' alignment gate (D-232). It is typed `alt` and marked `donor` (D-227): never the
//! canonical name. Each donor's agreement with the names people wrote in the target language,
//! on the places that have both, is measured on every build and gated (G-donor): a donor whose
//! labels mostly differ from the target's spelling is refused.

use std::fmt::Write as _;
use std::path::Path;

use teigeodata_core::PlaceKind;
use teigeodata_format::names::NameType;
use teigeodata_names::{fold, script_code};

use super::official::{FAR, cut, letters};
use super::{Origin, Place, PlaceName};

/// The least share of a donor's labels, on places that have both, whose fold is the target's
/// own name; a donor below it is refused. A person's label is taken only where it beats the
/// machine: the transliterator writes Nepal's official names exactly 45.5% of the time (G-xlit,
/// `cbs` set), so a donor must agree more often than half. Measured 2026-11-12 (nepali-names
/// §5.8): Sanskrit 77%, Awadhi 75%, Doteli 64% pass; Maithili 46%, Hindi 23%, Bhojpuri 25% fail.
pub(crate) const MIN_AGREE: f64 = 0.50;

/// The fewest places with both names for the agreement to be judged.
const MIN_PAIRS: usize = 30;

/// One target of the profile: `to` in `script`, from the donor languages `from`, in order.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub(crate) struct Donors {
    pub(crate) to: String,
    pub(crate) script: String,
    pub(crate) from: Vec<String>,
    /// The donors' own type words (जिला, मण्डलम्), cut like the profile's.
    #[serde(default)]
    pub(crate) strip: Vec<String>,
    /// The donors' spelling of a word's end and the target's (`ंग` as `ङ`): the
    /// donor-to-target spelling pass (nepali-names §5.2).
    #[serde(default)]
    pub(crate) word_ends: Vec<WordEnd>,
}

/// A word ending a donor writes as `text` and the target as `means`.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub(crate) struct WordEnd {
    pub(crate) text: String,
    pub(crate) means: String,
}

/// `text` with each word's ending respelled by the first rule of `ends` it matches.
fn respell(text: &str, ends: &[WordEnd]) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, w) in text.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        match ends.iter().find_map(|e| {
            w.strip_suffix(e.text.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| (s, e))
        }) {
            Some((stem, e)) => {
                out.push_str(stem);
                out.push_str(&e.means);
            }
            None => out.push_str(w),
        }
    }
    out
}

/// `text` without a trailing bracketed qualifier (`महोत्तरी(जनकपुर)`) and then a type word of
/// `words`, longest first.
fn bare<'t>(text: &'t str, words: &[String]) -> &'t str {
    let t = text.trim_end();
    let t = t
        .strip_suffix(')')
        .and_then(|r| r.rfind('(').map(|i| r[..i].trim_end()))
        .filter(|r| !r.is_empty())
        .unwrap_or(t);
    cut(t, words)
}

/// What a donor's labels did: places with both names, how many agree and how many are one
/// edit away; names given.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Tally {
    pairs: usize,
    /// The donor's label folds to the target's name.
    agree: usize,
    /// Or is one edit from it, a spelling variant (ओखलढुंगा, ओखलढुङगा).
    near: usize,
    given: usize,
    /// The first few places where the donor differs: (the target's name, the donor's).
    differ: Vec<(String, String)>,
}

/// G-donor: a donor judged on enough places that writes the target's name no more often than
/// [`MIN_AGREE`].
fn refused_by_gate(t: &Tally) -> bool {
    #[allow(clippy::cast_precision_loss, reason = "counts below 2^52")]
    let share = t.agree as f64 / t.pairs.max(1) as f64;
    t.pairs >= MIN_PAIRS && share < MIN_AGREE
}

/// How many differing pairs a tally keeps for the report.
const EXAMPLES: usize = 5;

/// Adds the donor names; returns the summary's line.
pub(super) fn run(
    root: &Path,
    (cc, romanise): (&str, &[String]),
    rules: &[Donors],
    type_words: &[String],
    places: &mut [Place],
) -> Result<String, String> {
    if rules.is_empty() {
        return Ok(String::new());
    }
    let namer = crate::names::Namer::load(root, cc, romanise)?;
    let mut out = String::new();
    for r in rules {
        let mut words: Vec<String> = type_words.iter().chain(&r.strip).cloned().collect();
        words.sort_by_key(|w| std::cmp::Reverse(w.len()));
        words.dedup();
        let (tallies, far) = fill(places, r, &words, |n| namer.romanise(n));
        let mut parts = Vec::new();
        let mut refused = Vec::new();
        for (lang, t) in r.from.iter().zip(&tallies) {
            #[allow(clippy::cast_precision_loss, reason = "counts below 2^52")]
            let (share, near) = (
                t.agree as f64 / t.pairs.max(1) as f64,
                t.near as f64 / t.pairs.max(1) as f64,
            );
            parts.push(format!(
                "{lang} {} given, of {} places with both {:.0}% the same and {:.0}% within one \
                 edit",
                t.given,
                t.pairs,
                share * 100.0,
                near * 100.0
            ));
            if refused_by_gate(t) {
                let eg: Vec<String> = t.differ.iter().map(|(a, b)| format!("{a} / {b}")).collect();
                refused.push(format!(
                    "{lang} ({:.0}%; e.g. {})",
                    share * 100.0,
                    eg.join(", ")
                ));
            }
        }
        if !refused.is_empty() {
            return Err(format!(
                "G-donor: these donors of `{}` write its own names on fewer than {:.0}% of the \
                 places with both, no better than the transliterator: {}; drop them from \
                 `name_donors`",
                r.to,
                MIN_AGREE * 100.0,
                refused.join(", ")
            ));
        }
        let _ = writeln!(
            out,
            "Donor names (D-236, `{}` in {}): {}; {far} labels refused, romanised far from the \
             Latin name.",
            r.to,
            r.script,
            parts.join("; ")
        );
    }
    Ok(out)
}

/// For each place (timezone areas left out): with a name in `r.to` and `r.script`, tallies each
/// donor's agreement with it; without one, takes the first donor label (in `r.from` order) that
/// romanises within [`FAR`] of the canonical Latin name. Returns the tallies, in `r.from`
/// order, and the labels refused as far.
fn fill(
    places: &mut [Place],
    r: &Donors,
    type_words: &[String],
    romanise: impl Fn(&str) -> Option<String>,
) -> (Vec<Tally>, usize) {
    let mut tallies = vec![Tally::default(); r.from.len()];
    let mut far = 0;
    let in_script = |n: &PlaceName| script_code(&n.text) == Some(r.script.as_str());
    for p in places.iter_mut() {
        if p.kind == PlaceKind::TimezoneArea {
            continue;
        }
        let own = p.names.iter().find(|n| {
            n.lang.as_deref() == Some(r.to.as_str()) && in_script(n) && !n.origin.is_machine()
        });
        if let Some(own) = own {
            let own = fold(bare(&own.text, type_words));
            for (lang, t) in r.from.iter().zip(&mut tallies) {
                if let Some(d) = p
                    .names
                    .iter()
                    .find(|n| n.lang.as_deref() == Some(lang.as_str()) && in_script(n))
                {
                    t.pairs += 1;
                    let theirs = fold(&respell(bare(&d.text, type_words), &r.word_ends));
                    if theirs == own {
                        t.agree += 1;
                        t.near += 1;
                    } else if crate::xlit::levenshtein(
                        &own.chars().collect::<Vec<_>>(),
                        &theirs.chars().collect::<Vec<_>>(),
                    ) <= 1
                    {
                        t.near += 1;
                    } else if t.differ.len() < EXAMPLES {
                        t.differ.push((own.clone(), d.text.clone()));
                    }
                }
            }
            continue;
        }
        if p.names
            .iter()
            .any(|n| n.lang.as_deref() == Some(r.to.as_str()) && in_script(n))
        {
            continue;
        }
        let Some(latin) = p
            .names
            .first()
            .filter(|n| script_code(&n.text) == Some("Latn"))
        else {
            continue;
        };
        let want = letters(&latin.text.to_lowercase());
        let mut given = None;
        'donors: for (i, lang) in r.from.iter().enumerate() {
            for n in p
                .names
                .iter()
                .filter(|n| n.lang.as_deref() == Some(lang.as_str()) && in_script(n))
            {
                let text = respell(bare(&n.text, type_words), &r.word_ends);
                let got = letters(&romanise(&text).unwrap_or_default());
                let d = crate::xlit::levenshtein(&want, &got);
                #[allow(clippy::cast_precision_loss, reason = "names are short")]
                let share = d as f64 / want.len().max(got.len()).max(1) as f64;
                if share > FAR || !super::pack::fits(&text) {
                    far += 1;
                    continue;
                }
                given = Some((i, text));
                break 'donors;
            }
        }
        if let Some((i, text)) = given {
            if let Some(t) = tallies.get_mut(i) {
                t.given += 1;
            }
            p.names.push(PlaceName {
                text,
                lang: Some(r.to.clone()),
                kind: Some(NameType::Alt),
                origin: Origin::Donor,
            });
        }
    }
    (tallies, far)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]
mod tests {
    use super::{Donors, Tally, fill};
    use crate::build::{Origin, Place, PlaceName};
    use teigeodata_core::PlaceKind;
    use teigeodata_format::names::NameType;

    fn place(names: &[(&str, Option<&str>, Origin)]) -> Place {
        Place {
            key: names[0].0.into(),
            kind: PlaceKind::Settlement,
            admin_level: 0,
            flags: 0,
            point: (0, 0),
            names: names
                .iter()
                .map(|(t, l, o)| PlaceName {
                    text: (*t).into(),
                    lang: l.map(str::to_owned),
                    kind: None,
                    origin: *o,
                })
                .collect(),
            population: None,
            rank: 0,
            parent: None,
            sources: Vec::new(),
        }
    }

    fn rule() -> Donors {
        Donors {
            to: "ne".into(),
            script: "Deva".into(),
            from: vec!["mai".into(), "awa".into()],
            strip: vec![],
            word_ends: vec![super::WordEnd {
                text: "ंग".into(),
                means: "ङ".into(),
            }],
        }
    }

    /// A toy romaniser: the few letters the tests use.
    fn roman(n: &str) -> String {
        let map = [
            ('ज', "ja"),
            ('न', "na"),
            ('क', "ka"),
            ('प', "pa"),
            ('र', "ra"),
            ('ु', "u"),
            ('ल', "la"),
            ('ं', "n"),
            ('ड', "da"),
        ];
        let mut s = String::new();
        for c in n.chars() {
            if let Some((_, r)) = map.iter().find(|(k, _)| *k == c) {
                s.push_str(r);
            }
        }
        // "जनकपुर" → "janakapura": close to "janakpur", as a real romaniser's output is.
        s
    }

    /// The first donor label close to the Latin name is given, cut of its type word, typed alt
    /// and marked donor; a far label and a place with its own name gain none.
    #[test]
    fn a_close_donor_label_fills_a_gap_and_a_far_one_does_not() {
        let words = vec!["उपमहानगरपालिका".to_owned(), "नगरपालिका".to_owned()];
        let mut ps = vec![
            place(&[
                ("Janakpur", None, Origin::GeoNames),
                ("जनकपुर उपमहानगरपालिका", Some("mai"), Origin::Wikidata),
                ("जनकपुर", Some("awa"), Origin::Wikidata),
            ]),
            place(&[
                ("Lalbandi", None, Origin::GeoNames),
                ("डलकल", Some("mai"), Origin::Wikidata),
            ]),
            place(&[
                ("Janakpur", None, Origin::GeoNames),
                ("जनकपुर", Some("ne"), Origin::Wikidata),
                ("जनकपुर नगरपालिका", Some("mai"), Origin::Wikidata),
                ("जनकपुरा", Some("awa"), Origin::Wikidata),
            ]),
        ];
        let (t, far) = fill(&mut ps, &rule(), &words, |n| Some(roman(n)));
        let given = ps[0].names.last().unwrap();
        assert_eq!(given.text, "जनकपुर");
        assert_eq!(given.lang.as_deref(), Some("ne"));
        assert_eq!(given.kind, Some(NameType::Alt));
        assert_eq!(given.origin, Origin::Donor);
        assert_eq!(ps[1].names.len(), 2, "a far label is not given");
        assert_eq!(far, 1);
        assert_eq!(ps[2].names.len(), 4, "a place with its own name gains none");
        assert_eq!(
            t,
            vec![
                Tally {
                    pairs: 1,
                    agree: 1,
                    near: 1,
                    given: 1,
                    differ: vec![]
                },
                Tally {
                    pairs: 1,
                    agree: 0,
                    near: 1,
                    given: 0,
                    differ: vec![]
                },
            ]
        );
    }

    /// G-donor refuses a donor judged on enough places below the floor, and only such a one.
    #[test]
    fn the_gate_refuses_a_donor_that_disagrees() {
        let t = |pairs, agree| Tally {
            pairs,
            agree,
            ..Tally::default()
        };
        assert!(
            super::refused_by_gate(&t(100, 46)),
            "Maithili's 46% is refused"
        );
        assert!(!super::refused_by_gate(&t(100, 64)), "Doteli's 64% passes");
        assert!(
            !super::refused_by_gate(&t(14, 5)),
            "too few places to judge"
        );
    }

    /// A bracketed qualifier, then a type word, is cut; never to nothing.
    #[test]
    fn a_qualifier_and_a_type_word_are_cut() {
        let w = vec!["जिला".to_owned()];
        assert_eq!(super::bare("महोत्तरी(जनकपुर)", &w), "महोत्तरी");
        assert_eq!(super::bare("इलाम जिला", &w), "इलाम");
        assert_eq!(super::bare("(जनकपुर)", &w), "(जनकपुर)");
        assert_eq!(super::bare("जिला", &w), "जिला");
    }

    /// A word's end is respelled, each word on its own, never to nothing.
    #[test]
    fn a_donor_s_word_end_is_respelled() {
        let ends = rule().word_ends;
        assert_eq!(super::respell("लमजुंग", &ends), "लमजुङ");
        assert_eq!(super::respell("धादिंग बेसी", &ends), "धादिङ बेसी");
        assert_eq!(super::respell("मंगल", &ends), "मंगल");
        assert_eq!(super::respell("ंग", &ends), "ंग");
    }

    /// A later donor fills a gap the first has no label for; a machine name in the target is
    /// not counted as the target's own.
    #[test]
    fn a_later_donor_fills_and_a_machine_name_is_not_measured() {
        let mut ps = vec![
            place(&[
                ("Janakpur", None, Origin::GeoNames),
                ("जनकपुर", Some("awa"), Origin::Wikidata),
            ]),
            place(&[
                ("Janakpur", None, Origin::GeoNames),
                ("जनकपुर", Some("ne"), Origin::Transliteration),
                ("जनकपुर", Some("mai"), Origin::Wikidata),
            ]),
        ];
        let (t, _) = fill(&mut ps, &rule(), &[], |n| Some(roman(n)));
        assert_eq!(t[0], Tally::default(), "a machine name is not measured");
        assert_eq!(t[1].given, 1);
        assert_eq!(ps[0].names.last().unwrap().origin, Origin::Donor);
    }
}
