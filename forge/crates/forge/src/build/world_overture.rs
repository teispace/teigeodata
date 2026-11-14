//! The main build's first-level units and country outlines from Overture's divisions (D-155).
//!
//! 1. **Read** the land areas of every country, dependency and region, and each division's
//!    Wikidata item and OSM relation ([`overture`]).
//! 2. **Fold:** a dependency the point of view draws inside its sovereign (no country of the
//!    view has its code: Mayotte, Réunion, Christmas Island) is one unit of that sovereign; its
//!    own regions are dropped.
//! 3. **Partition and own:** the world's land as disjoint units ([`overture::land_units`]),
//!    owned by the point of view ([`own`]).
//! 4. **Pack countries** take their slice's outline (D-154): every other piece is cut by it,
//!    and their own pieces outside it go to the neighbour sharing the most border.
//! 5. **Thin** the whole partition to [`THIN_LOD`] as shared arcs ([`thin`]), the pack
//!    outlines pinned; each country is then its pieces dissolved.
//!
//! A region (or a folded dependency) is a unit of the country holding most of it, keyed
//! `gers:<division ID>` with its Wikidata item and OSM relation; its parts in other countries,
//! and every country's land no region holds, are fillers. Natural Earth supplies the
//! countries' rows (codes, names, population); a feature that owns no land has no row.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rayon::prelude::*;

use super::world::{Partition, WorldUnit, pack_outlines};
use crate::lock::Lock;
use crate::read::natural_earth::Country;
use crate::read::overture::{self, Subtype};
use crate::tiles;
use crate::topo::arcs::V;
use crate::topo::pov::{self, Input, own};
use crate::topo::thin;

/// The level the world's land is thinned to before the topology stage: 3, 100 m. The world
/// part ships at 1 km (D-144), so 100 m loses nothing it keeps, and the stage works on 3.6 M
/// vertices instead of 31 M (measured 2026-11-10, the research note §3.5).
const THIN_LOD: u8 = 3;

/// A polygon's rings.
type Rings = Vec<Vec<V>>;

/// A piece of the world's land before thinning.
struct Piece {
    /// Its area's index; `None` for a pack country's outline or a gap it gave away.
    area: Option<usize>,
    country: usize,
    /// Part of its area's unit row.
    row: bool,
    /// A pack country's outline: every vertex kept.
    pinned: bool,
    rings: Vec<Vec<V>>,
}

/// The units, fillers and pinned countries, and each country's outline set to its pieces
/// dissolved (`countries` keeps only the features that own land, in order).
pub(super) fn partition(
    root: &Path,
    lock: &Lock,
    source: &str,
    packs: &[String],
    build: &str,
    countries: &mut Vec<Country>,
    islets: &Islets<'_>,
) -> Result<Partition, String> {
    let start = Instant::now();
    let manifest = tiles::manifest(root, lock.get(source)?)?;
    let parts_of = |prefix: &str| -> Result<Vec<PathBuf>, String> {
        manifest
            .iter()
            .filter(|t| t.name.starts_with(prefix))
            .map(|t| {
                tiles::tile_path(root, t).ok_or_else(|| {
                    format!(
                        "{source}: tile {} is not stored (forge fetch {source})",
                        t.name
                    )
                })
            })
            .collect()
    };
    let mut areas = overture::read_areas(&parts_of("area-")?)?;
    let divisions = overture::read_divisions(&parts_of("division-")?)?;
    let mut summary = String::new();
    let folded = fold(&mut areas, countries, source, &mut summary);
    let pieces = owned_pieces(&mut areas, &folded, countries, &mut summary)?;
    let pieces = without_islets(pieces, islets, &mut summary);
    let outlines = pack_outlines(root, packs, build, countries)?;
    let pieces = take_outlines(pieces, &outlines, &mut summary);
    let pieces = thinned(pieces, &mut summary)?;
    let index = dissolve_countries(&pieces, countries, &mut summary);
    let mut units: BTreeMap<usize, (WorldUnit, Vec<Rings>)> = BTreeMap::new();
    let mut fills = Vec::new();
    for p in pieces {
        let country = index
            .get(p.country)
            .copied()
            .flatten()
            .ok_or("a piece of a country with no land")?;
        let Some(a) = p.area.filter(|_| p.row) else {
            fills.push((country, p.rings));
            continue;
        };
        let area = areas.get(a).ok_or("an area out of range")?;
        units
            .entry(a)
            .or_insert_with(|| {
                let d = divisions.get(&area.division);
                let mut refs: Vec<String> = d
                    .and_then(|d| d.wikidata.as_ref())
                    .map(|q| format!("wikidata:{q}"))
                    .into_iter()
                    .collect();
                refs.extend(
                    d.and_then(|d| d.osm_relation)
                        .map(|r| format!("osm:relation/{r}")),
                );
                let unit = WorldUnit {
                    key: format!("gers:{}", area.division),
                    refs,
                    iso: None,
                    names: super::world::unit_names(&area.names),
                    label: None,
                    country,
                    rings: Vec::new(),
                };
                (unit, Vec::new())
            })
            .1
            .push(p.rings);
    }
    // A unit of several pieces is their dissolve, as its country is: their shared edges go.
    let units: Vec<WorldUnit> = units
        .into_par_iter()
        .map(|(_, (mut u, mut sets))| {
            u.rings = if sets.len() == 1 {
                sets.pop().unwrap_or_default()
            } else {
                let refs: Vec<&[Vec<V>]> = sets.iter().map(Vec::as_slice).collect();
                pov::dissolve(&refs)
            };
            u
        })
        .collect();
    let _ = writeln!(
        summary,
        "- {} units, {} filler pieces, in {:.1?}.",
        units.len(),
        fills.len(),
        start.elapsed()
    );
    Ok(Partition {
        units,
        fills,
        pinned: pack_indices(packs, countries),
        report: summary.trim_end().to_string(),
    })
}

