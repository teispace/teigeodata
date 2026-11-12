//! Tier 1 of the names chain (nepali-names §5.1, D-232): names from an official list joined by
//! the list's own code. `data/countries/<cc>/official-names.yaml` (P16: countries are data)
//! names each list (a pinned source, its code and name columns, the name's language) and a
//! **crosswalk** from each unit's key in the build to the list's code, reviewed once and checked
//! in, because the boundary source's codes are not the list's (COD-AB's `NP0101401` is CBS
//! `10101`). A unit renamed after the list was made is left out of the crosswalk: the list's
//! name is the old one.
//!
//! Each joined unit gains the list's name, its unit-type word cut (`strip`), typed `official`
//! and marked [`Origin::Official`], placed right after the canonical name so a reader asking for
//! the language takes it first (D-227). A name the unit already has with the same text is
//! moved there and re-marked instead. Refused: a crosswalk key that is no place of the build, a
//! unit no longer named as when it was joined (renamed: the list's name may be the old one), a
//! code the list does not have, a code joined twice.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::File as FsFile;
use std::io::{BufRead, BufReader};
use std::path::Path;

use teigeodata_format::names::NameType;
use unicode_normalization::UnicodeNormalization;

use super::{Origin, Place, PlaceName};
use crate::lock::Lock;

const SCHEMA: &str = "teigeodata/official-names/v1";

/// The alignment gate: a joined row is far when its romanised name differs from its own row's
/// Latin name in more than [`FAR`] of the longer one's letters, and a list fails with more than
/// [`MAX_FAR`] of its joined rows far. A column shifted against its neighbours makes most rows
/// far (CBS's district file: 35 of 77, from row 43); real differences are few (its local
/// levels: 3 of 753, such as K.I. Singh, के.आई.सिं.). Measured 2026-11-12, D-232.
pub(super) const FAR: f64 = 0.45;
const MAX_FAR: f64 = 0.02;

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct OfficialFile {
    schema: String,
    lists: Vec<List>,
}

/// One official list.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    /// The pinned source (a CSV with a header row).
    source: String,
    /// Its code column.
    code: String,
    /// Its name column.
    name: String,
    /// The names' language (BCP 47 primary subtag).
    lang: String,
    /// Unit-type words cut from the end of a name, the source's own spellings included.
    #[serde(default)]
    strip: Vec<String>,
    /// The list's Latin column, when it has one: the alignment gate romanises each joined
    /// name and compares it with its own row's Latin name ([`MAX_FAR`]).
    #[serde(default)]
    latin: Option<String>,
    /// Unit-type words cut from the end of a Latin name before that comparison.
    #[serde(default)]
    latin_strip: Vec<String>,
    /// Each unit's key in the build, with its code in the list and its name in the boundary
    /// source when the crosswalk was made.
    units: BTreeMap<String, Join>,
}

/// One crosswalk entry.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Join {
    code: String,
    /// The unit's name when joined: a unit no longer named so was renamed, and the list's name
    /// may be the old one.
    unit: String,
    /// The list's own text for this unit when the list's row for `code` carries another
    /// unit's name (a misaligned column): it must occur in the list's name column, so it is
    /// still the list's spelling, and the alignment gate checks it against the row's Latin
    /// name.
    #[serde(default)]
    name: Option<String>,
    /// A spelling preferred over the list's, with the evidence for it (the unit's own usage, the
    /// more common one; D-233). The list's spelling stays as an alternate, still searchable.
    #[serde(default)]
    prefer: Option<Prefer>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Prefer {
    name: String,
    /// Where the preferred spelling is used, so a reviewer can check it.
    source: String,
}

fn read_file(root: &Path, cc: &str) -> Result<Option<OfficialFile>, String> {
    let path = root
        .join("data/countries")
        .join(cc)
        .join("official-names.yaml");
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let f: OfficialFile =
        serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if f.schema != SCHEMA {
        return Err(format!("{}: schema is not {SCHEMA}", path.display()));
    }
    Ok(Some(f))
}

/// The sources the country's official lists read (for the licence gate and `make`'s fetch).
pub(crate) fn sources(root: &Path, cc: &str) -> Result<Vec<String>, String> {
    Ok(read_file(root, cc)?
        .map(|f| f.lists.into_iter().map(|l| l.source).collect())
        .unwrap_or_default())
}

