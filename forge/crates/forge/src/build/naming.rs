//! A place's names (D-060, D-107) and display name (D-106).

use super::Origin;
use super::Place;
use super::PlaceName;
use super::pack;
use super::sources::Rec;
use crate::aliases::rules::COLLISION_M;
use crate::aliases::rules::Dropped;
use crate::aliases::rules::Rules;
use crate::names::How;
use crate::names::Namer;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use teigeodata_core::PlaceKind;
use teigeodata_format::names::NameType;
use teigeodata_names::fold;

/// How the clustered places got their display names (names-canonical §5.2).
#[derive(Default)]
pub(super) struct Naming {
    /// By `names::How`: the only Latin name, a majority, a romanisation.
    pub(super) by: BTreeMap<How, usize>,
    /// The first name that fits, for places with neither a Latin name nor a romaniser.
    pub(super) fallback: usize,
    /// Display names the lexicon respelled.
    pub(super) rendered: usize,
    /// Places whose display name is not their first record's name: (place key, that name,
    /// the display name, how), for review.
    pub(super) changed: Vec<(String, String, String, &'static str)>,
    /// Names not taken by the D-060 rules: (place key, record, name, why, what it names).
    pub(super) dropped: BTreeSet<(String, String, String, Dropped, String)>,
    /// Admin units' names whose trailing type word was cut (D-243).
    pub(super) type_words_cut: usize,
}

impl Naming {
    pub(super) fn line(&self) -> String {
        let get = |h| self.by.get(&h).copied().unwrap_or(0);
        format!(
            "Display names: {} the only Latin name, {} a majority of Latin names, {} romanised, {} the first name that fits; {} respelled by the lexicon.\n",
            get(How::Latin),
            get(How::Voted),
            get(How::Romanised),
            self.fallback,
            self.rendered
        ) + &format!(
            "Names of other records (D-060): {} records lent none (no name in common with the place's first), {} names not taken (the primary name of an admin unit, or of another place within {} km).\n",
            self.count(Dropped::Uncorroborated),
            self.count(Dropped::Collision),
            COLLISION_M / 1000.0
        ) + &format!(
            "Names with their context (D-202): {} not kept (another of the place's names, or the country's or a unit's name, after a comma or in brackets).\n",
            self.count(Dropped::Qualified)
        ) + &format!(
            "Type words (D-243): {} admin unit names lost a trailing type word of the profile (\"कैलाली जिल्ला\" is \"कैलाली\").\n",
            self.type_words_cut
        )
    }

