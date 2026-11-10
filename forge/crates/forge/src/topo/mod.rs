//! The `topology` stage: a boundary source's admin units as shared arcs with levels of detail,
//! and every unit as a polygon of arc references (B10, D-088, D-098).
//!
//! ```text
//! forge topology <boundary-source>
//! forge topology --geoboundaries <file> <name>   one geoBoundaries level, for measurement
//! ```
//!
//! 1. The finest level's rings are cleaned (outer rings counter-clockwise, holes clockwise) and
//!    cut into arcs at topology nodes ([`arcs`]). The source must be noded: any two edges that
//!    cross or touch away from a shared vertex stop the stage (snap rounding is not built).
//! 2. Every coarser level the source names (the country at least) is dissolved from the finest
//!    units by their stated ancestor codes, so all levels share one arc set, and each is compared with the
//!    source's own polygon of that unit both ways, by hole count and by area (G-topo). A unit
//!    off by more than [`MAX_PARENT_M`] either way, or with a hole the other lacks, stops the
//!    stage: a gap between children is a hole of the dissolved parent.
//! 3. Levels of detail and the guard ([`simplify`]); area and label point ([`measure`]).
//!
//! Output, in `data/build/topology/<source>/`: `arcs.bin` (an ARCS payload, arcs in Hilbert
//! order of their first vertex), `polygons.tsv` (level, P-code, label, hectares and parts as
//! arc references, read by `build` into POLYGONS), and `summary.md`.

#![allow(
    clippy::indexing_slicing,
    reason = "indices come from the same vectors (ring positions, arc vertices, segment ids): an out-of-range index is a forge bug and must stop the build, not be skipped"
)]

pub(crate) mod arcs;
pub(crate) mod measure;
pub(crate) mod pov;
pub(crate) mod simplify;
pub(crate) mod snap;

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::time::Instant;

use rayon::prelude::*;
use teigeodata_core::{E7, Point, hilbert};
use teigeodata_format::arcs::{self as arcs_section, ArcColumns};

use self::arcs::{Parts, Ref, V, ring_points};
use crate::lock::Lock;
use crate::read::{boundaries, natural_earth};

/// The largest distance from a source parent's vertex to the border dissolved from its
/// children, metres: COD-AB's levels differ by at most 1 cm (the geometry note §3.4), so 1 m is
/// a snapping tolerance, not a source property.
const MAX_PARENT_M: f64 = 1.0;
/// Metres per 1e-7° of latitude.
const M_PER_UNIT: f64 = 111_195.08e-7;
/// The header of `polygons.tsv`.
/// The box is every ring's full-resolution bounds: west, south, east, north on the 1e-7° grid.
pub(crate) const HEADER: &str = "level\tpcode\tlabel_lon_e7\tlabel_lat_e7\tarea_ha\twest_e7\tsouth_e7\teast_e7\tnorth_e7\tparts";

/// One output polygon.
struct Poly {
    level: u8,
    pcode: String,
    parts: Parts,
    label: V,
    area_ha: u32,
    bbox: [i32; 4],
}

/// A source unit.
struct Src {
    level: u8,
    pcode: String,
    parents: Vec<String>,
    rings: Vec<Vec<V>>,
    area_km2: Option<f64>,
}

fn src(level: u8, u: boundaries::Unit) -> Src {
    Src {
        level,
        pcode: u.pcode,
        parents: u.parents,
        rings: arcs::clean_rings(&u.polygon.rings),
        area_km2: u.area_km2,
    }
}

/// The units of a boundary source: a zipped COD-AB set (one file per level), or a plain
/// Natural Earth admin-0 file (one level, coded by `ADM0_A3`, D-147).
fn read_units(zip: &Path) -> Result<Vec<Src>, String> {
    if natural_earth::is_plain(zip)? {
        return Ok(natural_earth::read(zip)?
            .into_iter()
            .map(|c| Src {
                level: 0,
                pcode: c.code,
                parents: Vec::new(),
                rings: arcs::clean_rings(&c.polygon.rings),
                area_km2: None,
            })
            .collect());
    }
    let mut out = Vec::new();
    for level in boundaries::levels(zip)? {
        out.extend(
            boundaries::read(zip, level)?
                .into_iter()
                .map(|u| src(level, u)),
        );
    }
    Ok(out)
}

