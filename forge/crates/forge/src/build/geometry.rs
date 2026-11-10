//! Admin geometry in the pack (B10): the `topology` stage's ARCS payload and its polygons,
//! linked to their PLACES rows as POLYGONS, indexed by `POLY_INDEX`, and read back and checked.

#![allow(
    clippy::indexing_slicing,
    reason = "indices come from the same vectors (ring positions, arc vertices, segment ids): an out-of-range index is a forge bug and must stop the build, not be skipped"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use teigeodata_core::{E7, PlaceKind, Point};
use teigeodata_format::arcs::{self, Arcs};
use teigeodata_format::poly_index::{self, PolyIndex, Rect};
use teigeodata_format::polygons::{self, PolygonInput, Polygons};
use teigeodata_format::region::{self, Options, Region};
use teigeodata_format::{Encoding, Kind, Pack, Section, Writer, places};

use super::Place;
use super::pack::Body;
use crate::topo::{self, arcs::Parts};

/// One polygon of the `topology` stage.
struct Poly {
    level: u8,
    pcode: String,
    label: (i32, i32),
    area_ha: u32,
    /// West, south, east, north on the 1e-7° grid.
    bbox: [i32; 4],
    parts: Parts,
}

/// The `topology` stage's output for a boundary source.
pub(crate) struct Geometry {
    arcs: Vec<u8>,
    arc_count: u64,
    polys: Vec<Poly>,
}

