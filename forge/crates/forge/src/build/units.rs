//! The boundary source's admin units and the country place.

use super::Place;
use super::PlaceName;
use super::Profile;
use super::naming::Naming;
use super::sources::read_names;
use super::sources::read_records;
use crate::aliases::rules::{Dropped, Rules};
use crate::geom::GridIndex;
use crate::geom::Polygon;
use crate::lock::Lock;
use crate::read::boundaries;
use std::collections::BTreeSet;
use std::path::Path;
use teigeodata_core::PlaceKind;
use teigeodata_format::names::NameType;
use teigeodata_names::PhoneticKey;

/// The admin places, their polygons level by level, and the box of the whole set.
pub(super) struct Units {
    pub(super) places: Vec<Place>,
    /// Coarsest level first.
    pub(super) levels: Vec<Level>,
    pub(super) coverage: Option<(i32, i32, i32, i32)>,
}

/// One admin level's polygons with a grid index over their boxes.
pub(super) struct Level {
    /// The admin level.
    pub(super) number: u8,
    pub(super) keys: Vec<String>,
    pub(super) polys: Vec<Polygon>,
    pub(super) index: GridIndex,
}

impl Level {
    /// The key of the unit holding `p`.
    pub(super) fn find(&self, p: (i32, i32)) -> Option<&String> {
        self.index
            .candidates(p.0, p.1)
            .find(|&i| {
                self.polys
                    .get(i)
                    .is_some_and(|poly| poly.contains(p.0, p.1))
            })
            .and_then(|i| self.keys.get(i))
    }
}

/// 0.1° cells: a few polygons per cell at local-unit scale (as `pip`).
pub(super) const CELL_E7: i32 = 1_000_000;

pub(super) fn admin_places(root: &Path, lock: &Lock, source: &str) -> Result<Units, String> {
    let zip = lock.path(root, source)?;
    let mut numbers = boundaries::levels(&zip)?;
    numbers.sort_unstable();
    let mut places = Vec::new();
    let mut levels: Vec<Level> = Vec::new();
    let mut coverage: Option<(i32, i32, i32, i32)> = None;
    for &level in numbers.iter().filter(|&&l| l > 0) {
        let mut units = boundaries::read(&zip, level)?;
        units.sort_by(|a, b| a.pcode.cmp(&b.pcode));
        let (mut keys, mut polys) = (Vec::new(), Vec::new());
        for u in units {
            // National codes, not the source ID: a new boundary release keeps the IDs.
            let key = format!("admin:{}", u.pcode);
            let point = u
                .polygon
                .interior_point()
                .ok_or_else(|| format!("{key}: no point inside its polygon"))?;
            if let Some(b) = u.polygon.bbox() {
                coverage = Some(coverage.map_or(b, |c| {
                    (c.0.min(b.0), c.1.min(b.1), c.2.max(b.2), c.3.max(b.3))
                }));
            }
            // The parent is the unit one level up that holds this unit's point.
            let parent = levels.last().and_then(|l| l.find(point)).cloned();
            places.push(Place {
                key: key.clone(),
                kind: PlaceKind::Admin,
                admin_level: level,
                flags: 0,
                point,
                names: vec![PlaceName {
                    text: u.name.clone(),
                    lang: None,
                    kind: Some(NameType::Official),
                    origin: crate::build::Origin::Unknown,
                }],
                population: None,
                rank: 0,
                parent,
                sources: vec![key.clone()],
            });
            keys.push(key);
            polys.push(u.polygon);
        }
        let index = GridIndex::new(&polys, CELL_E7);
        levels.push(Level {
            number: level,
            keys,
            polys,
            index,
        });
    }
    Ok(Units {
        places,
        levels,
        coverage,
    })
}

/// The deepest admin unit holding `p`, by key.
pub(super) fn deepest(levels: &[Level], p: (i32, i32)) -> Option<String> {
    levels.iter().rev().find_map(|l| l.find(p)).cloned()
}

/// The country's names after its primary `label`: each distinct name of `own` the country
/// keeps ([`Rules::country_keeps`] against `units`); `dropped` is told of the others, with the
/// unit.
fn country_names(
    label: &str,
    own: Vec<PlaceName>,
    units: &[Place],
    phonetic: Option<PhoneticKey>,
    mut dropped: impl FnMut(&str, String),
) -> Vec<PlaceName> {
    let mut rules = Rules::new(phonetic, None);
    for p in units {
        if let Some(n) = p.names.first() {
            rules.add_admin(&n.text);
        }
    }
    rules.finish();
    let label_keys = rules.anchor([label]);
    let mut seen = BTreeSet::from([label.to_string()]);
    let mut out = Vec::new();
    for n in own {
        if !seen.insert(n.text.clone()) {
            continue;
        }
        match rules.country_keeps(&label_keys, &n.text) {
            Ok(()) => out.push(n),
            Err(unit) => dropped(&n.text, unit),
        }
    }
    out
}