/// Renumbers arcs by the Hilbert key of their first vertex (then their vertices), so the
/// section's bases pack tight and the order is the same on every run.
fn sort_arcs(arcs: Vec<Vec<V>>, refs: &mut [Vec<Ref>]) -> Vec<Vec<V>> {
    let key = |a: &Vec<V>| Point::<E7>::from_units(a[0].1, a[0].0).map_or(0, hilbert);
    let mut order: Vec<usize> = (0..arcs.len()).collect();
    order.sort_by(|&x, &y| {
        key(&arcs[x])
            .cmp(&key(&arcs[y]))
            .then_with(|| arcs[x].cmp(&arcs[y]))
    });
    let mut new = vec![0_u32; arcs.len()];
    for (n, &old) in order.iter().enumerate() {
        new[old] = u32::try_from(n).unwrap_or(u32::MAX);
    }
    for r in refs.iter_mut().flatten() {
        *r = new[(*r >> 1) as usize] << 1 | (*r & 1);
    }
    let mut arcs: Vec<Option<Vec<V>>> = arcs.into_iter().map(Some).collect();
    order.iter().filter_map(|&o| arcs[o].take()).collect()
}

/// A dissolved parent against the source's own polygon of it (G-topo).
#[derive(Debug)]
struct Compared {
    /// The largest distance, metres, from a source vertex to the derived border.
    to_derived: f64,
    /// The largest distance, metres, from a derived vertex to the source's border: a gap
    /// between children leaves derived rings, and their vertices, away from it.
    to_source: f64,
    /// The share of source vertices that are derived vertices.
    same: f64,
    /// Holes in the derived polygon and in the source's.
    holes: (usize, usize),
    /// `|derived − source| / source` area.
    area_off: f64,
}

impl Compared {
    /// What makes the parent wrong: a border apart by more than `limit` metres either way, or
    /// a hole one side has and the other does not (a gap or an overlap between children).
    fn problem(&self, limit: f64) -> Option<String> {
        if self.to_derived > limit {
            return Some(format!(
                "a source vertex lies {:.2} m from the border dissolved from its children",
                self.to_derived
            ));
        }
        if self.to_source > limit {
            return Some(format!(
                "a vertex dissolved from its children lies {:.2} m from the source's border (a gap between them)",
                self.to_source
            ));
        }
        (self.holes.0 != self.holes.1).then(|| {
            format!(
                "{} holes dissolved from its children, {} in the source (a gap between them)",
                self.holes.0, self.holes.1
            )
        })
    }
}

fn compare(arcs: &[Vec<V>], parts: &Parts, src: &[Vec<V>]) -> Compared {
    let mine: Vec<Vec<V>> = parts
        .iter()
        .flatten()
        .map(|r| ring_points(arcs, r))
        .collect();
    let (to_derived, same) = distance(&mine, src);
    let (to_source, _) = distance(src, &mine);
    let area = |rings: &[Vec<V>]| {
        rings
            .iter()
            .map(|r| measure::ring_area_m2(r))
            .sum::<f64>()
            .abs()
    };
    let theirs = area(src);
    Compared {
        to_derived,
        to_source,
        same,
        holes: (
            parts.iter().map(|p| p.len().saturating_sub(1)).sum(),
            src.iter().filter(|r| arcs::area2(r) < 0).count(),
        ),
        area_off: (area(&mine) - theirs).abs() / theirs.max(1.0),
    }
}

