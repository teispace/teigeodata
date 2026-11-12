//! Admin records of the points sources (D-110): a cluster with no settlement or locality whose
//! records are admin units (`GeoNames` `A.*`, Wikidata or OSM admin items) either names a
//! current unit of the boundary source, or a former one, or neither.
//!
//! - **Linked:** the cluster names a current unit when one of its names matches one of the
//!   unit's by fold, join or phonetic key (also romanised), and the unit holds the cluster's
//!   point, at the source's level first (`GeoNames` `ADM<n>`) and then any level; failing
//!   that, the matching unit whose outline is nearest the point, within [`NEAR_M`], of the
//!   source's level, or of any level for a record with no class (a Wikidata item: Nepal's
//!   newer districts carry points 16–17 km outside their outlines). Never for a division. The unit takes the records' keys (so their IDs resolve to it) and their names,
//!   under the collision rule of D-060 (Wikidata lists Hetauda as an alias of Makwanpur
//!   District). A country record (`PCLI`) links to the country.
//! - **Former:** an unlinked `ADM<n>H` with `n` ≤ [`MAX_FORMER_LEVEL`] becomes a place of its
//!   own, an admin unit flagged former, whose parent is the current unit one level up that
//!   holds its point (schema: former units stay, linked to their successors). Deeper former
//!   units (Nepal's 36,035 former wards, `ADM5H`) are numbered parts, left out.
//! - **Division:** an unlinked `ADMD` ("an administrative division, undifferentiated") is a
//!   locality: in Nepal these are villages.
//! - Anything else is counted and left out.

use std::collections::{HashMap, HashSet};

use crate::aliases::rules::Rules;
use crate::links::haversine_e7;

use super::{Level, Place, PlaceName};

/// The farthest a unit's outline may be from a record that names it, when the record's point is
/// outside the unit.
pub(super) const NEAR_M: f64 = 30_000.0;

/// The deepest former level kept as places.
pub(super) const MAX_FORMER_LEVEL: u8 = 4;

/// What an admin record says it is, from its source class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Class {
    /// `ADM<n>`, a current unit of level `n`.
    Current(u8),
    /// `ADM<n>H`, a former unit of level `n`.
    Former(u8),
    /// `ADMD`, an undifferentiated division.
    Division,
    /// `PCLI` and the like: the country.
    Country,
    /// Any other admin record (Wikidata, OSM): level unknown.
    Other,
}

impl Class {
    /// The class of a `GeoNames` feature (`A.ADM3`, `A.ADM4H`, `A.ADMD`, `A.PCLI`).
    pub(super) fn of(class: &str) -> Self {
        let Some(code) = class.strip_prefix("A.") else {
            return Self::Other;
        };
        if code.starts_with("PCL") {
            return Self::Country;
        }
        if code == "ADMD" || code == "ADMDH" {
            return Self::Division;
        }
        let Some(rest) = code.strip_prefix("ADM") else {
            return Self::Other;
        };
        let (digits, former) = match rest.strip_suffix('H') {
            Some(d) => (d, true),
            None => (rest, false),
        };
        match digits.parse::<u8>() {
            Ok(n) if former => Self::Former(n),
            Ok(n) => Self::Current(n),
            Err(_) => Self::Other,
        }
    }

    /// The most specific class among a cluster's records: a stated level wins over none.
    pub(super) fn of_cluster<'c>(classes: impl IntoIterator<Item = &'c str>) -> Self {
        let rank = |c: Self| match c {
            Self::Country => 5,
            Self::Current(_) => 4,
            Self::Former(_) => 3,
            Self::Division => 2,
            Self::Other => 1,
        };
        classes
            .into_iter()
            .map(Self::of)
            .max_by_key(|&c| rank(c))
            .unwrap_or(Self::Other)
    }

    /// The level the class states.
    const fn level(self) -> Option<u8> {
        match self {
            Self::Current(n) | Self::Former(n) => Some(n),
            _ => None,
        }
    }
}