/// The country, from the record of its Wikidata item, when a source has it.
///
/// The country takes its record's names but, as a lent name (D-060), none that is the primary
/// name of one of `units` by fold or phonetic key: Wikidata lists Palpa, a district and a
/// former kingdom, as an alias of Nepal. Each one left out goes to `naming`.
pub(super) fn country_place(
    build: &Path,
    read: &[String],
    profile: &Profile,
    units: &[Place],
    naming: &mut Naming,
) -> Result<Option<Place>, String> {
    let Some(q) = &profile.wikidata else {
        return Ok(None);
    };
    let id = format!("wikidata:{q}");
    for s in read {
        let recs = read_records(build, s)?;
        if let Some(r) = recs.get(&id)
            && let Some(point) = r.point
        {
            let mut names = vec![PlaceName {
                text: r.name.clone(),
                lang: None,
                kind: Some(NameType::Official),
                origin: crate::build::Origin::Unknown,
            }];
            let own = read_names(build, s)?.remove(&id).unwrap_or_default();
            names.extend(country_names(
                &r.name,
                own,
                units,
                profile.phonetic,
                |t, with| {
                    naming.dropped.insert((
                        id.clone(),
                        id.clone(),
                        t.to_string(),
                        Dropped::Collision,
                        with,
                    ));
                },
            ));
            return Ok(Some(Place {
                sources: vec![id.clone()],
                key: id,
                kind: PlaceKind::Country,
                admin_level: 0,
                flags: 0,
                point,
                names,
                population: r.population,
                rank: 0,
                parent: None,
            }));
        }
    }
    Ok(None)
}

/// Parents every level-1 unit without a parent (current or former) to the country `key`: the
/// boundary source's levels start at 1, so the country is the root above them (D-146).
/// Returns how many it parented.
pub(super) fn parent_to_country(places: &mut [Place], key: &str) -> usize {
    let mut n = 0;
    for p in places
        .iter_mut()
        .filter(|p| p.kind == PlaceKind::Admin && p.admin_level == 1 && p.parent.is_none())
    {
        p.parent = Some(key.to_string());
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::{Place, PlaceName, country_names, parent_to_country};
    use teigeodata_core::PlaceKind;
    use teigeodata_names::PhoneticKey;

    fn name(t: &str) -> PlaceName {
        PlaceName {
            text: t.to_string(),
            lang: None,
            kind: None,
            origin: crate::build::Origin::Unknown,
        }
    }

    #[test]
    fn level_one_units_hang_from_the_country() {
        let place = |key: &str, kind, admin_level, parent: Option<&str>| Place {
            key: key.into(),
            kind,
            admin_level,
            flags: 0,
            point: (0, 0),
            names: Vec::new(),
            population: None,
            rank: 0,
            parent: parent.map(str::to_string),
            sources: Vec::new(),
        };
        let mut places = [
            place("admin:A", PlaceKind::Admin, 1, None),
            place("admin:A1", PlaceKind::Admin, 2, Some("admin:A")),
            place("halo", PlaceKind::Settlement, 0, None),
            place("admin:B", PlaceKind::Admin, 1, Some("elsewhere")),
        ];
        assert_eq!(parent_to_country(&mut places, "country"), 1);
        let parents: Vec<_> = places.iter().map(|p| p.parent.as_deref()).collect();
        assert_eq!(
            parents,
            [Some("country"), Some("admin:A"), None, Some("elsewhere")]
        );
    }

    #[test]
    fn the_country_takes_no_unit_name_as_an_alias() {
        let unit = |t: &str| Place {
            key: format!("codab:{t}"),
            kind: PlaceKind::Admin,
            admin_level: 2,
            flags: 0,
            point: (0, 0),
            names: vec![name(t)],
            population: None,
            rank: 0,
            parent: None,
            sources: Vec::new(),
        };
        // A unit may share the country's name (Luxembourg, Djibouti): its spellings stay.
        let units = [unit("Palpa"), unit("Nepalgunj"), unit("Nepal")];
        let own = vec![
            name("नेपाल"),
            name("Palpa"),
            name("palpā"), // the same by fold
            name("Federal Democratic Republic of Nepal"),
            name("Nepal"),
            name("Nepāl"),
        ];
        let mut dropped = Vec::new();
        let kept = country_names(
            "Nepal",
            own,
            &units,
            PhoneticKey::from_name("indic-v0"),
            |t, with| dropped.push((t.to_string(), with)),
        );
        let kept: Vec<&str> = kept.iter().map(|n| n.text.as_str()).collect();
        assert_eq!(
            kept,
            ["नेपाल", "Federal Democratic Republic of Nepal", "Nepāl"]
        );
        assert_eq!(
            dropped,
            [
                ("Palpa".to_string(), "admin:Palpa".to_string()),
                ("palpā".to_string(), "admin:Palpa".to_string())
            ]
        );
    }
}