/// Which islets the world part leaves out: parts of land smaller than `min_km2`, unless a
/// part is its area's largest (no unit or island country vanishes) or holds one of `keep`.
pub(super) struct Islets<'a> {
    pub(super) min_km2: f64,
    /// The cities' points: a part holding one stays.
    pub(super) keep: &'a [V],
}

/// Step 3b: the islets dropped ([`Islets`]). OSM maps about 62,000 of the world's 80,000
/// land parts under 1 km², each an arc and region entries at any tolerance, where Natural
/// Earth keeps 300 (measured 2026-11-10, the research note §3.7).
fn without_islets(pieces: Vec<Piece>, islets: &Islets<'_>, summary: &mut String) -> Vec<Piece> {
    if islets.min_km2 <= 0.0 {
        return pieces;
    }
    let mut keep: Vec<V> = islets.keep.to_vec();
    keep.sort_unstable();
    // Each piece's parts with their areas; then the largest part of every area.
    let parts: Vec<Vec<(f64, Rings)>> = pieces
        .par_iter()
        .map(|p| {
            pov::normal(&p.rings)
                .into_iter()
                .map(|r| (km2(&r), r))
                .collect()
        })
        .collect();
    let mut largest: BTreeMap<usize, (usize, usize, f64)> = BTreeMap::new();
    for (i, (p, ps)) in pieces.iter().zip(&parts).enumerate() {
        let Some(a) = p.area else { continue };
        for (j, (km, _)) in ps.iter().enumerate() {
            let e = largest.entry(a).or_insert((i, j, -1.0));
            if *km > e.2 {
                *e = (i, j, *km);
            }
        }
    }
    let (mut dropped, mut dropped_km2, mut by_city) = (0_usize, 0.0_f64, 0_usize);
    let mut out = Vec::with_capacity(pieces.len());
    for (i, (mut p, ps)) in pieces.into_iter().zip(parts).enumerate() {
        let mut rings = Vec::new();
        for (j, (km, r)) in ps.into_iter().enumerate() {
            let main = p
                .area
                .and_then(|a| largest.get(&a))
                .is_some_and(|&(li, lj, _)| (li, lj) == (i, j));
            let small = km < islets.min_km2 && !main;
            if small && holds_any(&r, &keep) {
                by_city += 1;
            } else if small {
                dropped += 1;
                dropped_km2 += km;
                continue;
            }
            rings.extend(r);
        }
        if rings.is_empty() {
            continue;
        }
        p.rings = rings;
        out.push(p);
    }
    let _ = writeln!(
        summary,
        "- Islets under {} km² left out (not their area's largest part, no city): {dropped} \
         parts, {dropped_km2:.0} km²; {by_city} kept for a city.",
        islets.min_km2
    );
    out
}