/// The rows of a CSV with a header row and no quoted fields, as the named columns' values,
/// trimmed and NFC.
pub(crate) fn csv_columns(path: &Path, cols: &[&str]) -> Result<Vec<Vec<String>>, String> {
    let file = FsFile::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = BufReader::new(file).lines();
    let header = lines
        .next()
        .ok_or("empty csv")?
        .map_err(|e| e.to_string())?;
    let header: Vec<String> = header
        .trim_start_matches('\u{FEFF}')
        .split(',')
        .map(|h| h.trim().nfc().collect())
        .collect();
    let at: Vec<usize> = cols
        .iter()
        .map(|c| {
            let c: String = c.nfc().collect();
            header
                .iter()
                .position(|h| *h == c)
                .ok_or_else(|| format!("{}: no column {c:?}", path.display()))
        })
        .collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        if line.contains('"') {
            return Err(format!(
                "{}: quoted CSV fields are not read",
                path.display()
            ));
        }
        let f: Vec<&str> = line.split(',').map(str::trim).collect();
        out.push(
            at.iter()
                .map(|&i| f.get(i).copied().unwrap_or("").nfc().collect())
                .collect(),
        );
    }
    Ok(out)
}

/// `text` with the longest trailing word of `strip` cut, once (so `उपमहानगरपालिका` is cut
/// whole, never as `नगरपालिका`), never to nothing.
pub(super) fn cut<'t>(text: &'t str, strip: &[String]) -> &'t str {
    strip
        .iter()
        .filter(|w| text.ends_with(w.as_str()))
        .max_by_key(|w| w.len())
        .and_then(|w| text.strip_suffix(w.as_str()))
        .map(str::trim_end)
        .filter(|t| !t.is_empty())
        .unwrap_or(text)
}

/// Gives each joined unit its official name; returns the summary's lines. `allowed` is the
/// lists' sources the build may read.
pub(super) fn run(
    root: &Path,
    lock: &Lock,
    (cc, allowed, romanise): (&str, &[String], &[String]),
    places: &mut [Place],
) -> Result<String, String> {
    let Some(f) = read_file(root, cc)? else {
        return Ok(String::new());
    };
    let namer = crate::names::Namer::load(root, cc, romanise)?;
    let row_of: BTreeMap<&str, usize> = places
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            std::iter::once(p.key.as_str())
                .chain(p.sources.iter().map(String::as_str))
                .map(move |k| (k, i))
        })
        .collect();
    let mut adds: Vec<(usize, PlaceName)> = Vec::new();
    let mut out = String::new();
    for l in &f.lists {
        if !allowed.contains(&l.source) {
            let _ = writeln!(
                out,
                "Official names from {}: not read in this build.",
                l.source
            );
            continue;
        }
        let latin = l.latin.as_deref().unwrap_or(&l.name);
        let rows = csv_columns(&lock.path(root, &l.source)?, &[&l.code, &l.name, latin])?;
        let by_code: BTreeMap<&str, (&str, &str)> = rows
            .iter()
            .filter_map(|r| match r.as_slice() {
                [c, n, la] if !c.is_empty() && !n.is_empty() => {
                    Some((c.as_str(), (n.as_str(), la.as_str())))
                }
                _ => None,
            })
            .collect();
        let mut seen = BTreeSet::new();
        let listed: BTreeSet<&str> = by_code.values().map(|v| v.0).collect();
        for (
            key,
            Join {
                code,
                unit,
                name: own,
                prefer,
            },
        ) in &l.units
        {
            let &i = row_of
                .get(key.as_str())
                .ok_or_else(|| format!("{}: crosswalk key {key} is no place", l.source))?;
            if !places
                .get(i)
                .is_some_and(|p| p.names.iter().any(|n| n.text == *unit))
            {
                return Err(format!(
                    "{}: {key} is no longer named {unit:?}; review the crosswalk",
                    l.source
                ));
            }
            let &(row, _) = by_code
                .get(code.as_str())
                .ok_or_else(|| format!("{}: no code {code} (for {key})", l.source))?;
            let name = listed_name(row, own.as_deref(), &listed)
                .map_err(|e| format!("{}: {key}: {e}", l.source))?;
            if !seen.insert(code.as_str()) {
                return Err(format!("{}: code {code} joined twice", l.source));
            }
            let unit_names = joined(cut(name, &l.strip), prefer.as_ref(), &l.lang)
                .map_err(|e| format!("{}: {key}: {e}", l.source))?;
            adds.extend(unit_names.into_iter().map(|n| (i, n)));
        }
        let far = if l.latin.is_some() {
            aligned(l, &by_code, &namer)?
        } else {
            String::new()
        };
        let _ = writeln!(
            out,
            "Official names (D-232) from {}: {} units joined by code, of {} in the list{far}.",
            l.source,
            l.units.len(),
            by_code.len()
        );
    }
    let same = same_named(places, &adds);
    let shared = same.len();
    let (added, moved) = apply(places, adds.into_iter().chain(same).collect());
    let _ = writeln!(
        out,
        "Official names placed after the canonical one: {added} new, {moved} already there and \
         re-marked; {shared} of them on places named as one of their own units (a town and its \
         municipality: the same name, the same spelling)."
    );
    Ok(out)
}

