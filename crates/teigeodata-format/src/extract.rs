//! Rows of a pack as a new pack's sections (D-145): the embedded baseline's slice of a country
//! pack, and the CLI's `extract`.
//!
//! [`rows`] keeps rows `keep` of a pack, in their order (PLACES stays in Hilbert order), and
//! re-encodes every per-row section the engine reads: IDMAP, PLACES, `PLACE_INDEX`, ELEVATION,
//! NAMES, SEARCH (keys cut to the kept rows, the query rules and unit types kept), and
//! HIERARCHY, where each kept row's parent is its **nearest kept ancestor**, so a slice that
//! keeps its places' ancestors keeps their chain (D-143). The admin geometry of the kept rows
//! (ARCS, POLYGONS, `POLY_INDEX`, `ADMIN_REGION`) is re-encoded with only the arcs they use, at
//! level of detail `lod` of the pack's ARCS (0 = full resolution): the levels are the forge's
//! topology-safe simplification, so no new simplifier runs here. `PLACE_TZ` keeps each kept
//! row's zone when the zone's row is kept too, and none otherwise (D-151): keep the zone rows
//! of the kept places to keep their zones.
//!
//! Left out, for the caller to add if it needs them: REGION (a slice answers `tz_at` from its
//! units and `PLACE_TZ`), REDIRECTS, POSTAL, DEM, BLOBS and EXTRA. The manifest is the
//! caller's too: it names the new pack, and must carry the source pack's attribution.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec;
use alloc::vec::Vec;

use teigeodata_core::{E6, E7, Point};

use crate::arcs::{self, ArcColumns, Arcs};
use crate::elevation::{self, Elevations};
use crate::hierarchy::{self, Hierarchy};
use crate::idmap::{self, IdMap};
use crate::names::{self, NameRecord, Names};
use crate::place_index::{self, PlaceIndex};
use crate::place_tz::{self, PlaceTz};
use crate::places::{self, PlaceColumns, Places};
use crate::poly_index::{self, PolyIndex, Rect};
use crate::polygons::{self, PolygonInput, Polygons};
use crate::region::{self, Options};
use crate::search::{self, Search};
use crate::{Encoding, Error, Kind, Pack};

/// One section of the new pack: kind, encoding, item count, body.
pub type Body = (Kind, Encoding, u64, Vec<u8>);

/// The node size of a rebuilt index when the pack has none to copy.
const NODE: usize = 16;

fn fail(problem: &'static str) -> Error {
    Error::Write(problem)
}

/// The finest level of `arcs` whose tolerance is at most `max_cm`: 0 (full resolution) when the
/// arcs have no levels or no level is that fine.
#[must_use]
pub fn lod_within(arcs: &Arcs<'_>, max_cm: u32) -> u8 {
    (1..=6)
        .rev()
        .find(|&l| arcs.tolerance_cm(l).is_some_and(|t| t <= max_cm))
        .unwrap_or(0)
}