/// Whether a part (outer ring first) holds any of the sorted points.
fn holds_any(part: &[Vec<V>], sorted: &[V]) -> bool {
    let b = pov::bbox(part);
    let from = sorted.partition_point(|v| v.0 < b[0]);
    let polygon = crate::geom::Polygon {
        rings: part.to_vec(),
    };
    sorted
        .get(from..)
        .unwrap_or_default()
        .iter()
        .take_while(|v| v.0 <= b[2])
        .any(|v| v.1 >= b[1] && v.1 <= b[3] && polygon.contains(v.0, v.1))
}

/// Step 2: the dependencies no country of the view has the code of lose their regions; returns
/// their codes.
fn fold(
    areas: &mut Vec<overture::Area>,
    countries: &[Country],
    source: &str,
    summary: &mut String,
) -> BTreeSet<String> {
    let held: BTreeSet<&str> = countries.iter().filter_map(|c| c.iso2.as_deref()).collect();
    let folded: BTreeSet<String> = areas
        .iter()
        .filter(|a| a.subtype == Subtype::Dependency)
        .map(|a| a.country.clone())
        .filter(|cc| !held.contains(cc.to_ascii_lowercase().as_str()))
        .collect();
    let before = areas.len();
    areas.retain(|a| !(a.subtype == Subtype::Region && folded.contains(&a.country)));
    let _ = writeln!(
        summary,
        "- First-level units ({source}, D-155): {} land areas; {} dependencies the view draws \
         inside their sovereign are one unit each ({} regions of theirs dropped).",
        areas.len(),
        folded.len(),
        before - areas.len(),
    );
    folded
}

/// Step 3: the land partition owned by the view, each piece marked when it is part of its
/// area's unit row (a region's or a folded dependency's, in the country holding most of it).
/// The areas' geometry is released.
fn owned_pieces(
    areas: &mut [overture::Area],
    folded: &BTreeSet<String>,
    countries: &[Country],
    summary: &mut String,
) -> Result<Vec<Piece>, String> {
    let land = overture::land_units(areas)?;
    for a in areas.iter_mut() {
        a.polygon.rings = Vec::new();
    }
    let codes: Vec<String> = land
        .iter()
        .map(|u| {
            areas
                .get(u.area)
                .map(|a| a.country.to_ascii_lowercase())
                .unwrap_or_default()
        })
        .collect();
    let owned = {
        let view: Vec<Input<'_>> = countries
            .iter()
            .map(|c| Input {
                code: &c.code,
                rings: &c.polygon.rings,
            })
            .collect();
        let iso2: Vec<Option<&str>> = countries.iter().map(|c| c.iso2.as_deref()).collect();
        let units: Vec<Input<'_>> = land
            .iter()
            .zip(&codes)
            .map(|(u, cc)| Input {
                code: cc,
                rings: &u.rings,
            })
            .collect();
        own::own(&view, &iso2, &units, own::WIDTH)
    };
    let area_of = |u: usize| land.get(u).map_or(usize::MAX, |l| l.area);
    report_owned(&owned, areas, countries, &area_of, land.len(), summary);
    // A row area's country: the one holding most of its pieces.
    let rows_of = |area: usize| {
        areas
            .get(area)
            .is_some_and(|a| a.subtype == Subtype::Region || folded.contains(&a.country))
    };
    let mut shares: BTreeMap<usize, BTreeMap<usize, f64>> = BTreeMap::new();
    for p in &owned.pieces {
        let a = area_of(p.unit);
        if rows_of(a) {
            *shares.entry(a).or_default().entry(p.country).or_default() += km2(&p.rings);
        }
    }
    let main: BTreeMap<usize, usize> = shares
        .into_iter()
        .filter_map(|(a, by)| {
            by.into_iter()
                .max_by(|x, y| x.1.total_cmp(&y.1).then(y.0.cmp(&x.0)))
                .map(|(k, _)| (a, k))
        })
        .collect();
    Ok(owned
        .pieces
        .into_iter()
        .map(|p| {
            let a = area_of(p.unit);
            Piece {
                area: Some(a),
                country: p.country,
                row: main.get(&a) == Some(&p.country),
                pinned: false,
                rings: p.rings,
            }
        })
        .collect())
}