/// The list's name for a unit: its row's, or the entry's own when given, which must be one of
/// the list's names (`listed`), so a misaligned row is mended with the list's text only.
fn listed_name<'a>(
    row: &'a str,
    own: Option<&'a str>,
    listed: &BTreeSet<&str>,
) -> Result<&'a str, String> {
    match own {
        None => Ok(row),
        Some(n) if listed.contains(n) => Ok(n),
        Some(n) => Err(format!("the name {n:?} is not in the list's name column")),
    }
}

/// A joined unit's names in the order `apply` needs: each is inserted right after the canonical
/// name, so the last pushed ends up first. With a preferred spelling the list's own becomes an
/// alternate behind it; the preference must differ from the list and cite a source.
fn joined(listed: &str, prefer: Option<&Prefer>, lang: &str) -> Result<Vec<PlaceName>, String> {
    let named = |text: &str, kind| PlaceName {
        text: text.to_string(),
        lang: Some(lang.to_string()),
        kind: Some(kind),
        origin: Origin::Official,
    };
    match prefer {
        None => Ok(vec![named(listed, NameType::Official)]),
        Some(Prefer { name, source }) => {
            if name.trim().is_empty() || source.trim().is_empty() || name == listed {
                return Err(format!(
                    "the preferred name must differ from the list's ({listed}) and cite a source"
                ));
            }
            Ok(vec![
                named(listed, NameType::Alt),
                named(name, NameType::Official),
            ])
        }
    }
}

/// The deepest unit above each place that gains an official name and has the place's own
/// canonical name (Jiri the town in Jiri the municipality): the place takes that name too, so
/// a town is not left with a machine spelling of its own unit's official one.
fn same_named(places: &[Place], adds: &[(usize, PlaceName)]) -> Vec<(usize, PlaceName)> {
    let official: BTreeMap<usize, &PlaceName> = adds
        .iter()
        .filter(|(_, n)| n.kind == Some(NameType::Official))
        .map(|(i, n)| (*i, n))
        .collect();
    let by_key: BTreeMap<&str, usize> = places
        .iter()
        .enumerate()
        .map(|(i, p)| (p.key.as_str(), i))
        .collect();
    let canonical = |p: &Place| p.names.first().map(|n| n.text.to_lowercase());
    let mut out = Vec::new();
    for (i, p) in places.iter().enumerate() {
        if official.contains_key(&i) {
            continue;
        }
        let Some(own) = canonical(p) else { continue };
        let mut up = p.parent.as_deref();
        // Units nest a few levels deep; the bound stops a cycle a bad source might make.
        for _ in 0..8 {
            let Some(&a) = up.and_then(|k| by_key.get(k)) else {
                break;
            };
            let Some(unit) = places.get(a) else { break };
            if canonical(unit).as_deref() == Some(own.as_str())
                && let Some(n) = official.get(&a)
            {
                out.push((i, (*n).clone()));
                break;
            }
            up = unit.parent.as_deref();
        }
    }
    out
}