/// The current units, ready for matching.
pub(super) struct Units<'a> {
    /// Match key of a unit name → the units with it (place indexes).
    by_key: HashMap<String, Vec<usize>>,
    /// Unit key → place index.
    index: HashMap<&'a str, usize>,
    /// Unit key → its outline (level index, polygon index).
    outline: HashMap<&'a str, (usize, usize)>,
    places: &'a [Place],
    levels: &'a [Level],
}

impl<'a> Units<'a> {
    pub(super) fn new(places: &'a [Place], levels: &'a [Level], rules: &Rules<'_>) -> Self {
        let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, p) in places.iter().enumerate() {
            for k in rules.anchor(p.names.iter().map(|n| n.text.as_str())) {
                by_key.entry(k).or_default().push(i);
            }
        }
        let index = places
            .iter()
            .enumerate()
            .map(|(i, p)| (p.key.as_str(), i))
            .collect();
        let outline = levels
            .iter()
            .enumerate()
            .flat_map(|(l, level)| {
                level
                    .keys
                    .iter()
                    .enumerate()
                    .map(move |(i, k)| (k.as_str(), (l, i)))
            })
            .collect();
        Self {
            by_key,
            index,
            outline,
            places,
            levels,
        }
    }

    /// The current unit a cluster with these names, at `point`, names; its place index.
    pub(super) fn find(
        &self,
        class: Class,
        point: (i32, i32),
        names: &[&str],
        rules: &Rules<'_>,
    ) -> Option<usize> {
        let matching: HashSet<usize> = rules
            .anchor(names.iter().copied())
            .iter()
            .filter_map(|k| self.by_key.get(k))
            .flatten()
            .copied()
            .collect();
        if matching.is_empty() {
            return None;
        }
        let want = class.level();
        // The units holding the point, the source's level first, then deepest first.
        let mut holding: Vec<usize> = self
            .levels
            .iter()
            .rev()
            .filter_map(|l| l.find(point))
            .filter_map(|k| self.index.get(k.as_str()).copied())
            .filter(|i| matching.contains(i))
            .collect();
        holding.sort_by_key(|&i| self.places.get(i).map(|p| p.admin_level) != want);
        if let Some(&i) = holding.first() {
            return Some(i);
        }
        // The point is off its unit: the named unit whose outline is nearest, of the source's
        // level when it states one; a unit with no outline by its point. A division states no
        // level but is a village, never a unit off its point: Mahankal, a former village of
        // Kathmandu, is not Lalitpur's Mahankal 25 km away.
        if want.is_none() && class != Class::Other {
            return None;
        }
        matching
            .iter()
            .filter_map(|&i| Some((i, self.places.get(i)?)))
            .filter(|(_, p)| want.is_none_or(|l| p.admin_level == l))
            .map(|(i, p)| {
                let d = self
                    .outline
                    .get(p.key.as_str())
                    .and_then(|&(l, j)| self.levels.get(l)?.polys.get(j))
                    .map_or_else(
                        || haversine_e7(point, p.point),
                        |poly| poly.distance_m(point),
                    );
                (d, i)
            })
            .filter(|&(d, _)| d < NEAR_M)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, i)| i)
    }

    /// The unit at a place index.
    pub(super) fn place(&self, i: usize) -> Option<&Place> {
        self.places.get(i)
    }

    /// The key of the current unit coarser than `level` that holds `point`: a former unit's
    /// successor.
    pub(super) fn successor(&self, level: u8, point: (i32, i32)) -> Option<String> {
        self.levels
            .iter()
            .rev()
            .filter(|l| l.number < level)
            .find_map(|l| l.find(point))
            .cloned()
    }
}