    /// Writes the two review files, `naming-<build>.tsv` and `dropped-names-<build>.tsv`.
    pub(super) fn write(&self, dir: &Path, build: &str) -> Result<(), String> {
        for (file, text) in [
            (format!("naming-{build}.tsv"), self.review()),
            (format!("dropped-names-{build}.tsv"), self.dropped()),
        ] {
            let path = dir.join(file);
            fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        Ok(())
    }

    pub(super) fn count(&self, why: Dropped) -> usize {
        self.dropped.iter().filter(|d| d.3 == why).count()
    }

    /// The review file of names not taken, sorted.
    pub(super) fn dropped(&self) -> String {
        let mut out = String::from(
            "# Names of other records a place did not take (D-060).\nplace\trecord\tname\twhy\tnames\n",
        );
        for (k, rec, name, why, with) in &self.dropped {
            let _ = writeln!(
                out,
                "{k}\t{rec}\t{}\t{}\t{}",
                name.replace(['\t', '\n', '\r'], " "),
                why.as_str(),
                with.replace(['\t', '\n', '\r'], " ")
            );
        }
        out
    }

    /// The review file: one line per place whose display name differs from its first record's
    /// name, sorted by place key.
    pub(super) fn review(&self) -> String {
        let mut rows: Vec<&(String, String, String, &str)> = self.changed.iter().collect();
        rows.sort();
        let mut out = String::from(
            "# Display names that differ from the first source's (names-canonical §5.2).\nplace\tsource\tdisplay\thow\n",
        );
        for (k, from, to, how) in rows {
            let _ = writeln!(out, "{k}\t{from}\t{to}\t{how}");
        }
        out
    }
}

/// A clustered place, as the collision rule knows it.
pub(super) struct Here<'a> {
    pub(super) cluster: &'a str,
    pub(super) owner: u32,
    /// Its seed point: the one its primary name was registered at.
    pub(super) seed: (i32, i32),
}

/// A clustered place's names, its display name first (names-canonical §5.2); `None` when no
/// name can be stored. `recs` are its records in source priority with their source index.
pub(super) fn place_names<'a>(
    here: &Here<'_>,
    recs: &[(&'a Rec, &'a str, usize)],
    names: &'a [BTreeMap<String, Vec<PlaceName>>],
    (display, rules): (&Namer, &Rules<'_>),
    naming: &mut Naming,
) -> Option<Vec<PlaceName>> {
    let own_names = |&(r, id, s): &(&'a Rec, &'a str, usize)| {
        let own = names.get(s).and_then(|m| m.get(id));
        std::iter::once(r.name.as_str()).chain(own.into_iter().flatten().map(|n| n.text.as_str()))
    };
    // The anchor: the first record with a name. Others lend their names only when one of
    // them is the anchor's too (D-060).
    let anchor = recs
        .iter()
        .position(|(r, _, _)| !r.name.is_empty())
        .unwrap_or(0);
    let known = recs
        .get(anchor)
        .map(|r| rules.anchor(own_names(r)))
        .unwrap_or_default();
    let mut rest: BTreeSet<PlaceName> = BTreeSet::new();
    // Each record's candidates for the display name: its primary name, then its names
    // with no language or in English that are not historical or alternates.
    let mut candidates: Vec<Vec<&str>> = Vec::with_capacity(recs.len());
    for (i, rec) in recs.iter().enumerate() {
        let &(r, id, s) = rec;
        if i != anchor && !rules.corroborates(&known, own_names(rec)) {
            naming.dropped.insert((
                here.cluster.to_string(),
                id.to_string(),
                r.name.clone(),
                Dropped::Uncorroborated,
                String::new(),
            ));
            continue;
        }
        // A lent name the anchor lacks must not be another place's name.
        let mut take = |t: &str| {
            if i == anchor || Rules::fold_known(&known, t) {
                return true;
            }
            let Some(with) = rules.collides(here.owner, here.seed, t) else {
                return true;
            };
            naming.dropped.insert((
                here.cluster.to_string(),
                id.to_string(),
                t.to_string(),
                Dropped::Collision,
                with,
            ));
            false
        };
        let mut c = Vec::new();
        // The source's own spelling stays a name of the place.
        if !r.name.is_empty() && take(&r.name) {
            c.push(r.name.as_str());
            rest.insert(PlaceName {
                text: r.name.clone(),
                lang: None,
                kind: None,
                origin: super::sources::origin_of(id),
            });
        }
        for n in names.get(s).and_then(|m| m.get(id)).into_iter().flatten() {
            if n.text == r.name || !take(&n.text) {
                continue;
            }
            rest.insert(n.clone());
            if n.lang.as_deref().is_none_or(|l| l == "en")
                && matches!(n.kind, None | Some(NameType::Official))
            {
                c.push(n.text.as_str());
            }
        }
        candidates.push(c);
    }
    let (canonical, origin) = display_name(here, recs, &candidates, &rest, display, naming)?;
    // A name taken as written keeps the origin of the record it came from (D-227).
    let origin = if origin == Origin::Unknown {
        rest.iter()
            .find(|n| n.text == canonical)
            .map_or(Origin::Unknown, |n| n.origin)
    } else {
        origin
    };
    let mut all: Vec<PlaceName> = vec![PlaceName {
        text: canonical.clone(),
        lang: None,
        kind: None,
        origin,
    }];
    for n in rest {
        if !all.iter().any(|m| m.same(&n)) {
            all.push(n);
        }
    }
    Some(all)
}

/// The name before a qualifier: the text before the first `, ` or ` (`, when both parts have
/// text ("Bara District, Nepal" → "Bara District").
fn qualified_head(text: &str) -> Option<&str> {
    let cut = [text.find(", "), text.find(" (")]
        .into_iter()
        .flatten()
        .min()?;
    let (head, tail) = (text.get(..cut)?.trim(), text.get(cut + 2..)?.trim());
    (!head.is_empty() && !tail.is_empty()).then_some(head)
}

/// The country's and the admin units' names, folded: the context a qualifier names.
fn context_names(places: &[Place]) -> BTreeSet<String> {
    places
        .iter()
        .filter(|p| matches!(p.kind, PlaceKind::Country | PlaceKind::Admin))
        .flat_map(|p| &p.names)
        .map(|n| fold(&n.text))
        .filter(|f| !f.is_empty())
        .collect()
}

/// Cuts a trailing type word (`words`: the profile's native names of its admin levels and unit
/// types, longest first) from each name of an admin unit or country that states a language,
/// when a space parts it from the name: Wikidata's "कैलाली जिल्ला" is the district "कैलाली", as
/// the official lists write it (D-243). The canonical name and a name that is only a type word
/// stay; a name the cut makes equal to another of the place's goes. Counted in `naming`.
pub(super) fn cut_unit_type_words(places: &mut [Place], words: &[String], naming: &mut Naming) {
    let cut = |text: &str| -> Option<String> {
        words.iter().find_map(|w| {
            let head = text.strip_suffix(w.as_str())?;
            let bare = head.trim_end();
            (bare.len() < head.len() && !bare.is_empty()).then(|| bare.to_owned())
        })
    };
    for p in places
        .iter_mut()
        .filter(|p| matches!(p.kind, PlaceKind::Admin | PlaceKind::Country))
    {
        if !p
            .names
            .iter()
            .skip(1)
            .any(|n| n.lang.is_some() && cut(&n.text).is_some())
        {
            continue;
        }
        let mut kept: Vec<PlaceName> = Vec::with_capacity(p.names.len());
        for (i, mut n) in std::mem::take(&mut p.names).into_iter().enumerate() {
            if i > 0
                && n.lang.is_some()
                && let Some(bare) = cut(&n.text)
            {
                n.text = bare;
                naming.type_words_cut += 1;
            }
            if !kept.iter().any(|m| m.same(&n)) {
                kept.push(n);
            }
        }
        p.names = kept;
    }
}

/// Drops the names (never the canonical, the first) that carry their context after a comma or
/// in brackets: "Chandragiri, Nepal" beside "Chandragiri", or "Bara District, Nepal" where
/// the text after the last comma is the country's or an admin unit's name. Search reads the
/// context after a comma itself (D-119), and the qualifier's words would otherwise make the
/// place a match for its country's or district's name (D-202). Returns how many were dropped.
pub(super) fn drop_qualified(places: &mut [Place], naming: &mut Naming) -> usize {
    let context = context_names(places);
    let mut n = 0;
    for p in places.iter_mut() {
        let own: BTreeSet<String> = p.names.iter().map(|x| fold(&x.text)).collect();
        let key = p.key.clone();
        let mut first = true;
        p.names.retain(|x| {
            if std::mem::take(&mut first) {
                return true;
            }
            let Some(head) = qualified_head(&x.text) else {
                return true;
            };
            let tail = x.text.rsplit(", ").next().unwrap_or_default();
            let why = if own.contains(&fold(head)) {
                head.to_string()
            } else if x.text.contains(", ") && context.contains(&fold(tail)) {
                tail.to_string()
            } else {
                return true;
            };
            naming.dropped.insert((
                key.clone(),
                String::new(),
                x.text.clone(),
                Dropped::Qualified,
                why,
            ));
            n += 1;
            false
        });
    }
    n
}

/// The display name (names-canonical §5.2); else the first record's name, in source priority,
/// that can be stored; else the first of the other names that can. With it, how it was made
/// when this step made it: romanised, or respelled by the lexicon (D-227); else `Unknown`.
pub(super) fn display_name(
    here: &Here<'_>,
    recs: &[(&Rec, &str, usize)],
    candidates: &[Vec<&str>],
    rest: &BTreeSet<PlaceName>,
    display: &Namer,
    naming: &mut Naming,
) -> Option<(String, Origin)> {
    let chosen = display.choose(candidates, pack::fits);
    match &chosen {
        Some(c) => {
            *naming.by.entry(c.how).or_default() += 1;
            naming.rendered += usize::from(c.rendered);
            let first = recs.first().map_or("", |(r, _, _)| r.name.as_str());
            if c.text != first {
                let how = match (c.how, c.rendered) {
                    (How::Romanised, _) => "romanised",
                    (How::Voted, true) => "majority, respelled",
                    (How::Voted, false) => "majority",
                    (How::Latin, true) => "latin, respelled",
                    (How::Latin, false) => "latin",
                };
                naming.changed.push((
                    here.cluster.to_string(),
                    first.to_string(),
                    c.text.clone(),
                    how,
                ));
            }
            let origin = match (c.how, c.rendered) {
                (How::Romanised, _) => Origin::Romanisation,
                (_, true) => Origin::Lexicon,
                _ => Origin::Unknown,
            };
            Some((c.text.clone(), origin))
        }
        None => candidates
            .iter()
            .filter_map(|c| c.first().copied())
            .chain(rest.iter().map(|n| n.text.as_str()))
            .find(|n| pack::fits(n))
            .map(|n| {
                naming.fallback += 1;
                (n.to_string(), Origin::Unknown)
            }),
    }
}

#[cfg(test)]
mod qualified_tests {
    use super::{Naming, Place, PlaceName, cut_unit_type_words, drop_qualified, qualified_head};
    use teigeodata_core::PlaceKind;