/// Letters only, lower case, accents dropped: what the alignment gate compares.
pub(super) fn letters(s: &str) -> Vec<char> {
    teigeodata_names::strip_accents(s)
        .chars()
        .filter(char::is_ascii_alphabetic)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The alignment gate on list `l`'s joined rows: refused past [`MAX_FAR`]; else the summary's
/// note of the far rows.
fn aligned(
    l: &List,
    by_code: &BTreeMap<&str, (&str, &str)>,
    namer: &crate::names::Namer,
) -> Result<String, String> {
    let mut far = Vec::new();
    for j in l.units.values() {
        let Some(&(row, latin)) = by_code.get(j.code.as_str()) else {
            continue;
        };
        let name = j.name.as_deref().unwrap_or(row);
        let lower = latin.to_lowercase();
        let want = letters(cut(&lower, &l.latin_strip));
        let got = letters(&namer.romanise(cut(name, &l.strip)).unwrap_or_default());
        let d = crate::xlit::levenshtein(&want, &got);
        #[allow(
            clippy::cast_precision_loss,
            reason = "names are far shorter than 2^52 letters"
        )]
        let share = d as f64 / want.len().max(got.len()).max(1) as f64;
        if share > FAR {
            far.push(format!("{} {latin} = {name}", j.code));
        }
    }
    #[allow(clippy::cast_precision_loss, reason = "counts below 2^52")]
    let rate = far.len() as f64 / l.units.len().max(1) as f64;
    if rate > MAX_FAR {
        return Err(format!(
            "{}: {} of {} joined names romanise far from their own row's Latin name (a shifted \
             column?): {}",
            l.source,
            far.len(),
            l.units.len(),
            far.iter().take(5).cloned().collect::<Vec<_>>().join("; ")
        ));
    }
    Ok(format!(
        " ({} romanise far from their row's Latin name: {})",
        far.len(),
        if far.is_empty() {
            "none".into()
        } else {
            far.join("; ")
        }
    ))
}