/// Names one cluster gives the unit it links to.
pub(super) struct Link {
    /// The unit's place index, or `None` for the country.
    pub(super) unit: Option<usize>,
    pub(super) sources: Vec<String>,
    pub(super) names: Vec<PlaceName>,
    pub(super) population: Option<i64>,
}

/// Adds a link's keys and names to its place; returns how many names it added.
pub(super) fn apply(place: &mut Place, link: Link) -> usize {
    let before = place.names.len();
    for n in link.names {
        if !place.names.iter().any(|m| m.same(&n)) {
            place.names.push(n);
        }
    }
    for s in link.sources {
        if !place.sources.contains(&s) {
            place.sources.push(s);
        }
    }
    if place.population.is_none() {
        place.population = link.population;
    }
    place.names.len() - before
}

#[cfg(test)]
mod tests {
    use super::{Class, NEAR_M, Units};
    use crate::aliases::rules::Rules;
    use crate::build::{Level, Origin, Place, PlaceName};
    use crate::geom::{GridIndex, Polygon};
    use teigeodata_core::PlaceKind;

    /// D-239: a record off its unit's outline links to the named unit whose outline is
    /// nearest, within `NEAR_M`: of the stated level, or of any level for a record with no class
    /// (Wikidata); never for a division (a village), nor past `NEAR_M`.
    #[test]
    fn a_record_off_its_unit_links_by_the_nearest_outline() {
        // A 0.1° square unit at level 3, its label point in the middle.
        let poly = Polygon {
            rings: vec![vec![
                (0, 0),
                (1_000_000, 0),
                (1_000_000, 1_000_000),
                (0, 1_000_000),
            ]],
        };
        let level = Level {
            number: 3,
            keys: vec!["admin:X1".into()],
            index: GridIndex::new(std::slice::from_ref(&poly), 1_000_000),
            polys: vec![poly],
        };
        let place = Place {
            key: "admin:X1".into(),
            kind: PlaceKind::Admin,
            admin_level: 3,
            flags: 0,
            point: (500_000, 500_000),
            names: vec![PlaceName {
                text: "Mahankal".into(),
                lang: None,
                kind: None,
                origin: Origin::Official,
            }],
            population: None,
            rank: 0,
            parent: None,
            sources: vec![],
        };
        let (places, levels) = ([place], [level]);
        let mut rules = Rules::new(None, None);
        rules.add_admin("Mahankal");
        rules.finish();
        let units = Units::new(&places, &levels, &rules);
        // 0.25° north of the square's top edge: about 27.8 km from the outline, 33.4 km from
        // the label point, past NEAR_M.
        let off = (500_000, 3_500_000);
        let find = |class, at| units.find(class, at, &["Mahankal"], &rules);
        assert_eq!(find(Class::Other, off), Some(0));
        assert_eq!(find(Class::Current(3), off), Some(0));
        assert_eq!(find(Class::Current(2), off), None);
        assert_eq!(find(Class::Division, off), None);
        // Inside, any class links; past NEAR_M, none.
        assert_eq!(find(Class::Division, (500_000, 500_000)), Some(0));
        const { assert!(NEAR_M < 33_000.0) };
        assert_eq!(find(Class::Other, (500_000, 4_000_000)), None);
    }

    #[test]
    fn classes() {
        assert_eq!(Class::of("A.ADM3"), Class::Current(3));
        assert_eq!(Class::of("A.ADM4H"), Class::Former(4));
        assert_eq!(Class::of("A.ADMD"), Class::Division);
        assert_eq!(Class::of("A.PCLI"), Class::Country);
        assert_eq!(Class::of("A.ADMX"), Class::Other);
        assert_eq!(Class::of("P.PPL"), Class::Other);
        assert_eq!(
            Class::of_cluster(["", "A.ADM3H", "A.ADM2"]),
            Class::Current(2)
        );
        assert_eq!(Class::of_cluster(["", "Q123"]), Class::Other);
        assert_eq!(Class::of_cluster([]), Class::Other);
    }
}