    /// D-243: a unit's labelled names lose a trailing type word after a space, longest first;
    /// the canonical name, a glued word, a name that is only the type word, an unlabelled name
    /// and a settlement's names stay; a cut that repeats another name of the place goes.
    #[test]
    fn a_unit_s_names_lose_their_type_word() {
        let words: Vec<String> = ["उपमहानगरपालिका", "नगरपालिका", "जिल्ला"]
            .map(String::from)
            .to_vec();
        let named = |key: &str, kind, names: &[(&str, Option<&str>)]| {
            let mut p = place(key, kind, &[]);
            p.names = names
                .iter()
                .map(|&(t, l)| PlaceName {
                    text: t.into(),
                    lang: l.map(Into::into),
                    kind: None,
                    origin: crate::build::Origin::Unknown,
                })
                .collect();
            p
        };
        let mut places = [
            named(
                "admin:a",
                PlaceKind::Admin,
                &[
                    ("Kailali", None),
                    ("कैलाली जिल्ला", Some("ne")),
                    ("कैलाली", Some("ne")),
                    ("धरान उपमहानगरपालिका", Some("ne")),
                    ("जिल्ला", Some("ne")),
                    ("भीमनगरपालिका", Some("ne")),
                    ("सुर्खेत जिल्ला", None),
                ],
            ),
            named(
                "s",
                PlaceKind::Settlement,
                &[("X", None), ("कैलाली जिल्ला", Some("ne"))],
            ),
        ];
        let mut naming = Naming::default();
        cut_unit_type_words(&mut places, &words, &mut naming);
        let texts = |p: &Place| p.names.iter().map(|n| n.text.clone()).collect::<Vec<_>>();
        assert_eq!(
            texts(&places[0]),
            [
                "Kailali",
                "कैलाली",
                "धरान",
                "जिल्ला",
                "भीमनगरपालिका",
                "सुर्खेत जिल्ला"
            ]
        );
        assert_eq!(texts(&places[1]), ["X", "कैलाली जिल्ला"]);
        assert_eq!(naming.type_words_cut, 2);
    }

