//! The boundary source's admin units and the country place.

use super::Place;
use super::PlaceName;
use super::Profile;
use super::sources::read_names;
use super::sources::read_records;
use crate::fetch::store_path;
use crate::geom::GridIndex;
use crate::geom::Polygon;
use crate::lock::Lock;
use crate::read::boundaries;
use std::collections::BTreeSet;
use std::path::Path;
use teigeodata_core::PlaceKind;
use teigeodata_format::names::NameType;

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
    let zip = store_path(root, lock.get(source)?);
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
                }],
                population: None,
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

/// The country, from the record of its Wikidata item, when a source has it.
pub(super) fn country_place(
    build: &Path,
    read: &[String],
    profile: &Profile,
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
            }];
            let mut seen = BTreeSet::from([r.name.clone()]);
            for n in read_names(build, s)?.remove(&id).unwrap_or_default() {
                if seen.insert(n.text.clone()) {
                    names.push(n);
                }
            }
            return Ok(Some(Place {
                sources: vec![id.clone()],
                key: id,
                kind: PlaceKind::Country,
                admin_level: 0,
                flags: 0,
                point,
                names,
                population: r.population,
                parent: None,
            }));
        }
    }
    Ok(None)
}