/// The summary's lines on the ownership: how each piece got its country, what the view
/// moved, what no country holds and what lies offshore.
fn report_owned(
    owned: &own::Ownership,
    areas: &[overture::Area],
    countries: &[Country],
    area_of: &dyn Fn(usize) -> usize,
    pieces: usize,
    summary: &mut String,
) {
    let name = |unit: usize| {
        areas.get(area_of(unit)).map_or_else(String::new, |a| {
            let n = a.names.first().map_or("", |n| n.1.as_str());
            format!("{} {n}", a.country)
        })
    };
    let feature = |k: usize| countries.get(k).map_or("?", |c| c.code.as_str());
    let by = |b: own::By| owned.pieces.iter().filter(|p| p.by == b).count();
    let list = |v: Vec<String>| {
        if v.is_empty() {
            "none".to_string()
        } else {
            v.join("; ")
        }
    };
    let _ = writeln!(
        summary,
        "  - {pieces} pieces of land: {} owned by their code, {} by area, {} where their \
         code's other units went; unowned (no country of the view holds any): {}.",
        by(own::By::Code),
        by(own::By::Area),
        by(own::By::Kin),
        list(owned.unowned.iter().map(|&u| name(u)).collect()),
    );
    let _ = writeln!(
        summary,
        "  - Moved by the view (parts at least {:.0} km wide): {}.",
        own_width_km(),
        list(
            owned
                .pieces
                .iter()
                .filter(|p| p.moved)
                .map(|p| format!(
                    "{} → {} ({:.0} km²)",
                    name(p.unit),
                    feature(p.country),
                    km2(&p.rings)
                ))
                .collect()
        )
    );
    let _ = writeln!(
        summary,
        "  - At least 1,000 km² kept by their code but mostly outside their country's \
         outline: {}.",
        list(
            owned
                .offshore
                .iter()
                .map(|&(u, k, s)| format!(
                    "{} in {} ({:.0}% inside)",
                    name(u),
                    feature(k),
                    s * 100.0
                ))
                .collect()
        )
    );
}

/// Step 4: each pack country takes its slice's outline (D-154): every other piece is cut by
/// it, and its own pieces' parts outside it go to the neighbour sharing the most border.
fn take_outlines(
    mut pieces: Vec<Piece>,
    outlines: &[pov::Outline],
    summary: &mut String,
) -> Vec<Piece> {
    let (mut replaced, mut given, mut left) = (0, 0, 0);
    for (k, outline) in outlines {
        let (own_pieces, rest): (Vec<Piece>, Vec<Piece>) =
            pieces.into_iter().partition(|p| p.country == *k);
        let clip: [&[Vec<V>]; 1] = [outline];
        let bounds = pov::bbox(outline);
        pieces = rest
            .into_par_iter()
            .flat_map_iter(|mut p| {
                if pov::meets(&pov::bbox(&p.rings), &bounds)
                    && let Some(parts) = pov::minus(&p.rings, &clip, pov::MIN_WIDTH)
                {
                    p.rings = parts.into_iter().flatten().collect();
                }
                (!p.rings.is_empty()).then_some(p)
            })
            .collect();
        replaced += own_pieces.len();
        let gaps: Vec<Vec<Vec<V>>> = own_pieces
            .par_iter()
            .flat_map_iter(|p| {
                pov::minus(&p.rings, &clip, pov::MIN_WIDTH).unwrap_or_else(|| vec![p.rings.clone()])
            })
            .collect();
        for gap in gaps {
            let gb = pov::bbox(&gap);
            let best = pov::best_neighbour(
                &gap,
                pieces
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| pov::meets(&pov::bbox(&p.rings), &gb))
                    .map(|(i, p)| (i, p.rings.as_slice())),
            );
            if let Some(country) = best.and_then(|i| pieces.get(i)).map(|n| n.country) {
                pieces.push(Piece {
                    area: None,
                    country,
                    row: false,
                    pinned: false,
                    rings: gap,
                });
                given += 1;
            } else {
                left += 1;
            }
        }
        pieces.push(Piece {
            area: None,
            country: *k,
            row: false,
            pinned: true,
            rings: outline.clone(),
        });
    }
    let _ = writeln!(
        summary,
        "- Pack outlines (D-154): {} pack countries drawn by their slice ({replaced} of their \
         pieces replaced), {given} parts outside it given to the neighbour sharing the most \
         border, {left} touching none.",
        outlines.len()
    );
    pieces
}