    fn place(key: &str, kind: PlaceKind, names: &[&str]) -> Place {
        Place {
            key: key.into(),
            kind,
            admin_level: 0,
            flags: 0,
            point: (0, 0),
            names: names
                .iter()
                .map(|t| PlaceName {
                    text: (*t).into(),
                    lang: None,
                    kind: None,
                    origin: crate::build::Origin::Unknown,
                })
                .collect(),
            population: None,
            rank: 0,
            parent: None,
            sources: Vec::new(),
        }
    }

    #[test]
    fn names_with_their_context_are_dropped_and_others_kept() {
        let mut places = vec![
            place("c", PlaceKind::Country, &["Nepal"]),
            place("d", PlaceKind::Admin, &["Bara", "Bara District, Nepal"]),
            place(
                "s",
                PlaceKind::Settlement,
                &[
                    "Chandragiri",
                    "Chandragiri, Nepal",
                    "Chandragiri (Nepal)",
                    "Ram, Shyam",
                ],
            ),
            // The canonical name stays, whatever it is.
            place("t", PlaceKind::Settlement, &["Mimi, Nepal", "Mimi"]),
        ];
        let mut naming = Naming::default();
        assert_eq!(drop_qualified(&mut places, &mut naming), 3);
        let names =
            |i: usize| -> Vec<&str> { places[i].names.iter().map(|n| n.text.as_str()).collect() };
        assert_eq!(names(1), ["Bara"]);
        // "Ram, Shyam": neither part is the place's or the context's: kept.
        assert_eq!(names(2), ["Chandragiri", "Ram, Shyam"]);
        assert_eq!(names(3), ["Mimi, Nepal", "Mimi"]);
    }

    #[test]
    fn a_qualifier_is_cut_after_a_comma_or_a_bracket() {
        assert_eq!(
            qualified_head("Bara District, Nepal"),
            Some("Bara District")
        );
        assert_eq!(qualified_head("Seri (Nepal, Karnali)"), Some("Seri"));
        assert_eq!(qualified_head("बेनी, सोलुखुम्बु"), Some("बेनी"));
        assert_eq!(qualified_head("Kathmandu"), None);
        assert_eq!(qualified_head(", Nepal"), None);
        assert_eq!(qualified_head("Bara, "), None);
    }
}