/// Places each name right after its place's canonical name, moving an equal text there.
/// Returns (added, moved).
fn apply(places: &mut [Place], adds: Vec<(usize, PlaceName)>) -> (usize, usize) {
    let (mut added, mut moved) = (0, 0);
    for (i, name) in adds {
        let Some(p) = places.get_mut(i) else { continue };
        if p.names.first().is_some_and(|c| c.text == name.text) {
            if let Some(c) = p.names.first_mut() {
                c.lang = name.lang;
                c.origin = Origin::Official;
            }
            moved += 1;
            continue;
        }
        if !super::pack::fits(&name.text) {
            continue;
        }
        if let Some(at) = p.names.iter().skip(1).position(|n| n.text == name.text) {
            p.names.remove(at + 1);
            moved += 1;
        } else {
            added += 1;
        }
        let at = p.names.len().min(1);
        p.names.insert(at, name);
    }
    (added, moved)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests {
    use super::{Origin, Place, PlaceName, Prefer, apply, cut, joined, listed_name, same_named};
    use teigeodata_format::names::NameType;

    fn unit(names: &[(&str, Option<&str>, Origin)]) -> Place {
        Place {
            key: "admin:X1".into(),
            kind: teigeodata_core::PlaceKind::Admin,
            admin_level: 3,
            flags: 0,
            point: (0, 0),
            names: names
                .iter()
                .map(|(t, l, o)| PlaceName {
                    text: (*t).into(),
                    lang: l.map(Into::into),
                    kind: None,
                    origin: *o,
                })
                .collect(),
            population: None,
            rank: 0,
            parent: None,
            sources: vec!["admin:X1".into()],
        }
    }

    fn official(text: &str) -> PlaceName {
        PlaceName {
            text: text.into(),
            lang: Some("ne".into()),
            kind: Some(NameType::Official),
            origin: Origin::Official,
        }
    }

    #[test]
    fn the_type_word_is_cut_once_and_never_to_nothing() {
        let strip = vec![
            "गाउँपालिका".to_string(),
            "नगरपालिका".to_string(),
            "उपमहानगरपालिका".to_string(),
        ];
        assert_eq!(cut("फक्ताङ्लुङ्ग गाउँपालिका", &strip), "फक्ताङ्लुङ्ग");
        assert_eq!(cut("जनकपुर उपमहानगरपालिका", &strip), "जनकपुर");
        assert_eq!(cut("नगरपालिका", &strip), "नगरपालिका");
        assert_eq!(cut("ईलाम", &strip), "ईलाम");
    }

    #[test]
    fn an_official_name_comes_first_after_the_canonical_and_replaces_its_copy() {
        let mut ps = vec![
            unit(&[
                ("Ilam", None, Origin::Official),
                ("इलाम", Some("ne"), Origin::Transliteration),
                ("ईलाम", Some("ne"), Origin::Wikidata),
            ]),
            unit(&[("Pokhara", None, Origin::Official)]),
        ];
        let (added, moved) = apply(&mut ps, vec![(0, official("ईलाम")), (1, official("पोखरा"))]);
        assert_eq!((added, moved), (1, 1));
        let texts: Vec<_> = ps[0]
            .names
            .iter()
            .map(|n| (n.text.as_str(), n.origin))
            .collect();
        assert_eq!(
            texts,
            [
                ("Ilam", Origin::Official),
                ("ईलाम", Origin::Official),
                ("इलाम", Origin::Transliteration)
            ]
        );
        assert_eq!(ps[1].names[1].text, "पोखरा");
    }

    #[test]
    fn a_mended_row_takes_only_the_list_s_own_text() {
        let listed = std::collections::BTreeSet::from(["दाङ", "रुपन्देही"]);
        assert_eq!(listed_name("रुपन्देही", None, &listed), Ok("रुपन्देही"));
        assert_eq!(listed_name("रुपन्देही", Some("दाङ"), &listed), Ok("दाङ"));
        assert!(listed_name("रुपन्देही", Some("दांग"), &listed).is_err());
    }

    #[test]
    fn a_preferred_spelling_goes_first_and_the_list_s_stays_as_an_alternate() {
        let mut ps = vec![unit(&[
            ("Biratnagar", None, Origin::Official),
            ("विराटनगर", Some("ne"), Origin::Wikidata),
        ])];
        let prefer = |name: &str, source: &str| Prefer {
            name: name.into(),
            source: source.into(),
        };
        assert!(joined("बिराटनगर", Some(&prefer("बिराटनगर", "x")), "ne").is_err());
        assert!(joined("बिराटनगर", Some(&prefer("विराटनगर", " ")), "ne").is_err());
        let adds: Vec<_> = joined("बिराटनगर", Some(&prefer("विराटनगर", "the city")), "ne")
            .unwrap()
            .into_iter()
            .map(|n| (0, n))
            .collect();
        // A same-named town gets the preferred spelling only, never the alternate.
        let mut town = unit(&[("Biratnagar", None, Origin::GeoNames)]);
        town.key = "geonames:1".into();
        town.parent = Some("admin:X1".into());
        ps.push(town);
        let same = same_named(&ps, &adds);
        assert_eq!(same.len(), 1);
        assert_eq!(same[0].1.text, "विराटनगर");
        apply(&mut ps, adds.into_iter().chain(same).collect());
        let got: Vec<_> = ps[0]
            .names
            .iter()
            .map(|n| (n.text.as_str(), n.kind))
            .collect();
        assert_eq!(
            got,
            [
                ("Biratnagar", None),
                ("विराटनगर", Some(NameType::Official)),
                ("बिराटनगर", Some(NameType::Alt)),
            ]
        );
        assert_eq!(ps[1].names[1].text, "विराटनगर");
    }

    #[test]
    fn a_town_named_as_its_unit_takes_the_unit_s_official_name() {
        let mut town = unit(&[("Jiri", None, Origin::GeoNames)]);
        town.key = "geonames:1".into();
        town.kind = teigeodata_core::PlaceKind::Settlement;
        town.parent = Some("admin:X1".into());
        let mut other = town.clone();
        other.key = "geonames:2".into();
        other.names[0].text = "Jiri Bazar".into();
        let ps = vec![unit(&[("Jiri", None, Origin::Official)]), town, other];
        let same = same_named(&ps, &[(0, official("जिरी"))]);
        assert_eq!(same.len(), 1);
        assert_eq!((same[0].0, same[0].1.text.as_str()), (1, "जिरी"));
    }
}