/// The sections of rows `keep` (increasing, each a PLACES row of `pack`) as a pack of their own,
/// admin geometry at level `lod` with `ADMIN_REGION` built with `region` (a coarse slice wants a
/// coarser index than the pack's). See the [module](self).
///
/// # Errors
/// [`Error::Write`] if `keep` is not increasing or names a row past the end, the pack has no
/// IDMAP or PLACES, or a section does not re-encode; a reader's error for a malformed section.
pub fn rows(pack: &Pack<'_>, keep: &[u32], lod: u8, region: &Options) -> Result<Vec<Body>, Error> {
    let ids = IdMap::open(&pack.section(Kind::IDMAP).ok_or(fail("no IDMAP"))?)?;
    let places = Places::open(&pack.section(Kind::PLACES).ok_or(fail("no PLACES"))?)?;
    let n = places.len();
    if keep.windows(2).any(|w| matches!(w, [a, b] if a >= b))
        || keep.iter().any(|&r| r as usize >= n)
    {
        return Err(fail("the kept rows are not increasing rows of the pack"));
    }
    let mut new_of = vec![u32::MAX; n];
    for (new, &old) in (0_u32..).zip(keep) {
        if let Some(slot) = new_of.get_mut(old as usize) {
            *slot = new;
        }
    }
    let row = |r: u32| r as usize;
    let k = keep.len() as u64;
    let mut out: Vec<Body> = Vec::new();

    let id_list = keep
        .iter()
        .map(|&r| ids.id(row(r)).ok_or(fail("a row without an ID")))
        .collect::<Result<Vec<_>, _>>()?;
    out.push((Kind::IDMAP, Encoding::TABLE, k, idmap::encode(&id_list)?));
    let (points, body) = places_of(&places, keep)?;
    out.push((Kind::PLACES, Encoding::TABLE, k, body));
    let node = match pack.section(Kind::PLACE_INDEX) {
        Some(s) => PlaceIndex::open(&s, places)?.node_size(),
        None => NODE,
    };
    out.push((
        Kind::PLACE_INDEX,
        Encoding::TABLE,
        k,
        place_index::encode(&points, node)?,
    ));
    if let Some(s) = pack.section(Kind::ELEVATION) {
        let h = Elevations::open(&s)?;
        let heights: Vec<Option<i16>> = keep.iter().map(|&r| h.get(row(r))).collect();
        if let Some(body) = elevation::encode(&heights)? {
            out.push((Kind::ELEVATION, Encoding::TABLE, k, body));
        }
    }
    if let Some(s) = pack.section(Kind::PLACE_TZ) {
        let tz = PlaceTz::open(&s)?;
        let zone_of: Vec<Option<u32>> = keep
            .iter()
            .map(|&r| {
                tz.get(row(r))
                    .and_then(|z| new_of.get(z as usize).copied())
                    .filter(|&z| z != u32::MAX)
            })
            .collect();
        if let Some(body) = place_tz::encode(&zone_of)? {
            out.push((Kind::PLACE_TZ, Encoding::TABLE, k, body));
        }
    }
    if let Some(s) = pack.section(Kind::NAMES) {
        out.push((
            Kind::NAMES,
            Encoding::TABLE,
            k,
            names_of(&Names::open(&s)?, keep)?,
        ));
    }
    if let Some(s) = pack.section(Kind::SEARCH) {
        let body = search_of(&Search::open(&s)?, keep, &new_of)?;
        out.push((Kind::SEARCH, Encoding::TABLE, k, body));
    }
    if let Some(s) = pack.section(Kind::HIERARCHY) {
        let h = Hierarchy::open(&s)?;
        // The nearest kept ancestor: the chain of a kept row through dropped rows.
        let parents: Vec<Option<u32>> = keep
            .iter()
            .map(|&r| {
                h.ancestors(row(r))
                    .map(|a| new_of.get(a).copied().unwrap_or(u32::MAX))
                    .find(|&a| a != u32::MAX)
            })
            .collect();
        if parents.iter().any(Option::is_some) {
            let body = hierarchy::encode(&parents)?;
            out.push((Kind::HIERARCHY, Encoding::TABLE, k, body));
        }
    }
    if let (Some(a), Some(p)) = (pack.section(Kind::ARCS), pack.section(Kind::POLYGONS)) {
        let node = match pack.section(Kind::POLY_INDEX) {
            Some(s) => PolyIndex::open(&s, &Polygons::open(&p)?)?.node_size(),
            None => NODE,
        };
        let geometry = Geometry {
            arcs: &Arcs::open(&a)?,
            polys: &Polygons::open(&p)?,
            count: p.item_count(),
        };
        out.extend(geometry.slice(&new_of, lod, node, region)?);
    }
    Ok(out)
}