/// Step 5: the pieces thinned to [`THIN_LOD`] as one partition and normalised; a piece that
/// leaves empty is dropped.
///
/// # Errors
/// Overlapping pieces, or a crossing the guard could not remove.
fn thinned(mut pieces: Vec<Piece>, summary: &mut String) -> Result<Vec<Piece>, String> {
    let rings: Vec<&[Vec<V>]> = pieces.iter().map(|p| p.rings.as_slice()).collect();
    let pinned: Vec<bool> = pieces.iter().map(|p| p.pinned).collect();
    let (out, t) = thin::partition(&rings, &pinned, THIN_LOD)?;
    let _ = writeln!(
        summary,
        "- Thinned to level {THIN_LOD} as one partition: {} → {} vertices, {} arcs; snap \
         rounding of {} crossing or touching edge pairs ({} vertices added, {} rings \
         collapsed), {} folds undone; the guard left {}.",
        t.vertices.0,
        t.vertices.1,
        t.arcs,
        t.snapped.bad_pairs,
        t.snapped.vertices_added,
        t.snapped.rings_dropped,
        t.folds,
        t.guard.left
    );
    if t.guard.left > 0 {
        return Err(format!("thinning left {} crossings", t.guard.left));
    }
    // Normalised as the countries' dissolve reads them, without the parts thinning pressed
    // narrower than noise ([`pov::MIN_WIDTH`]: a sliver between a pack's outline and its
    // neighbour's), so each country is exactly its pieces (G-topo) and every part has an
    // inside for its label.
    pieces.par_iter_mut().zip(out).for_each(|(p, rings)| {
        let parts: Vec<Vec<V>> = pov::normal_wide(&rings, pov::MIN_WIDTH)
            .into_iter()
            .flatten()
            .collect();
        p.rings = crate::topo::arcs::clean_rings(&parts);
    });
    pieces.retain(|p| !p.rings.is_empty());
    let nested = unnest(&mut pieces);
    let _ = writeln!(
        summary,
        "  - {nested} pieces cut by a smaller one of their country thinning left inside them (an \
         islet whose lake collapsed below 100 m)."
    );
    Ok(pieces)
}