/// Reads `data/build/topology/<admin>/`.
pub(crate) fn load(root: &Path, admin: &str) -> Result<Geometry, String> {
    let dir = root.join("data/build/topology").join(admin);
    let arcs = fs::read(dir.join("arcs.bin"))
        .map_err(|e| format!("{}: {e} (run `forge topology {admin}`)", dir.display()))?;
    let arc_count = teigeodata_core::table::Table::parse(&arcs)
        .and_then(|t| t.column(arcs::BASE_X))
        .map_err(|e| format!("arcs.bin: {e:?}"))?
        .len() as u64;
    let text = fs::read_to_string(dir.join("polygons.tsv")).map_err(|e| e.to_string())?;
    let mut lines = text.lines();
    if lines.next() != Some(topo::HEADER) {
        return Err("polygons.tsv: unexpected header".into());
    }
    let polys = lines
        .enumerate()
        .map(|(i, l)| {
            let f: Vec<&str> = l.split('\t').collect();
            let bad = || format!("polygons.tsv line {}: malformed", i + 2);
            let [level, pcode, x, y, ha, west, south, east, north, parts] = f[..] else {
                return Err(bad());
            };
            let num = |v: &str| v.parse::<i32>().map_err(|_| bad());
            Ok(Poly {
                level: level.parse().map_err(|_| bad())?,
                pcode: pcode.to_string(),
                label: (x.parse().map_err(|_| bad())?, y.parse().map_err(|_| bad())?),
                area_ha: ha.parse().map_err(|_| bad())?,
                bbox: [num(west)?, num(south)?, num(east)?, num(north)?],
                parts: topo::parse_parts(parts)?,
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(Geometry {
        arcs,
        arc_count,
        polys,
    })
}

/// The key of the place a polygon belongs to: the country's for level 0, else the admin unit's.
fn key<'a>(p: &Poly, country: Option<&'a str>) -> Result<std::borrow::Cow<'a, str>, String> {
    if p.level == 0 {
        country
            .map(std::borrow::Cow::Borrowed)
            .ok_or_else(|| "a country polygon but no country place".to_string())
    } else {
        Ok(std::borrow::Cow::Owned(format!("admin:{}", p.pcode)))
    }
}

/// The ARCS and POLYGONS sections, polygons in (level, row) order, one group per level.
pub(super) fn sections(
    g: &Geometry,
    rows: &[&Place],
    row_of: &BTreeMap<&str, u32>,
) -> Result<Vec<Body>, String> {
    let country = rows
        .iter()
        .find(|p| p.kind == PlaceKind::Country)
        .map(|p| p.key.as_str());
    let mut linked: Vec<(u8, u32, &Poly)> = g
        .polys
        .iter()
        .map(|p| {
            let k = key(p, country)?;
            let row = row_of
                .get(k.as_ref())
                .copied()
                .ok_or_else(|| format!("{k}: a polygon without a place"))?;
            Ok((p.level, row, p))
        })
        .collect::<Result<_, String>>()?;
    linked.sort_by_key(|&(level, row, _)| (level, row));
    encode(g, &linked)
}

/// The sections of polygons coded by `pcode` (one level), each linked to the row `row_of`
/// gives it; a polygon without a row is an error (D-147).
pub(super) fn sections_by_code(
    g: &Geometry,
    row_of: &BTreeMap<String, u32>,
) -> Result<Vec<Body>, String> {
    let mut linked: Vec<(u8, u32, &Poly)> = g
        .polys
        .iter()
        .map(|p| {
            row_of
                .get(&p.pcode)
                .map(|&row| (p.level, row, p))
                .ok_or_else(|| format!("{}: a polygon without a place", p.pcode))
        })
        .collect::<Result<_, String>>()?;
    linked.sort_by_key(|&(level, row, _)| (level, row));
    encode(g, &linked)
}

impl Geometry {
    /// Each polygon's label point (lon, lat on the 1e-7° grid), by code.
    pub(super) fn labels(&self) -> BTreeMap<&str, (i32, i32)> {
        self.polys
            .iter()
            .map(|p| (p.pcode.as_str(), p.label))
            .collect()
    }
}

/// The sections of linked polygons, `(level, row, polygon)` in (level, row) order.
fn encode(g: &Geometry, linked: &[(u8, u32, &Poly)]) -> Result<Vec<Body>, String> {
    let mut groups = Vec::new();
    for (i, w) in linked.iter().enumerate() {
        if i == 0 || linked[i - 1].0 != w.0 {
            groups.push(u32::try_from(i).map_err(|e| e.to_string())?);
        }
    }
    // Borrowed views for the encoder.
    let rings: Vec<Vec<Vec<&[u32]>>> = linked
        .iter()
        .map(|(_, _, p)| {
            p.parts
                .iter()
                .map(|part| part.iter().map(Vec::as_slice).collect())
                .collect()
        })
        .collect();
    let parts: Vec<Vec<&[&[u32]]>> = rings
        .iter()
        .map(|p| p.iter().map(Vec::as_slice).collect())
        .collect();
    let input: Vec<PolygonInput<'_>> = linked
        .iter()
        .zip(&parts)
        .map(|((_, row, p), parts)| {
            let label =
                Point::<E7>::from_units(p.label.1, p.label.0).ok_or("a label off the grid")?;
            Ok(PolygonInput {
                row: *row,
                parts,
                label,
                area_ha: p.area_ha,
            })
        })
        .collect::<Result<_, String>>()?;
    let body = polygons::encode(&input, &groups).map_err(|e| e.to_string())?;
    // POLY_INDEX over the boxes in POLYGONS order; `verify` recomputes them from the arcs.
    let boxes: Vec<Rect> = linked
        .iter()
        .map(|(_, _, p)| {
            let [west, south, east, north] = p.bbox;
            let mut b = Rect::around_e7(south, west);
            b.grow(&Rect::around_e7(north, east));
            b
        })
        .collect();
    let index = poly_index::encode(&boxes, super::pack::NODE).map_err(|e| e.to_string())?;
    let admin = admin_region(g, linked)?;
    let n = input.len() as u64;
    let mut out = vec![
        (Kind::ARCS, Encoding::TABLE, g.arc_count, g.arcs.clone()),
        (Kind::POLYGONS, Encoding::TABLE, n, body),
        (Kind::POLY_INDEX, Encoding::TABLE, n, index),
        (Kind::ADMIN_REGION, Encoding::TABLE, n, admin),
    ];
    // ARCS_OVERVIEW (D-102): the coarse levels alone, when the arcs have levels.
    let arcs = Arcs::open(&Section::unpacked(Kind::ARCS, g.arc_count, &g.arcs))
        .map_err(|e| e.to_string())?;
    if arcs.has_levels() {
        let body = arcs::overview(&arcs, OVERVIEW_FROM).map_err(|e| e.to_string())?;
        out.push((Kind::ARCS_OVERVIEW, Encoding::TABLE, g.arc_count, body));
    }
    Ok(out)
}

/// The level `ARCS_OVERVIEW` starts at: 500 m and coarser (B10: 3.6% of ARCS's bytes, reads at
/// levels 4 to 6 four times faster).
const OVERVIEW_FROM: u8 = 4;

/// `ADMIN_REGION` (D-100): REGION's index on the ARCS vertices, region `i` = polygon `i` of
/// POLYGONS, with D-083's default shape.
fn admin_region(g: &Geometry, linked: &[(u8, u32, &Poly)]) -> Result<Vec<u8>, String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let arcs = Arcs::open(&Section::unpacked(Kind::ARCS, g.arc_count, &g.arcs)).map_err(e)?;
    let points: Vec<Vec<[i32; 2]>> = (0..arcs.len())
        .map(|a| {
            arcs.points(a, false, 0)
                .map(|v| [v.lon(), v.lat()])
                .collect()
        })
        .collect();
    let scales: Vec<u32> = if arcs.has_scale() {
        (0..arcs.len()).map(|a| arcs.scale(a)).collect()
    } else {
        Vec::new()
    };
    let uses: Vec<Vec<u32>> = linked
        .iter()
        .map(|(_, _, p)| p.parts.iter().flatten().flatten().map(|r| r >> 1).collect())
        .collect();
    region::encode_on_arcs(&points, &scales, &uses, &Options::default()).map_err(e)
}

/// Reads ARCS, POLYGONS and `POLY_INDEX` back: their checks, every polygon on an admin unit of its level (the
/// country at level 0), and every current admin unit with a polygon (a former unit has none,
/// D-110). Returns a line for the summary.
pub(super) fn verify(p: &Pack<'_>, rows: &[&Place], g: &Geometry) -> Result<String, String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let places: Vec<Point<E7>> = rows
        .iter()
        .step_by(97)
        .filter_map(|r| Point::<E7>::from_units(r.point.1, r.point.0))
        .collect();
    let line = check(p, rows.len(), &places)?;
    let polys = Polygons::open(&p.section(Kind::POLYGONS).ok_or("no POLYGONS")?).map_err(e)?;
    // Group k holds the k-th admin level present, coarse to fine.
    let mut levels: Vec<u8> = g.polys.iter().map(|q| q.level).collect();
    levels.sort_unstable();
    levels.dedup();
    let mut with = vec![false; rows.len()];
    for i in 0..polys.len() {
        let row = polys.row(i).ok_or("a polygon without a row")? as usize;
        let place = rows.get(row).ok_or("a polygon past the rows")?;
        let group = (0..polys.groups())
            .find(|&k| polys.group(k).is_some_and(|r| r.contains(&i)))
            .ok_or("a polygon in no group")?;
        let level = levels
            .get(group)
            .copied()
            .ok_or("more groups than levels")?;
        let ok = match place.kind {
            PlaceKind::Country => level == 0,
            PlaceKind::Admin => place.admin_level == level,
            _ => false,
        };
        if !ok {
            return Err(format!(
                "{}: a polygon on a place that is not an admin unit of its level",
                place.key
            ));
        }
        with[row] = true;
    }
    let missing = rows
        .iter()
        .zip(&with)
        .filter(|(r, w)| r.kind == PlaceKind::Admin && r.flags & places::FLAG_FORMER == 0 && !**w)
        .count();
    if missing > 0 {
        return Err(format!("{missing} admin units without a polygon"));
    }
    let a = Arcs::open(&p.section(Kind::ARCS).ok_or("no ARCS")?).map_err(e)?;
    let admin = Region::open_on_arcs(&p.section(Kind::ADMIN_REGION).ok_or("no ADMIN_REGION")?, &a)
        .map_err(e)?;
    let pip = pip(&admin, &polys, &levels, rows)?;
    Ok(format!(
        "{line}; every current admin unit and the country has its polygon.\n\n{pip}"
    ))
}

/// The largest share of settlements and localities outside every admin unit (G-pip): the
/// source extracts reach past the border (Nepal 2026.11.0: 180 of 96,805, 0.19%, median 536 m
/// out, 170 within 5 km); more means a boundary or a point source has gone wrong.
const MAX_OUTSIDE: f64 = 0.005;

/// G-pip on the written pack: every settlement and locality inside the admin coverage lies
/// in exactly one unit of each level, and its parent is the deepest of them; places outside
/// every unit are counted, the first listed, and capped at [`MAX_OUTSIDE`]. Returns a line
/// for the summary.
fn pip(
    admin: &Region<'_>,
    polys: &Polygons<'_>,
    levels: &[u8],
    rows: &[&Place],
) -> Result<String, String> {
    let mut group_of = vec![0_usize; polys.len()];
    for g in 0..polys.groups() {
        for i in polys.group(g).unwrap_or_default() {
            group_of[i] = g;
        }
    }
    let key_of = |i: usize| {
        polys
            .row(i)
            .and_then(|r| rows.get(r as usize))
            .map(|p| p.key.as_str())
    };
    let mut got = [0_u32; region::MAX_SET];
    let (mut checked, mut outside, mut wrong) = (0_usize, Vec::new(), Vec::new());
    for r in rows
        .iter()
        .filter(|r| matches!(r.kind, PlaceKind::Settlement | PlaceKind::Locality))
    {
        let at = Point::<E7>::from_units(r.point.1, r.point.0).ok_or("a place off the grid")?;
        let k = admin.regions_at(at, &mut got);
        let held = got.get(..k).unwrap_or_default();
        if held.is_empty() {
            outside.push(r.key.as_str());
            continue;
        }
        checked += 1;
        let mut per = vec![0_u8; levels.len()];
        for &i in held {
            per[group_of[i as usize]] += 1;
        }
        if per.iter().any(|&n| n != 1) {
            wrong.push(format!("{}: units per level {per:?}", r.key));
            continue;
        }
        let deepest = held.iter().copied().max_by_key(|&i| group_of[i as usize]);
        let want = deepest.and_then(|i| key_of(i as usize));
        if r.parent.as_deref() != want {
            wrong.push(format!(
                "{}: parent {:?}, its point is in {want:?}",
                r.key, r.parent
            ));
        }
    }
    if !wrong.is_empty() {
        return Err(format!(
            "G-pip: {} of {checked} places inside the admin coverage are not in one unit of every level with the deepest as parent, first {:?}",
            wrong.len(),
            wrong.iter().take(8).collect::<Vec<_>>()
        ));
    }
    #[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
    let share = outside.len() as f64 / (checked + outside.len()).max(1) as f64;
    if share > MAX_OUTSIDE {
        return Err(format!(
            "G-pip: {} places ({:.2}%) lie outside every admin unit, over {:.1}%; first {:?}",
            outside.len(),
            share * 100.0,
            MAX_OUTSIDE * 100.0,
            outside.iter().take(8).collect::<Vec<_>>()
        ));
    }
    Ok(format!(
        "G-pip: {checked} settlements and localities each lie in one unit of every level, the deepest their parent; {} ({:.2}%) outside every unit, first {:?}.",
        outside.len(),
        share * 100.0,
        outside.iter().take(8).collect::<Vec<_>>()
    ))
}

/// The geometry sections' own checks (POLYGONS against `rows` place rows), the overview
/// against ARCS, and `ADMIN_REGION` against `POLY_INDEX` at every label point and at `probes`.
/// Returns a line for the summary.
fn check(p: &Pack<'_>, rows: usize, probes: &[Point<E7>]) -> Result<String, String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let a = Arcs::open(&p.section(Kind::ARCS).ok_or("no ARCS")?).map_err(e)?;
    a.check().map_err(e)?;
    let polys = Polygons::open(&p.section(Kind::POLYGONS).ok_or("no POLYGONS")?).map_err(e)?;
    polys.check(&a, rows).map_err(e)?;
    let index =
        PolyIndex::open(&p.section(Kind::POLY_INDEX).ok_or("no POLY_INDEX")?, &polys).map_err(e)?;
    index.check(&polys, &a).map_err(e)?;
    let admin = Region::open_on_arcs(&p.section(Kind::ADMIN_REGION).ok_or("no ADMIN_REGION")?, &a)
        .map_err(e)?;
    admin.check().map_err(e)?;
    let compared = cross_check(&admin, &index, &polys, &a, probes)?;
    let overview = match p.section(Kind::ARCS_OVERVIEW) {
        Some(section) => {
            let ov = Arcs::open(&section).map_err(e)?;
            ov.check().map_err(e)?;
            ov.check_overview(&a).map_err(e)?;
            format!(
                "; the overview holds the {} vertices of levels {} to 7",
                ov.vertices(),
                ov.overview_from().unwrap_or(0)
            )
        }
        None if a.has_levels() => return Err("no ARCS_OVERVIEW for arcs with levels".into()),
        None => String::new(),
    };
    Ok(format!(
        "Geometry: {} polygons on {} arcs of {} vertices, levels of detail {}{overview}; every ring joins, closes and is oriented, every label is inside; the polygon index ({} levels) matches every box; the admin region index ({} leaves, {} runs on the arcs) agrees with the polygons at {compared} points",
        polys.len(),
        a.len(),
        a.vertices(),
        if a.has_levels() { "0 to 7" } else { "0 only" },
        index.depth(),
        admin.leaves(),
        admin.runs(),
    ))
}

/// `ADMIN_REGION` against `POLY_INDEX` and `Polygons::contains`, two independent structures, at
/// every label point and at `probes`: the same polygons, in the same order (region IDs are
/// polygon numbers). Returns the number of points compared.
fn cross_check(
    admin: &Region<'_>,
    index: &PolyIndex<'_>,
    polys: &Polygons<'_>,
    arcs: &Arcs<'_>,
    probes: &[Point<E7>],
) -> Result<usize, String> {
    let labels = (0..polys.len()).filter_map(|i| polys.label(i));
    let mut n = 0;
    let mut got = [0_u32; region::MAX_SET];
    for at in labels.chain(probes.iter().copied()) {
        let k = admin.regions_at(at, &mut got);
        let mut want = Vec::new();
        let _ = index.containing(polys, arcs, at, 0..polys.len(), |i| {
            want.push(u32::try_from(i).unwrap_or(u32::MAX));
            std::ops::ControlFlow::Continue(())
        });
        want.sort_unstable();
        if got.get(..k) != Some(&want[..]) {
            return Err(format!(
                "ADMIN_REGION at {at} gives {:?}, the polygons {want:?}",
                got.get(..k.min(got.len()))
            ));
        }
        n += 1;
    }
    Ok(n)
}

/// `forge topology --geoboundaries`: a pack of the geometry sections alone, for measurement,
/// at `data/build/topology/<name>/geometry.tgd`. There are no places, so each polygon is its
/// own row, numbered as PLACES numbers rows: in Hilbert order of the point (here the label).
/// Every section is read back and checked. Returns a line for the summary.
pub(crate) fn measure_pack(root: &Path, name: &str) -> Result<String, String> {
    let g = load(root, name)?;
    let mut order: Vec<(u64, usize)> = g
        .polys
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let at = Point::<E7>::from_units(p.label.1, p.label.0).ok_or("a label off the grid")?;
            Ok((teigeodata_core::hilbert(at), i))
        })
        .collect::<Result<_, String>>()?;
    order.sort_unstable();
    let mut linked: Vec<(u8, u32, &Poly)> = order
        .iter()
        .enumerate()
        .map(|(row, &(_, i))| {
            let p = &g.polys[i];
            Ok((p.level, u32::try_from(row).map_err(|e| e.to_string())?, p))
        })
        .collect::<Result<_, String>>()?;
    linked.sort_by_key(|&(level, row, _)| (level, row));
    let bodies = encode(&g, &linked)?;
    let sections: Vec<(Kind, Encoding, u64, &[u8])> = bodies
        .iter()
        .map(|(kind, enc, n, b)| (*kind, *enc, *n, b.as_slice()))
        .collect();
    let mut w = Writer::new("zz", "geometry", "0.0.0").map_err(|e| e.to_string())?;
    w.build_id(0).created_unix(0);
    let bytes = w.pack(&sections, None).map_err(|e| e.to_string())?;
    let p = Pack::open(&bytes).map_err(|e| e.to_string())?;
    let line = check(&p, linked.len(), &[])?;
    let file = root
        .join("data/build/topology")
        .join(name)
        .join("geometry.tgd");
    fs::write(&file, &bytes).map_err(|e| format!("{}: {e}", file.display()))?;
    let sizes: Vec<String> = bodies
        .iter()
        .map(|(kind, _, _, b)| format!("{kind} {} B", b.len()))
        .collect();
    Ok(format!(
        "{line}.\n\n{} ({} B): {}.",
        file.display(),
        bytes.len(),
        sizes.join(", ")
    ))
}
