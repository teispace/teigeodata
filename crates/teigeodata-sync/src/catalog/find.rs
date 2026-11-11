//! Discovery: from a name to the packs that hold it (D-164, [engine and catalog
//! §3.7](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/engine-and-catalog.md#37-discovery-dubai--ae-du)).
//!
//! A [`Resolver`] turns a name into candidate places, each with a point and the countries whose
//! packs could hold it: the engine's search over the embedded baseline (and the gazetteer pack,
//! when installed) with the locator around each point; later perhaps a hosted resolver. The
//! [`Catalog`] then gives each candidate its packs, offline: a country's own packs (`np`), and
//! the packs of a part of it (`ae-du`) whose box holds the point.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use teigeodata_core::{E7, Point};

use super::{Entry, Error, country_of};
use crate::trust::{Target, Targets, Trusted};

/// A place a [`Resolver`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Its stable ID (`PlaceId`), to show it with the engine.
    pub id: u64,
    /// Its point.
    pub point: Point<E7>,
    /// The resolver's score, higher first.
    pub score: i32,
    /// The countries whose packs could hold it (lower-case ISO 3166-1 alpha-2), each once.
    pub countries: Vec<[u8; 2]>,
}

/// Names to candidate places.
pub trait Resolver {
    /// Why resolving failed.
    type Error;

    /// The best `k` candidates for `query`, best first, into `out`.
    ///
    /// # Errors
    /// The resolver's own.
    fn resolve(&self, query: &str, k: usize, out: &mut Vec<Candidate>) -> Result<(), Self::Error>;
}

/// A candidate and the catalog's packs for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found<'a> {
    /// The place.
    pub candidate: Candidate,
    /// Its packs, by path; empty when the catalog has none.
    pub packs: Vec<&'a Entry>,
}

/// The verified catalog's entries, by path and by country.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    entries: BTreeMap<String, Entry>,
    by_country: BTreeMap<[u8; 2], Vec<String>>,
}

impl Catalog {
    /// The entries of verified targets. An entry this reader refuses is left out and reported,
    /// so one bad or newer entry never hides the rest.
    #[must_use]
    pub fn new(targets: &Targets) -> (Self, Vec<(String, Error)>) {
        Self::of(targets.targets.iter().map(|(p, t)| (p.as_str(), t)))
    }

    /// The entries of every target `trusted` trusts: the top-level targets and the delegated
    /// roles loaded so far (one per country the app asked for, D-175), each path from the role
    /// that may sign it. Refusals as [`Self::new`].
    #[must_use]
    pub fn trusted(trusted: &Trusted) -> (Self, Vec<(String, Error)>) {
        Self::of(trusted.trusted_targets())
    }

    fn of<'a>(
        targets: impl Iterator<Item = (&'a str, &'a Target)>,
    ) -> (Self, Vec<(String, Error)>) {
        let mut c = Self::default();
        let mut refused = Vec::new();
        for (path, t) in targets {
            match Entry::from_target(path, t) {
                Ok(e) => c.insert(String::from(path), e),
                Err(err) => refused.push((String::from(path), err)),
            }
        }
        (c, refused)
    }

    fn insert(&mut self, path: String, e: Entry) {
        if let Some(cc) = country_of(&e.address) {
            self.by_country.entry(cc).or_default().push(path.clone());
        }
        self.entries.insert(path, e);
    }

    /// The entry at `path`.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&Entry> {
        self.entries.get(path)
    }

    /// Every entry, by path.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = (&str, &Entry)> {
        self.entries.iter().map(|(p, e)| (p.as_str(), e))
    }

    /// The packs that could hold `at` in `country`: the country's own (`np`), and each pack of
    /// a part of it (`ae-du`) whose box holds `at` or that has no box. By path.
    pub fn packs_at(&self, country: [u8; 2], at: Point<E7>) -> impl Iterator<Item = &Entry> {
        self.by_country
            .get(&country)
            .into_iter()
            .flatten()
            .filter_map(|p| self.entries.get(p))
            .filter(move |e| e.address.len() == 2 || e.bbox.is_none_or(|b| holds(b, at)))
    }

    /// Discovery: the candidates `resolver` finds for `query` (at most `k`), each with its
    /// packs. Runs offline once the catalog is verified.
    ///
    /// # Errors
    /// The resolver's.
    pub fn find<R: Resolver>(
        &self,
        resolver: &R,
        query: &str,
        k: usize,
    ) -> Result<Vec<Found<'_>>, R::Error> {
        let mut candidates = Vec::new();
        resolver.resolve(query, k, &mut candidates)?;
        Ok(candidates
            .into_iter()
            .map(|candidate| {
                let mut packs: Vec<&Entry> = Vec::new();
                for &cc in &candidate.countries {
                    for e in self.packs_at(cc, candidate.point) {
                        if !packs.iter().any(|p| core::ptr::eq(*p, e)) {
                            packs.push(e);
                        }
                    }
                }
                Found { candidate, packs }
            })
            .collect())
    }
}

/// Whether `b` holds `at`, edges included; a box across the antimeridian has `max.lon` below
/// `min.lon`.
fn holds(b: teigeodata_format::Bbox, at: Point<E7>) -> bool {
    let lat = (b.min.lat()..=b.max.lat()).contains(&at.lat());
    let lon = if b.min.lon() <= b.max.lon() {
        (b.min.lon()..=b.max.lon()).contains(&at.lon())
    } else {
        at.lon() >= b.min.lon() || at.lon() <= b.max.lon()
    };
    lat && lon
}