/// Thinning can collapse a lake under [`THIN_LOD`]'s tolerance while keeping the islet inside
/// it, a separate piece (Diego Ramírez's, 2026-11-10): the two then overlap. Each country's
/// pieces are checked with one overlay ([`pov::overlapping`]); in a country that overlaps, a
/// piece is cut by every smaller one inside it, so the islet stays and its lake comes back as
/// the islet's outline. The pieces' shared edges are untouched: the cut only adds a ring.
/// Returns how many pieces were cut.
fn unnest(pieces: &mut [Piece]) -> usize {
    let mut by_country: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, p) in pieces.iter().enumerate() {
        by_country.entry(p.country).or_default().push(i);
    }
    let fixes: Vec<(usize, Vec<Vec<V>>)> = by_country
        .into_par_iter()
        .flat_map_iter(|(_, members)| {
            let sets: Vec<&[Vec<V>]> = members
                .iter()
                .filter_map(|&i| pieces.get(i))
                .map(|p| p.rings.as_slice())
                .collect();
            if members.len() < 2 || !pov::overlapping(&sets) {
                return Vec::new();
            }
            let size: Vec<f64> = sets.iter().map(|r| km2(r)).collect();
            let boxes: Vec<[i32; 4]> = sets.iter().map(|r| pov::bbox(r)).collect();
            (0..sets.len())
                .filter_map(|x| {
                    let (own, b) = (sets.get(x)?, boxes.get(x)?);
                    let clips: Vec<&[Vec<V>]> = (0..sets.len())
                        .filter(|&y| {
                            size.get(y) < size.get(x) || (size.get(y) == size.get(x) && y > x)
                        })
                        .filter(|&y| boxes.get(y).is_some_and(|o| pov::meets(o, b)))
                        .filter_map(|y| sets.get(y).copied())
                        .filter(|r| !pov::common(own, r, 0).is_empty())
                        .collect();
                    if clips.is_empty() {
                        return None;
                    }
                    let parts = pov::minus(own, &clips, 0)?;
                    Some((*members.get(x)?, parts.into_iter().flatten().collect()))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let n = fixes.len();
    for (i, rings) in fixes {
        if let Some(p) = pieces.get_mut(i) {
            p.rings = crate::topo::arcs::clean_rings(&rings);
        }
    }
    n
}

/// Each country's outline set to its pieces dissolved; the features with none are dropped.
/// Returns each old index's new one.
fn dissolve_countries(
    pieces: &[Piece],
    countries: &mut Vec<Country>,
    summary: &mut String,
) -> Vec<Option<usize>> {
    let mut of: Vec<Vec<&[Vec<V>]>> = vec![Vec::new(); countries.len()];
    for p in pieces {
        if let Some(v) = of.get_mut(p.country) {
            v.push(&p.rings);
        }
    }
    let outlines: Vec<Vec<Vec<V>>> = of.par_iter().map(|sets| pov::dissolve(sets)).collect();
    let mut index = Vec::with_capacity(countries.len());
    let mut next = 0;
    for o in &outlines {
        index.push((!o.is_empty()).then_some(next));
        next += usize::from(!o.is_empty());
    }
    let mut landless = Vec::new();
    let all = std::mem::take(countries);
    for (mut c, o) in all.into_iter().zip(outlines) {
        if o.is_empty() {
            landless.push(c.code);
        } else {
            c.polygon.rings = o;
            countries.push(c);
        }
    }
    let _ = writeln!(
        summary,
        "- {} Natural Earth features own no land and have no row: {}.",
        landless.len(),
        if landless.is_empty() {
            "none".to_string()
        } else {
            landless.join(", ")
        }
    );
    index
}

/// The width the view's parts must reach to move, km.
fn own_width_km() -> f64 {
    #[allow(clippy::cast_precision_loss, reason = "a constant far below 2^52")]
    let w = own::WIDTH as f64;
    w * 111.195_08e-7
}

/// The geodesic area of rings read even–odd, km².
fn km2(rings: &[Vec<V>]) -> f64 {
    rings
        .iter()
        .map(|r| crate::topo::measure::ring_area_m2(r))
        .sum::<f64>()
        .abs()
        / 1e6
}

/// The pack countries' indices among `countries` (by ISO code).
fn pack_indices(packs: &[String], countries: &[Country]) -> Vec<usize> {
    packs
        .iter()
        .filter_map(|cc| {
            countries
                .iter()
                .position(|c| c.iso2.as_deref() == Some(cc.as_str()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A counter-clockwise square of side `s` grid units at (x, y).
    fn square(x: i32, y: i32, s: i32) -> Vec<V> {
        vec![(x, y), (x + s, y), (x + s, y + s), (x, y + s), (x, y)]
    }

    fn piece(area: usize, rings: Rings) -> Piece {
        Piece {
            area: Some(area),
            country: 0,
            row: true,
            pinned: false,
            rings,
        }
    }

    #[test]
    fn islets_go_unless_largest_or_holding_a_city() {
        // Area 0: a 0.1° island (about 120 km²), a bare islet and an islet with a city (each
        // about 0.3 km²). Area 1: one islet only, its largest part.
        let (big, islet) = (1_000_000, 50_000);
        let pieces = vec![
            piece(
                0,
                vec![
                    square(0, 0, big),
                    square(2_000_000, 0, islet),
                    square(3_000_000, 0, islet),
                ],
            ),
            piece(1, vec![square(5_000_000, 0, islet)]),
        ];
        let city = [(3_000_000 + islet / 2, islet / 2)];
        let islets = Islets {
            min_km2: 1.0,
            keep: &city,
        };
        let mut summary = String::new();
        let out = without_islets(pieces, &islets, &mut summary);
        let boxes: Vec<Vec<[i32; 4]>> = out
            .iter()
            .map(|p| pov::normal(&p.rings).iter().map(|r| pov::bbox(r)).collect())
            .collect();
        assert_eq!(boxes.len(), 2, "{summary}");
        assert_eq!(
            boxes[0],
            [[0, 0, big, big], [3_000_000, 0, 3_000_000 + islet, islet]],
            "the bare islet goes, the city's stays"
        );
        assert_eq!(boxes[1], [[5_000_000, 0, 5_000_000 + islet, islet]]);
        assert!(summary.contains("1 parts"), "{summary}");
        // Off: nothing changes.
        let off = Islets {
            min_km2: 0.0,
            keep: &[],
        };
        let kept = without_islets(
            vec![piece(0, vec![square(0, 0, islet)])],
            &off,
            &mut summary,
        );
        assert_eq!(kept.len(), 1);
    }
}