/// The largest distance, metres, from a vertex of `src` to the border `rings`, and the share
/// of `src`'s vertices that are vertices of that border.
///
/// The border's segments are indexed by the 0.001° cells they pass through (not every cell of
/// their box: a Natural Earth border runs straight for thousands of kilometres), as a sorted
/// array, so memory is proportional to the border's length.
fn distance(rings: &[Vec<V>], src: &[Vec<V>]) -> (f64, f64) {
    const CELL: i64 = 10_000; // 0.001°: about 100 m
    let grid = snap::Grid {
        x0: 0,
        y0: 0,
        cell: CELL,
    };
    let mut segs: Vec<(V, V)> = Vec::new();
    let mut index: Vec<((i64, i64), u32)> = Vec::new();
    let mut verts: Vec<V> = Vec::new();
    let mut buf = Vec::new();
    for pts in rings {
        let n = pts.len();
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            verts.push(a);
            let k = u32::try_from(segs.len()).unwrap_or(u32::MAX);
            segs.push((a, b));
            grid.cells(a, b, &mut buf);
            index.extend(buf.iter().map(|&c| (c, k)));
        }
    }
    index.sort_unstable();
    index.dedup();
    verts.sort_unstable();
    let (mut worst, mut same, mut all) = (0.0_f64, 0_u32, 0_u32);
    for &p in src.iter().flatten() {
        all += 1;
        if verts.binary_search(&p).is_ok() {
            same += 1;
            continue;
        }
        let kx = M_PER_UNIT * crate::num::cos_deg(f64::from(p.1) * 1e-7);
        let (cx, cy) = grid.cell_of(p);
        // The 3 × 3 cells hold every segment within one cell (about 100 m); past that the
        // point is far from the border whatever the exact distance, so a miss counts as
        // infinitely far.
        let mut best = f64::INFINITY;
        for x in cx - 1..=cx + 1 {
            for y in cy - 1..=cy + 1 {
                let from = index.partition_point(|e| e.0 < (x, y));
                for &(c, k) in index.get(from..).into_iter().flatten() {
                    if c != (x, y) {
                        break;
                    }
                    if let Some(&(a, b)) = segs.get(k as usize) {
                        best = best.min(seg_m(p, a, b, kx));
                    }
                }
            }
        }
        worst = worst.max(best);
    }
    (worst, f64::from(same) / f64::from(all.max(1)))
}

