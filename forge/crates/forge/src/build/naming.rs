//! A place's names (D-060, D-107) and display name (D-106).

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
use teigeodata_format::names::NameType;

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
    let canonical = display_name(here, recs, &candidates, &rest, display, naming)?;
    let mut all: Vec<PlaceName> = vec![PlaceName {
        text: canonical.clone(),
        lang: None,
        kind: None,
    }];
    let mut seen: BTreeSet<String> = BTreeSet::from([canonical]);
    for n in rest {
        if seen.insert(n.text.clone()) {
            all.push(n);
        }
    }
    Some(all)
}

/// The display name (names-canonical §5.2); else the first record's name, in source priority,
/// that can be stored; else the first of the other names that can.
pub(super) fn display_name(
    here: &Here<'_>,
    recs: &[(&Rec, &str, usize)],
    candidates: &[Vec<&str>],
    rest: &BTreeSet<PlaceName>,
    display: &Namer,
    naming: &mut Naming,
) -> Option<String> {
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
            Some(c.text.clone())
        }
        None => candidates
            .iter()
            .filter_map(|c| c.first().copied())
            .chain(rest.iter().map(|n| n.text.as_str()))
            .find(|n| pack::fits(n))
            .map(|n| {
                naming.fallback += 1;
                n.to_string()
            }),
    }
}