/// The points and the PLACES body of rows `keep`.
fn places_of(places: &Places<'_>, keep: &[u32]) -> Result<(Vec<Point<E6>>, Vec<u8>), Error> {
    let col = |f: &dyn Fn(usize) -> Option<u32>| {
        keep.iter()
            .map(|&r| f(r as usize).ok_or(fail("a PLACES row is short")))
            .collect::<Result<Vec<u32>, Error>>()
    };
    let points: Vec<Point<E6>> = keep
        .iter()
        .map(|&r| {
            places
                .point(r as usize)
                .ok_or(fail("a row without a point"))
        })
        .collect::<Result<_, _>>()?;
    let kinds = keep
        .iter()
        .map(|&r| places.kind(r as usize).ok_or(fail("a row without a kind")))
        .collect::<Result<Vec<_>, _>>()?;
    let levels = keep
        .iter()
        .map(|&r| {
            places
                .admin_level(r as usize)
                .ok_or(fail("a row without a level"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let classes = col(&|r| places.class(r))?;
    let flags = col(&|r| places.flags(r))?;
    let body = places::encode(&PlaceColumns {
        points: &points,
        kinds: &kinds,
        admin_levels: &levels,
        classes: &classes,
        flags: &flags,
    })?;
    Ok((points, body))
}

/// NAMES of rows `keep`, every record as it was.
fn names_of(names: &Names<'_>, keep: &[u32]) -> Result<Vec<u8>, Error> {
    let mut buf = vec![0_u8; 1 << 16];
    let mut texts: Vec<Vec<Vec<u8>>> = Vec::with_capacity(keep.len());
    let mut meta = Vec::with_capacity(keep.len());
    for &r in keep {
        let (mut t, mut m) = (Vec::new(), Vec::new());
        for x in names.names(r as usize) {
            t.push(x.text(&mut buf)?.as_bytes().to_vec());
            m.push((x.lang(), x.script(), x.kind(), x.valid(), x.rule()));
        }
        texts.push(t);
        meta.push(m);
    }
    let recs: Vec<Vec<NameRecord<'_>>> = texts
        .iter()
        .zip(&meta)
        .map(|(t, m)| {
            t.iter()
                .zip(m)
                .map(|(text, &(lang, script, kind, valid, rule))| {
                    Ok(NameRecord {
                        text: core::str::from_utf8(text)
                            .map_err(|_| fail("a name is not UTF-8"))?,
                        lang,
                        script,
                        kind,
                        valid_from: valid.0,
                        valid_to: valid.1,
                        rule,
                    })
                })
                .collect::<Result<Vec<_>, Error>>()
        })
        .collect::<Result<_, _>>()?;
    let per_row: Vec<&[NameRecord<'_>]> = recs.iter().map(Vec::as_slice).collect();
    names::encode(&per_row)
}

/// SEARCH of rows `keep`: each key with its kept rows (a key left with none is dropped), each
/// row's importance and unit type, and the query rules.
fn search_of(s: &Search<'_>, keep: &[u32], new_of: &[u32]) -> Result<Vec<u8>, Error> {
    let mut keys: BTreeMap<Vec<u8>, BTreeSet<u32>> = BTreeMap::new();
    let mut buf = [0_u8; search::fst::MAX_KEY];
    for o in 0..s.keys() {
        let len = s.automaton().key(o, &mut buf).ok_or(fail("a SEARCH key"))?;
        let mut rows = BTreeSet::new();
        s.rows_of(o, |r| {
            if let Some(&n) = new_of.get(r as usize).filter(|&&n| n != u32::MAX) {
                rows.insert(n);
            }
            core::ops::ControlFlow::Continue(())
        });
        if !rows.is_empty() {
            keys.insert(buf.get(..len).ok_or(fail("a SEARCH key"))?.to_vec(), rows);
        }
    }
    let importance: Vec<u8> = keep.iter().map(|&r| s.importance(r)).collect();
    let mut units: Vec<u8> = keep.iter().map(|&r| s.unit(r)).collect();
    if units.iter().all(|&u| u == 0) {
        units.clear();
    }
    let rules: Vec<_> = s.rules().iter().collect();
    let extra = search::Extra {
        rules: (!rules.is_empty()).then_some(rules.as_slice()),
        units: &units,
    };
    search::encode_with(&keys, &importance, &extra)
}

/// Kept polygons renumbered onto the kept arcs: per polygon its parts' rings of arc references
/// (`arc << 1 | reversed`), its box, and the arcs it uses.
struct Renumbered {
    refs: Vec<Vec<Vec<Vec<u32>>>>,
    boxes: Vec<Rect>,
    arcs_per_polygon: Vec<Vec<u32>>,
}

/// POLYGONS of `input`, after moving each label the coarser outline no longer holds (a small
/// island's) to a point inside it.
fn labels_inside(
    input: &mut [PolygonInput<'_>],
    groups: &[u32],
    arcs: &Arcs<'_>,
) -> Result<Vec<u8>, Error> {
    let first = polygons::encode(input, groups)?;
    let check = Polygons::open(&crate::Section::unpacked(
        Kind::POLYGONS,
        input.len() as u64,
        &first,
    ))?;
    let mut moved = 0;
    for (i, poly) in input.iter_mut().enumerate() {
        if check.contains(arcs, i, poly.label) {
            continue;
        }
        poly.label = check
            .parts(i)
            .into_iter()
            .flatten()
            .find_map(|part| {
                let rings: Vec<Vec<Point<E7>>> = check
                    .rings(part)?
                    .map(|r| check.ring(arcs, r, 0).collect())
                    .collect();
                interior(&rings).filter(|&at| check.contains(arcs, i, at))
            })
            .ok_or(fail("no point inside a coarse polygon"))?;
        moved += 1;
    }
    if moved == 0 {
        Ok(first)
    } else {
        polygons::encode(input, groups)
    }
}

/// A point inside a part (its outer ring first, then holes): on a horizontal line through the
/// outer ring's box at 1/2, 1/4, 3/4, … of its height, the middle of the widest span between
/// crossings. `None` if no line finds one.
fn interior(rings: &[Vec<Point<E7>>]) -> Option<Point<E7>> {
    let outer = rings.first()?;
    let (lo, hi) = outer.iter().fold((i32::MAX, i32::MIN), |(lo, hi), v| {
        (lo.min(v.lat()), hi.max(v.lat()))
    });
    let h = i64::from(hi) - i64::from(lo);
    let mut xs: Vec<f64> = Vec::new();
    for k in [4_i64, 2, 6, 1, 3, 5, 7] {
        let y = i32::try_from(i64::from(lo) + h * k / 8).ok()?;
        xs.clear();
        for ring in rings {
            for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
                if (a.lat() > y) != (b.lat() > y) {
                    let t = f64::from(y - a.lat()) / f64::from(b.lat() - a.lat());
                    xs.push(f64::from(a.lon()) + t * f64::from(b.lon() - a.lon()));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        let best = xs
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[a, b]| (b - a, a.midpoint(b)))
            .max_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((width, mid)) = best
            && width >= 2.0
        {
            #[allow(clippy::cast_possible_truncation, reason = "between two longitudes")]
            return Point::<E7>::from_units(y, mid as i32);
        }
    }
    None
}

/// A pack's admin geometry.
struct Geometry<'g, 'a> {
    arcs: &'g Arcs<'a>,
    polys: &'g Polygons<'a>,
    count: u64,
}

impl Geometry<'_, '_> {
    /// The group of each polygon (one group per level, in order).
    fn groups(&self) -> Vec<usize> {
        let mut of = Vec::new();
        let mut g = 0;
        while let Some(range) = self.polys.group(g) {
            of.resize(of.len().max(range.end), g);
            for slot in of.get_mut(range).into_iter().flatten() {
                *slot = g;
            }
            g += 1;
        }
        of
    }

    /// Each kept polygon's parts, rings and arc references renumbered to `arc_new`, its box
    /// over the arcs' `points`, and the arcs it uses.
    fn renumber(
        &self,
        kept: &[(usize, u32)],
        arc_new: &BTreeMap<usize, u32>,
        points: &[Vec<Point<E7>>],
    ) -> Result<Renumbered, Error> {
        let mut out = Renumbered {
            refs: Vec::with_capacity(kept.len()),
            boxes: Vec::with_capacity(kept.len()),
            arcs_per_polygon: Vec::with_capacity(kept.len()),
        };
        for &(p, _) in kept {
            let mut parts = Vec::new();
            let mut b: Option<Rect> = None;
            let mut arcs_of = Vec::new();
            for part in self.polys.parts(p).ok_or(fail("a polygon's parts"))? {
                let mut rings = Vec::new();
                for ring in self.polys.rings(part).ok_or(fail("a part's rings"))? {
                    let mut list = Vec::new();
                    for (a, rev) in self.polys.refs(ring) {
                        let na = *arc_new.get(&a).ok_or(fail("an arc not collected"))?;
                        list.push(na << 1 | u32::from(rev));
                        arcs_of.push(na);
                        for v in points.get(na as usize).into_iter().flatten() {
                            let r = Rect::around_e7(v.lat(), v.lon());
                            match &mut b {
                                Some(b) => b.grow(&r),
                                None => b = Some(r),
                            }
                        }
                    }
                    rings.push(list);
                }
                parts.push(rings);
            }
            out.refs.push(parts);
            out.boxes.push(b.ok_or(fail("a polygon without vertices"))?);
            out.arcs_per_polygon.push(arcs_of);
        }
        Ok(out)
    }

    /// ARCS, POLYGONS, `POLY_INDEX` and `ADMIN_REGION` of the polygons on kept rows.
    fn slice(
        &self,
        new_of: &[u32],
        lod: u8,
        node: usize,
        opts: &Options,
    ) -> Result<Vec<Body>, Error> {
        let group_of = self.groups();
        // The kept polygons, in their order, and the arcs they use.
        let mut kept = Vec::new();
        let mut used = BTreeSet::new();
        for p in 0..usize::try_from(self.count).map_err(|_| fail("too many polygons"))? {
            let Some(row) = self.polys.row(p) else {
                continue;
            };
            let Some(&new) = new_of.get(row as usize).filter(|&&n| n != u32::MAX) else {
                continue;
            };
            for ring in self.polys.all_rings(p) {
                used.extend(self.polys.refs(ring).map(|(a, _)| a));
            }
            kept.push((p, new));
        }
        if kept.is_empty() {
            return Ok(Vec::new());
        }
        let arc_new: BTreeMap<usize, u32> = used.iter().copied().zip(0_u32..).collect();
        let points: Vec<Vec<Point<E7>>> = used
            .iter()
            .map(|&a| self.arcs.points(a, false, lod).collect())
            .collect();
        let Renumbered {
            refs,
            boxes,
            arcs_per_polygon,
        } = self.renumber(&kept, &arc_new, &points)?;
        let ring_refs: Vec<Vec<Vec<&[u32]>>> = refs
            .iter()
            .map(|p| {
                p.iter()
                    .map(|r| r.iter().map(Vec::as_slice).collect())
                    .collect()
            })
            .collect();
        let part_refs: Vec<Vec<&[&[u32]]>> = ring_refs
            .iter()
            .map(|p| p.iter().map(Vec::as_slice).collect())
            .collect();
        let mut input = kept
            .iter()
            .zip(&part_refs)
            .map(|(&(p, row), parts)| {
                Ok(PolygonInput {
                    row,
                    parts,
                    label: self
                        .polys
                        .label(p)
                        .ok_or(fail("a polygon without a label"))?,
                    area_ha: self.polys.area_ha(p).unwrap_or(0),
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let mut groups = Vec::new();
        for (i, &(p, _)) in kept.iter().enumerate() {
            let g = group_of.get(p).copied();
            if i == 0 || kept.get(i - 1).map(|&(q, _)| group_of.get(q).copied()) != Some(g) {
                groups.push(u32::try_from(i).map_err(|_| fail("too many polygons"))?);
            }
        }
        let arc_slices: Vec<&[Point<E7>]> = points.iter().map(Vec::as_slice).collect();
        let arcs_body = arcs::encode(&ArcColumns {
            arcs: &arc_slices,
            levels: &[],
            tolerances_cm: &[],
        })?;
        let lon_lat: Vec<Vec<[i32; 2]>> = points
            .iter()
            .map(|a| a.iter().map(|v| [v.lon(), v.lat()]).collect())
            .collect();
        // The region's run scales are the new arcs' own (a source on a coarse decimal grid has
        // them), so read them back from the encoded ARCS.
        let new_arcs = Arcs::open(&crate::Section::unpacked(
            Kind::ARCS,
            used.len() as u64,
            &arcs_body,
        ))?;
        let scales: Vec<u32> = if new_arcs.has_scale() {
            (0..new_arcs.len()).map(|a| new_arcs.scale(a)).collect()
        } else {
            Vec::new()
        };
        let admin = region::encode_on_arcs(&lon_lat, &scales, &arcs_per_polygon, opts)?;
        let polygons_body = labels_inside(&mut input, &groups, &new_arcs)?;
        let n = kept.len() as u64;
        Ok(vec![
            (Kind::ARCS, Encoding::TABLE, used.len() as u64, arcs_body),
            (Kind::POLYGONS, Encoding::TABLE, n, polygons_body),
            (
                Kind::POLY_INDEX,
                Encoding::TABLE,
                n,
                poly_index::encode(&boxes, node)?,
            ),
            (Kind::ADMIN_REGION, Encoding::TABLE, n, admin),
        ])
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use super::{E7, Point, interior};
    use alloc::vec::Vec;

    fn ring(v: &[(i32, i32)]) -> Vec<Point<E7>> {
        v.iter()
            .map(|&(x, y)| Point::<E7>::from_units(y, x).unwrap())
            .collect()
    }

    #[test]
    fn an_interior_point_avoids_the_hole() {
        // A 100 × 100 square with a hole over its middle band, x 10 to 90.
        let outer = ring(&[(0, 0), (100, 0), (100, 100), (0, 100)]);
        let hole = ring(&[(10, 40), (10, 60), (90, 60), (90, 40)]);
        let p = interior(&[outer.clone(), hole]).unwrap();
        // Half-way up is in the hole's band: the widest span there is outside the hole.
        assert!(
            p.lon() < 10 || p.lon() > 90 || !(40..=60).contains(&p.lat()),
            "{p:?}"
        );
        assert!((0..=100).contains(&p.lon()) && (0..=100).contains(&p.lat()));
        // Without the hole: the middle.
        let q = interior(&[outer]).unwrap();
        assert_eq!((q.lon(), q.lat()), (50, 50));
        // A degenerate ring has none.
        assert!(interior(&[ring(&[(0, 0), (5, 0), (9, 0)])]).is_none());
    }
}