fn seg_m(p: V, a: V, b: V, kx: f64) -> f64 {
    let (px, py) = (f64::from(p.0 - a.0) * kx, f64::from(p.1 - a.1) * M_PER_UNIT);
    let (bx, by) = (f64::from(b.0 - a.0) * kx, f64::from(b.1 - a.1) * M_PER_UNIT);
    let len2 = bx * bx + by * by;
    let t = if len2 > 0.0 {
        ((px * bx + py * by) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (px - t * bx).hypot(py - t * by)
}

/// Label, area (hectares) and box (west, south, east, north) of a polygon.
fn measure_poly(arcs: &[Vec<V>], parts: &Parts) -> Result<(V, u32, [i32; 4]), String> {
    let mut m2 = 0.0;
    let mut bbox = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for ring in parts.iter().flatten() {
        let pts = ring_points(arcs, ring);
        for &(x, y) in &pts {
            bbox = [
                bbox[0].min(x),
                bbox[1].min(y),
                bbox[2].max(x),
                bbox[3].max(y),
            ];
        }
        m2 += measure::ring_area_m2(&pts);
    }
    let first: Vec<Vec<V>> = parts
        .first()
        .ok_or("a polygon without parts")?
        .iter()
        .map(|r| ring_points(arcs, r))
        .collect();
    let label = match measure::label(&first) {
        Some(p) => p,
        None => crate::geom::Polygon { rings: first }
            .interior_point()
            .ok_or("no point inside the polygon")?,
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "hectares of one unit fit a u32"
    )]
    let ha = (m2.abs() / 10_000.0).round() as u32;
    Ok((label, ha, bbox))
}

/// `n` as an f64 for a share or a mean (counts far below 2^52).
#[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
const fn f(n: usize) -> f64 {
    n as f64
}

/// `forge topology <boundary-source>`.
pub(crate) fn run(root: &Path, source: &str) -> Result<String, String> {
    let lock = Lock::read(&root.join("data/sources.lock"))?;
    let units = read_units(&lock.path(root, source)?)?;
    stage(root, source, &units, MAX_PARENT_M)
}

/// The world's countries (level 0) and their first-level units cut to them (level 1, D-150)
/// as one source: the units and the remainders ([`pov::cut`]) are the finest level, so each
/// country is dissolved from them and checked against its own outline (G-topo), and countries
/// and units share one arc set. `name` names the output directory.
pub(crate) fn run_world(
    root: &Path,
    name: &str,
    countries: &[natural_earth::Country],
    units: &[natural_earth::Unit],
    cut: &pov::Cut,
) -> Result<String, String> {
    let mut src: Vec<Src> = countries
        .iter()
        .map(|c| Src {
            level: 0,
            pcode: c.code.clone(),
            parents: Vec::new(),
            rings: arcs::clean_rings(&c.polygon.rings),
            area_km2: None,
        })
        .collect();
    let code = |c: usize| {
        countries
            .get(c)
            .map(|c| c.code.clone())
            .ok_or("a country out of range")
    };
    for p in &cut.pieces {
        let u = units.get(p.unit).ok_or("a unit out of range")?;
        src.push(Src {
            level: 1,
            pcode: u.code.clone(),
            parents: vec![code(p.country)?],
            rings: arcs::clean_rings(&p.rings),
            area_km2: None,
        });
    }
    for (k, (c, rings)) in cut.fills.iter().enumerate() {
        let c = code(*c)?;
        src.push(Src {
            level: 1,
            pcode: format!("{}{c}.{k}", pov::FILLER),
            parents: vec![c],
            rings: arcs::clean_rings(rings),
            area_km2: None,
        });
    }
    // A sliver the cleaning emptied (no area on the grid) has nothing to give its country.
    src.retain(|u| !u.rings.is_empty());
    // The two sources draw one border a few metres apart, and a unit is cut only by the parts
    // of its overhang wider than noise, so the countries dissolve within that, not 1 m.
    stage(root, name, &src, pov::MAX_SHIFT_M)
}

/// `forge topology --geoboundaries <file> <name>`: one geoBoundaries level read from a local
/// file, for measurement (it is not a pinned source and feeds no build).
pub(crate) fn run_geoboundaries(root: &Path, file: &Path, name: &str) -> Result<String, String> {
    let (level, units) = boundaries::read_geoboundaries(file)?;
    let units: Vec<Src> = units.into_iter().map(|u| src(level, u)).collect();
    let s = stage(root, name, &units, MAX_PARENT_M)?;
    let pack = crate::build::geometry::measure_pack(root, name)?;
    Ok(format!("{s}\n{pack}\n"))
}

/// The stage on read units; `source` names the output directory. A dissolved parent may lie
/// at most `max_parent_m` from the source's own (G-topo).
fn stage(root: &Path, source: &str, units: &[Src], max_parent_m: f64) -> Result<String, String> {
    let t = Instant::now();
    let finest = units.iter().map(|u| u.level).max().ok_or("no units")?;
    let fine: Vec<&Src> = units.iter().filter(|u| u.level == finest).collect();
    let rings: Vec<(u32, Vec<V>)> = fine
        .iter()
        .enumerate()
        .flat_map(|(i, u)| {
            u.rings
                .iter()
                .map(move |r| (u32::try_from(i).unwrap_or(u32::MAX), r.clone()))
        })
        .collect();
    let (rings, snapped) = snap::snap_round(&rings);
    let (built, mut refs) = arcs::build(&rings)?;
    let arcs = sort_arcs(built, &mut refs);
    let mut s = format!("# topology: {source}\n\n");
    snap_line(&mut s, &snapped);
    let full: Vec<(V, V)> = arcs
        .iter()
        .flat_map(|a| a.windows(2).map(|w| (w[0], w[1])))
        .collect();
    let bad = simplify::crossings(&full);
    if !bad.is_empty() {
        return Err(format!(
            "{source}: snap rounding left {} edge pairs that cross or touch away from a vertex, first {:?}",
            bad.len(),
            bad.first().map(|&(i, j)| (full[i], full[j]))
        ));
    }
    // Each finest unit's references, then every coarser unit dissolved from them.
    let mut by_unit: Vec<Vec<Ref>> = vec![Vec::new(); fine.len()];
    for ((u, _), r) in rings.iter().zip(&refs) {
        by_unit[*u as usize].extend(r);
    }
    // A coarser level is dissolved when the source names it for every unit, and skipped when it
    // names it for none (a geoBoundaries file states only the country).
    let mut polys: Vec<(u8, String, Parts)> = Vec::new();
    for level in 0..=finest {
        let named = |u: &&Src| {
            !u.parents
                .get(usize::from(level))
                .is_none_or(String::is_empty)
        };
        if level < finest && !fine.iter().any(named) {
            continue;
        }
        let mut groups: BTreeMap<&str, Vec<Ref>> = BTreeMap::new();
        for (u, r) in fine.iter().zip(&by_unit) {
            let code = if level == finest {
                u.pcode.as_str()
            } else {
                u.parents.get(usize::from(level)).map_or("", String::as_str)
            };
            if code.is_empty() {
                return Err(format!("{}: no adm{level}_pcode", u.pcode));
            }
            groups.entry(code).or_default().extend(r);
        }
        for (code, r) in groups {
            let parts = arcs::polygon(&arcs, r).map_err(|e| format!("{code}: {e}"))?;
            polys.push((level, code.to_string(), parts));
        }
    }
    parents_report(&mut s, &arcs, units, &polys, (finest, max_parent_m))?;
    // Levels of detail, guarded on every ring of every polygon.
    let guard_rings: Vec<(Vec<Ref>, bool)> = polys
        .iter()
        .flat_map(|(_, _, parts)| {
            parts
                .iter()
                .flat_map(|p| p.iter().enumerate().map(|(k, r)| (r.clone(), k == 0)))
        })
        .collect();
    let (levels, report) = guarded_levels(source, &arcs, &guard_rings)?;
    let measured: Vec<(V, u32, [i32; 4])> = polys
        .par_iter()
        .map(|(_, _, p)| measure_poly(&arcs, p))
        .collect::<Result<_, _>>()?;
    let out: Vec<Poly> = polys
        .into_iter()
        .zip(measured)
        .map(|((level, pcode, parts), (label, area_ha, bbox))| Poly {
            level,
            pcode,
            parts,
            label,
            area_ha,
            bbox,
        })
        .collect();
    let dir = root.join("data/build/topology").join(source);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let written = write_arcs(&dir, &arcs, &levels)?;
    verify(&dir.join("arcs.bin"), &out)?;
    write_polygons(&dir, &out)?;
    summary(&mut s, &arcs, &levels, &report, &out, units, &written);
    let _ = writeln!(s, "\nIn {:.1?}.", t.elapsed());
    fs::write(dir.join("summary.md"), &s).map_err(|e| e.to_string())?;
    Ok(s)
}

/// The levels of detail; G-topo fails when the guard leaves a crossing or degenerate ring.
type Levels = (Vec<Vec<u8>>, Vec<(u8, simplify::Guarded)>);
fn guarded_levels(
    source: &str,
    arcs: &[Vec<V>],
    rings: &[(Vec<Ref>, bool)],
) -> Result<Levels, String> {
    let (levels, report) = simplify::levels(arcs, rings);
    if let Some((lod, g)) = report.iter().find(|(_, g)| g.left > 0) {
        return Err(format!(
            "G-topo: {source}: level of detail {lod} keeps {} crossings or degenerate rings no removed vertex fixes",
            g.left
        ));
    }
    Ok((levels, report))
}

/// What snap rounding did, when the source was not noded.
fn snap_line(s: &mut String, snapped: &snap::Snapped) {
    if snapped.bad_pairs > 0 {
        let _ = writeln!(
            s,
            "Snap rounding: {} edge pairs crossed or touched away from a vertex; {} crossing pixels, {} edges routed through hot pixels, {} vertices added, {} rings collapsed.\n",
            snapped.bad_pairs,
            snapped.new_pixels,
            snapped.edges_moved,
            snapped.vertices_added,
            snapped.rings_dropped
        );
    }
}

/// Compares each dissolved parent with the source's own polygon; fails past `limit` metres.
fn parents_report(
    s: &mut String,
    arcs: &[Vec<V>],
    units: &[Src],
    polys: &[(u8, String, Parts)],
    (finest, limit): (u8, f64),
) -> Result<(), String> {
    let derived: BTreeMap<(u8, &str), &Parts> = polys
        .iter()
        .map(|(l, c, p)| ((*l, c.as_str()), p))
        .collect();
    let _ = writeln!(
        s,
        "| Level | Units | Source vertices on the derived border | Largest distance | Back (derived to source) | Holes | Area off (P50 / max) |\n|---:|---:|---:|---:|---:|---:|---:|"
    );
    for level in 0..finest {
        let src: Vec<&Src> = units.iter().filter(|u| u.level == level).collect();
        if src.is_empty() {
            continue;
        }
        let rows: Vec<Compared> = src
            .par_iter()
            .map(|u| {
                let parts = derived
                    .get(&(level, u.pcode.as_str()))
                    .ok_or_else(|| format!("adm{level} {}: no finest unit names it", u.pcode))?;
                Ok(compare(arcs, parts, &u.rings))
            })
            .collect::<Result<_, String>>()?;
        let worst = rows.iter().map(|r| r.to_derived).fold(0.0, f64::max);
        let back = rows.iter().map(|r| r.to_source).fold(0.0, f64::max);
        let same = rows.iter().map(|r| r.same).sum::<f64>() / f(rows.len());
        let holes = rows.iter().map(|r| r.holes.0).sum::<usize>();
        let mut off: Vec<f64> = rows.iter().map(|r| r.area_off).collect();
        off.sort_by(f64::total_cmp);
        let _ = writeln!(
            s,
            "| {level} | {} | {:.2}% | {worst:.3} m | {back:.3} m | {holes} | {:.4}% / {:.4}% |",
            rows.len(),
            same * 100.0,
            off[off.len() / 2] * 100.0,
            off[off.len() - 1] * 100.0
        );
        if let Some((u, why)) = src
            .iter()
            .zip(&rows)
            .find_map(|(u, r)| r.problem(limit).map(|w| (u, w)))
        {
            return Err(format!(
                "{s}\nG-topo: adm{level} {}: {why} (limit {limit} m)",
                u.pcode
            ));
        }
    }
    if units.iter().filter(|u| u.level < finest).count()
        + units.iter().filter(|u| u.level == finest).count()
        < polys.len()
    {
        let _ = writeln!(
            s,
            "\nSome derived units have no polygon of their own in the source."
        );
    }
    Ok(())
}

/// Reads the written ARCS payload back through the format's reader and checks it: the section's
/// own check, then at every level of detail no two kept segments meet away from a shared end
/// and every ring keeps at least 3 vertices and its orientation. Independent of the levels the
/// guard computed: a guard that misses a crossing fails here.
fn verify(file: &Path, polys: &[Poly]) -> Result<(), String> {
    use teigeodata_format::{Encoding, Kind, Pack, Writer};
    let body = fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let n = teigeodata_core::table::Table::parse(&body)
        .map_err(|e| format!("{e:?}"))?
        .column(arcs_section::BASE_X)
        .map_err(|e| format!("{e:?}"))?
        .len();
    let w = Writer::new("xx", "topology", "0.0.0").map_err(|e| e.to_string())?;
    let bytes = w
        .pack(&[(Kind::ARCS, Encoding::TABLE, n as u64, &body[..])], None)
        .map_err(|e| e.to_string())?;
    let pack = Pack::open(&bytes).map_err(|e| e.to_string())?;
    let sec = pack.section(Kind::ARCS).ok_or("no ARCS")?;
    let a = arcs_section::Arcs::open(&sec).map_err(|e| e.to_string())?;
    a.check().map_err(|e| e.to_string())?;
    let read = |lod: u8| -> Vec<Vec<V>> {
        (0..a.len())
            .map(|i| {
                a.points(i, false, lod)
                    .map(|p| (p.lon(), p.lat()))
                    .collect()
            })
            .collect()
    };
    for lod in 0..=simplify::PINNED {
        let kept = read(lod);
        let segs: Vec<(V, V)> = kept
            .iter()
            .flat_map(|k| k.windows(2).map(|w| (w[0], w[1])))
            .collect();
        let bad = simplify::crossings(&segs);
        if let Some(&(i, j)) = bad.first() {
            return Err(format!(
                "level {lod}: {} crossings in the written arcs, first {:?} × {:?}",
                bad.len(),
                segs[i],
                segs[j]
            ));
        }
        for p in polys {
            for (k, ring) in p.parts.iter().flat_map(|part| part.iter().enumerate()) {
                let pts = ring_points(&kept, ring);
                let a2 = arcs::area2(&pts);
                if pts.len() < 3 || a2 == 0 || (a2 > 0) != (k == 0) {
                    return Err(format!(
                        "level {lod}: a ring of {} {} has {} vertices or the wrong orientation",
                        p.level,
                        p.pcode,
                        pts.len()
                    ));
                }
            }
        }
    }
    Ok(())
}

/// What was written: ARCS's bytes, and the arcs and vertices stored on a coarser decimal grid
/// (scale above 0), counted by the format's reader.
struct Written {
    bytes: usize,
    scaled_arcs: usize,
    scaled_vertices: usize,
}

fn write_arcs(dir: &Path, arcs: &[Vec<V>], levels: &[Vec<u8>]) -> Result<Written, String> {
    let pts: Vec<Vec<Point<E7>>> = arcs
        .iter()
        .map(|a| {
            a.iter()
                .map(|&(x, y)| {
                    Point::from_units(y, x).ok_or_else(|| format!("{x},{y} is off the grid"))
                })
                .collect()
        })
        .collect::<Result<_, _>>()?;
    let refs: Vec<&[Point<E7>]> = pts.iter().map(Vec::as_slice).collect();
    let lv: Vec<&[u8]> = levels.iter().map(Vec::as_slice).collect();
    let body = arcs_section::encode(&ArcColumns {
        arcs: &refs,
        levels: &lv,
        tolerances_cm: &simplify::TOLERANCE_CM,
    })
    .map_err(|e| e.to_string())?;
    fs::write(dir.join("arcs.bin"), &body).map_err(|e| e.to_string())?;
    let read = teigeodata_format::arcs::Arcs::open(&teigeodata_format::Section::unpacked(
        teigeodata_format::Kind::ARCS,
        arcs.len() as u64,
        &body,
    ))
    .map_err(|e| e.to_string())?;
    let scaled: Vec<usize> = (0..read.len()).filter(|&a| read.scale(a) > 0).collect();
    Ok(Written {
        bytes: body.len(),
        scaled_arcs: scaled.len(),
        scaled_vertices: scaled.iter().map(|&a| arcs[a].len()).sum(),
    })
}

/// `parts` as text: parts split by `|`, rings by `;`, references by `,`.
pub(crate) fn parts_text(parts: &Parts) -> String {
    parts
        .iter()
        .map(|p| {
            p.iter()
                .map(|r| r.iter().map(u32::to_string).collect::<Vec<_>>().join(","))
                .collect::<Vec<_>>()
                .join(";")
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// The inverse of [`parts_text`].
pub(crate) fn parse_parts(text: &str) -> Result<Parts, String> {
    text.split('|')
        .map(|p| {
            p.split(';')
                .map(|r| {
                    r.split(',')
                        .map(|v| v.parse::<u32>().map_err(|e| format!("{v}: {e}")))
                        .collect()
                })
                .collect()
        })
        .collect()
}

fn write_polygons(dir: &Path, polys: &[Poly]) -> Result<(), String> {
    let mut t = format!("{HEADER}\n");
    for p in polys {
        let _ = writeln!(
            t,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            p.level,
            p.pcode,
            p.label.0,
            p.label.1,
            p.area_ha,
            p.bbox[0],
            p.bbox[1],
            p.bbox[2],
            p.bbox[3],
            parts_text(&p.parts)
        );
    }
    fs::write(dir.join("polygons.tsv"), t).map_err(|e| e.to_string())
}

fn summary(
    s: &mut String,
    arcs: &[Vec<V>],
    levels: &[Vec<u8>],
    report: &[(u8, simplify::Guarded)],
    polys: &[Poly],
    units: &[Src],
    written: &Written,
) {
    let bytes = written.bytes;
    let v: usize = arcs.iter().map(Vec::len).sum();
    let ring_v: usize = units.iter().flat_map(|u| &u.rings).map(Vec::len).sum();
    let _ = writeln!(
        s,
        "\n{} polygons on {} arcs of {v} vertices ({:.1}% of the source's {ring_v} ring vertices); ARCS {bytes} B, {:.2} B per vertex.\n",
        polys.len(),
        arcs.len(),
        100.0 * f(v) / f(ring_v.max(1)),
        f(bytes) / f(v.max(1))
    );
    if written.scaled_arcs > 0 {
        let _ = writeln!(
            s,
            "On a coarser decimal grid (scale above 0): {} of {} arcs, {} of {v} vertices.\n",
            written.scaled_arcs,
            arcs.len(),
            written.scaled_vertices
        );
    }
    let _ = writeln!(
        s,
        "| Level of detail | Tolerance | Vertices kept | Crossings found | Degenerate rings | Added back | Left |\n|---:|---:|---:|---:|---:|---:|---:|"
    );
    for (lod, g) in report.iter().rev() {
        let tol = simplify::TOLERANCE_CM
            .get(usize::from(*lod) - 1)
            .map_or("pinned".to_string(), |c| format!("{} m", c / 100));
        let _ = writeln!(
            s,
            "| {lod} | {tol} | {} | {} | {} | {} | {} |",
            g.kept, g.crossings, g.rings, g.added, g.left
        );
    }
    let _ = levels;
    // Areas against the source's stated `area_sqkm`.
    let stated: HashMap<(u8, &str), f64> = units
        .iter()
        .filter_map(|u| Some(((u.level, u.pcode.as_str()), u.area_km2?)))
        .collect();
    let mut off: Vec<f64> = polys
        .iter()
        .filter_map(|p| {
            let km2 = *stated.get(&(p.level, p.pcode.as_str()))?;
            (km2 > 0.0).then(|| (f64::from(p.area_ha) / 100.0 - km2).abs() / km2)
        })
        .collect();
    off.sort_by(f64::total_cmp);
    if let (Some(m), Some(x)) = (off.get(off.len() / 2), off.last()) {
        let _ = writeln!(
            s,
            "\nArea against the source's `area_sqkm` ({} units): P50 {:.3}%, max {:.3}%.",
            off.len(),
            m * 100.0,
            x * 100.0
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_PARENT_M, V, arcs, compare, distance};

    const S: i32 = 100_000; // about 1.1 km on the 1e-7° grid

    /// Children around the middle square of a 3 × 3 parent, with or without the middle one.
    fn dissolved(with_middle: bool) -> super::Compared {
        let sq = |x: i32, y: i32| vec![(x, y), (x + S, y), (x + S, y + S), (x, y + S)];
        let mut rings: Vec<(u32, Vec<V>)> = vec![
            (
                0,
                vec![(0, 0), (3 * S, 0), (3 * S, S), (2 * S, S), (S, S), (0, S)],
            ),
            (
                1,
                vec![
                    (0, 2 * S),
                    (S, 2 * S),
                    (2 * S, 2 * S),
                    (3 * S, 2 * S),
                    (3 * S, 3 * S),
                    (0, 3 * S),
                ],
            ),
            (2, sq(0, S)),
            (3, sq(2 * S, S)),
        ];
        if with_middle {
            rings.push((4, sq(S, S)));
        }
        let (arcs, refs) = arcs::build(&rings).unwrap();
        let parts = arcs::polygon(&arcs, refs.into_iter().flatten()).unwrap();
        compare(
            &arcs,
            &parts,
            &[vec![(0, 0), (3 * S, 0), (3 * S, 3 * S), (0, 3 * S)]],
        )
    }

    #[test]
    fn a_gap_between_children_is_refused() {
        let whole = dissolved(true);
        assert_eq!(whole.problem(MAX_PARENT_M), None, "{whole:?}");
        assert_eq!(whole.holes, (0, 0));
        assert!(
            whole.to_derived < 0.01 && whole.to_source < 0.01,
            "{whole:?}"
        );
        let gap = dissolved(false);
        assert_eq!(gap.holes, (1, 0), "{gap:?}");
        assert!(gap.to_source > 1.0, "{gap:?}");
        assert!(
            gap.problem(MAX_PARENT_M).is_some_and(|p| p.contains("gap")),
            "{gap:?}"
        );
        // The area alone would have said 11%, with no limit on it.
        assert!((gap.area_off - 1.0 / 9.0).abs() < 1e-3, "{gap:?}");
    }

    /// A Natural Earth border runs straight for thousands of kilometres: its segments are
    /// indexed by the cells they pass through, not every cell of their box (a 20° diagonal's
    /// box is 4 × 10^8 cells, over 12 GB on the world build). A point 50 m beside the middle of
    /// the long edge is still found there.
    #[test]
    fn a_long_diagonal_border_is_indexed_along_itself() {
        const D: i32 = 200_000_000; // 20°
        let ring = vec![(0, 0), (D, D), (0, D)];
        let mid = D / 2;
        // About 50 m off the diagonal, perpendicular: 3,180 units (1.1 cm each) along each axis.
        let off = 3_180;
        let border = std::slice::from_ref(&ring);
        let (far, same) = distance(border, &[vec![(mid + off, mid - off)]]);
        assert!((far - 50.0).abs() < 2.0, "{far}");
        assert!(same.abs() < f64::EPSILON);
        let (near, same) = distance(border, border);
        assert!(
            near < 1e-9 && (same - 1.0).abs() < f64::EPSILON,
            "{near} {same}"
        );
    }
}
