//! The `deepest` stage: a country's deepest admin level from OpenStreetMap, joined to its
//! boundary source as one more level (D-363, [the note](../../../../docs/02-research/notes/ward-boundaries.md)).
//!
//! ```text
//! forge deepest <cc>
//! ```
//!
//! 1. The OSM areas of the recipe's `admin_level` are read with their rings
//!    ([`crate::read::osm_areas`]); each takes its number from the recipe's tag, or the
//!    trailing digits of its name.
//! 2. The parents' rings and the areas' rings are snap-rounded together into one planar
//!    arrangement ([`super::faces`]): OSM and the boundary source draw a shared line tens of
//!    metres apart, and the levels must nest exactly. Each face is labelled by the parent and
//!    the area holding a point inside it; an area belongs to the parent holding most of it,
//!    and two areas with one number in one parent are one unit.
//! 3. A face of a parent that no area of that parent holds is a gap: narrower than
//!    [`SLIVER_M`] on average, it is the two sources' disagreement and joins the neighbouring
//!    unit sharing the most border; wider, it is land OSM lacks and stays a filler (code
//!    [`pov::FILLER`]…), with no place, so the parent stays the union of its children. Every
//!    unit is a union of faces, so neighbours share each edge exactly.
//! 4. The set is written to `data/build/deepest/<admin>+<source>/`: `base.zip` (the boundary
//!    source as pinned) and `admin<level>.geojson`, the new level in the source's own format,
//!    coded by the unit's P-code and the 2-digit number, named by the recipe's pattern.
//!    `summary.md` counts what each step did.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::io::{BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Deserialize;

use super::arcs::{V, clean_rings};
use super::faces::Arrangement;
use super::pov;
use crate::geom::{GridIndex, Polygon};
use crate::lock::Lock;
use crate::read::{boundaries, osm_areas};

/// A remainder part narrower than this on average, metres, is a disagreement between the two
/// sources' lines, not a missing area: Kirtipur's OSM outline lies up to 89 m from COD-AB's
/// (2026-11-17). A part this narrow is at most a strip along a line.
pub(crate) const SLIVER_M: f64 = 60.0;

/// Metres per 1e-7° of latitude.
const M_PER_UNIT: f64 = 111_195.08e-7;

/// How far, metres, a parent dissolved from its derived children may lie from the source's
/// own line (G-topo): the children come from one arrangement of the parents' snap-rounded
/// rings, so a parent moves by at most a pixel's half diagonal, about 1 cm; the source's own
/// 1 m holds.
pub(crate) const MAX_PARENT_M: f64 = 1.0;

/// 0.1° cells for labelling faces.
const CELL_E7: i32 = 1_000_000;

/// `deepest:` in a country's `build.yaml`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Spec {
    /// The pinned OSM extract.
    pub(crate) source: String,
    /// OSM's `admin_level` of the areas.
    pub(crate) osm_admin_level: u8,
    /// The tag holding an area's number within its parent (`ward`).
    pub(crate) number: String,
    /// The pattern of an area's name: `{parent}` and `{number}`.
    pub(crate) name: String,
}

/// The derived set's name: the boundary source and the OSM extract.
pub(crate) fn set_name(admin: &str, spec: &Spec) -> String {
    format!("{admin}+{}", spec.source)
}

/// Where a boundary set is: a pinned source's file, or a derived set's directory.
pub(crate) fn set_path(root: &Path, lock: &Lock, set: &str) -> Result<PathBuf, String> {
    if set.contains('+') {
        let dir = root.join("data/build/deepest").join(set);
        if dir.is_dir() {
            Ok(dir)
        } else {
            Err(format!("{}: missing (run `forge deepest`)", dir.display()))
        }
    } else {
        lock.path(root, set)
    }
}

/// The numbers a code can hold: a unit's code is its parent's and 2 digits.
const NUMBERS: std::ops::RangeInclusive<u32> = 1..=99;

/// An area's number: its tag, else the trailing digits of its name, when it fits the code
/// ([`NUMBERS`]): OSM has `ward=1433` on one of Nepal's wards (2026-11-05 extract).
fn number(a: &osm_areas::Area, tag: &str) -> Option<u32> {
    let tagged = a.tag(tag).and_then(|v| v.trim().parse().ok());
    let named = a.tag("name").and_then(|name| {
        let digits: String = name
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        digits.parse().ok()
    });
    tagged
        .filter(|n| NUMBERS.contains(n))
        .or(named.filter(|n| NUMBERS.contains(n)))
}

/// One unit of the new level: a numbered area, or a filler.
struct Out {
    parent: usize,
    number: Option<u32>,
    rings: Vec<Vec<V>>,
}

/// `forge deepest <cc>`.
#[allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "the stage's steps, in order; face f, area a, parent p, number n, point x y"
)]
pub(crate) fn run(root: &Path, admin: &str, spec: &Spec) -> Result<String, String> {
    let t = Instant::now();
    let lock = Lock::read(&root.join("data/sources.lock"))?;
    let zip = lock.path(root, admin)?;
    let levels = boundaries::levels(&zip)?;
    let finest = *levels.last().ok_or("the boundary source has no levels")?;
    let level = finest + 1;
    let parents = boundaries::read(&zip, finest)?;
    let osm = osm_areas::admin_areas(
        &lock.path(root, &spec.source)?,
        &spec.osm_admin_level.to_string(),
    )?;

    // One arrangement of every parent's rings and every area's rings (tags: parents first).
    let p_count = u32::try_from(parents.len()).map_err(|_| "too many parents")?;
    let mut tagged: Vec<(u32, Vec<V>)> = Vec::new();
    for (i, u) in parents.iter().enumerate() {
        let tag = u32::try_from(i).unwrap_or(u32::MAX);
        tagged.extend(clean_rings(&u.polygon.rings).into_iter().map(|r| (tag, r)));
    }
    for (i, a) in osm.areas.iter().enumerate() {
        let tag = p_count + u32::try_from(i).unwrap_or(u32::MAX);
        tagged.extend(clean_rings(&a.rings).into_iter().map(|r| (tag, r)));
    }
    let (arr, snapped, report) = Arrangement::build(&tagged);
    // The rounded parents and areas, for labelling faces.
    let mut polys: Vec<Polygon> = vec![Polygon::default(); parents.len() + osm.areas.len()];
    for (tag, r) in snapped {
        if let Some(p) = polys.get_mut(tag as usize) {
            p.rings.push(r);
        }
    }
    let (parent_polys, area_polys) = polys.split_at(parents.len());
    let parent_index = GridIndex::new(parent_polys, CELL_E7);
    let area_index = GridIndex::new(area_polys, CELL_E7);

    // Label each bounded face by the parent and the area holding a point inside it.
    let mut label: Vec<Label> = vec![Label::Outside; arr.faces.len()];
    let mut thin = 0usize;
    let mut share: BTreeMap<(usize, usize), i128> = BTreeMap::new();
    let mut area_of_face: Vec<Option<usize>> = vec![None; arr.faces.len()];
    for (f, face) in arr.faces.iter().enumerate() {
        if face.area2 <= 0 {
            continue;
        }
        let poly = Polygon {
            rings: vec![arr.ring(face)],
        };
        let Some((x, y)) = poly.interior_point() else {
            thin += 1;
            if let Some(l) = label.get_mut(f) {
                *l = Label::Thin;
            }
            continue;
        };
        let holder = |index: &GridIndex, set: &[Polygon]| {
            index
                .candidates(x, y)
                .filter(|&i| set.get(i).is_some_and(|p| p.contains(x, y)))
                .min()
        };
        let Some(p) = holder(&parent_index, parent_polys) else {
            continue;
        };
        let a = holder(&area_index, area_polys);
        if let Some(a) = a {
            *share.entry((a, p)).or_default() += face.area2;
        }
        area_of_face[f] = a;
        if let Some(l) = label.get_mut(f) {
            *l = Label::Gap(p);
        }
    }
    // Each area belongs to the parent holding most of it; its faces in other parents are gaps.
    let mut home: BTreeMap<usize, (usize, i128)> = BTreeMap::new();
    for (&(a, p), &s2) in &share {
        let e = home.entry(a).or_insert((p, s2));
        if s2 > e.1 {
            *e = (p, s2);
        }
    }
    let mut unnumbered = BTreeSet::new();
    for (f, l) in label.iter_mut().enumerate() {
        let (Label::Gap(p), Some(a)) = (*l, area_of_face.get(f).copied().flatten()) else {
            continue;
        };
        if home.get(&a).map(|h| h.0) != Some(p) {
            continue;
        }
        match osm.areas.get(a).and_then(|ar| number(ar, &spec.number)) {
            Some(n) => *l = Label::Unit(p, n),
            None => {
                unnumbered.insert(a);
            }
        }
    }

    // Gaps and thin faces: a sliver (or a face too thin to label) joins the neighbouring unit
    // of its parent sharing the most border; repeated, so a strip of slivers is absorbed from
    // its ends inward. What remains of a gap is a filler.
    let mut slivers = 0usize;
    for _ in 0..8 {
        let mut changed = false;
        for f in 0..arr.faces.len() {
            let (Some(face), Some(&l)) = (arr.faces.get(f), label.get(f)) else {
                continue;
            };
            let parent = match l {
                Label::Gap(p) => Some(p),
                Label::Thin => None,
                _ => continue,
            };
            #[allow(clippy::cast_precision_loss, reason = "areas far below 2^100")]
            let width = face.area2 as f64 / face.perimeter.max(1.0);
            if l != Label::Thin && width * M_PER_UNIT >= SLIVER_M {
                continue;
            }
            let mut border: BTreeMap<Label, f64> = BTreeMap::new();
            for &h in &face.edges {
                let Some(&o) = label.get(arr.across(h) as usize) else {
                    continue;
                };
                let ok = match (o, parent) {
                    (Label::Unit(q, _), Some(p)) => q == p,
                    (Label::Unit(..) | Label::Gap(_), None) => true,
                    _ => false,
                };
                if ok {
                    let (a, b) = (arr.origin(h), arr.origin(h ^ 1));
                    *border.entry(o).or_default() +=
                        (f64::from(b.0) - f64::from(a.0)).hypot(f64::from(b.1) - f64::from(a.1));
                }
            }
            if let Some((&to, _)) = border.iter().max_by(|a, b| a.1.total_cmp(b.1)) {
                if let Some(slot) = label.get_mut(f) {
                    *slot = to;
                }
                slivers += usize::from(matches!(to, Label::Unit(..)));
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Each unit's half-edges: its faces' cycles and the outlines of the parts it surrounds.
    let mut groups: BTreeMap<Label, Vec<u32>> = BTreeMap::new();
    for (f, face) in arr.faces.iter().enumerate() {
        if let Some(&l @ (Label::Unit(..) | Label::Gap(_))) = label.get(f) {
            groups.entry(l).or_default().extend(&face.edges);
        }
    }
    for (hole, around) in arr.holes() {
        let Some(around) = around else { continue };
        let (Some(&l @ (Label::Unit(..) | Label::Gap(_))), Some(face)) =
            (label.get(around as usize), arr.faces.get(hole as usize))
        else {
            continue;
        };
        groups.entry(l).or_default().extend(&face.edges);
    }
    let (mut fillers, mut filler_ha) = (0usize, 0.0f64);
    let mut units: Vec<Out> = Vec::with_capacity(groups.len());
    for (l, edges) in groups {
        let rings = arr.rings_of(&edges);
        if rings.is_empty() {
            continue;
        }
        let (parent, number) = match l {
            Label::Unit(p, n) => (p, Some(n)),
            Label::Gap(p) => {
                fillers += 1;
                let (a2, _) = area2_perimeter(&rings);
                let lat = rings
                    .first()
                    .and_then(|r| r.first())
                    .map_or(0.0, |&(_, y)| f64::from(y) * 1e-7);
                #[allow(clippy::cast_precision_loss, reason = "areas far below 2^100")]
                let ha =
                    a2 as f64 / 2.0 * M_PER_UNIT * M_PER_UNIT * lat.to_radians().cos() / 10_000.0;
                filler_ha += ha;
                (p, None)
            }
            _ => continue,
        };
        units.push(Out {
            parent,
            number,
            rings,
        });
    }
    let numbered_count = units.iter().filter(|u| u.number.is_some()).count();
    let duplicates = {
        let mut seen: BTreeMap<(usize, u32), BTreeSet<usize>> = BTreeMap::new();
        for (a, ar) in osm.areas.iter().enumerate() {
            if let (Some(n), Some(&(p, _))) = (number(ar, &spec.number), home.get(&a)) {
                seen.entry((p, n)).or_default().insert(a);
            }
        }
        seen.values().filter(|v| v.len() > 1).count()
    };
    let outside = osm.areas.len() - home.len();
    // Each parent's numbers, for the report: gaps in 1..=max.
    let mut by_parent: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
    for u in &units {
        if let Some(n) = u.number {
            by_parent.entry(u.parent).or_default().push(n);
        }
    }
    let gaps: usize = by_parent
        .values()
        .map(|ns| {
            let max = ns.iter().copied().max().unwrap_or(0);
            (1..=max).filter(|n| !ns.contains(n)).count()
        })
        .sum();
    let without = parents.len() - by_parent.len();

    let set = set_name(admin, spec);
    let dir = root.join("data/build/deepest").join(&set);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let base = dir.join(boundaries::BASE);
    let _ = fs::remove_file(&base);
    if fs::hard_link(&zip, &base).is_err() {
        fs::copy(&zip, &base).map_err(|e| format!("{}: {e}", base.display()))?;
    }
    write_level(&dir, level, &parents, &units, &spec.name)?;

    let mut s = String::new();
    let _ = writeln!(s, "# Derived boundary set `{set}`\n");
    let _ = writeln!(
        s,
        "Level {level} from OSM `admin_level={}` cut to level {finest} of `{admin}` (D-363), in {:.1?}.\n",
        spec.osm_admin_level,
        t.elapsed()
    );
    let _ = writeln!(s, "| What | Count |\n|---|--:|");
    for (what, n) in [
        ("OSM areas read", osm.areas.len()),
        ("OSM relations left out (not closed)", osm.broken.len()),
        ("areas in no unit", outside),
        ("areas without a number", unnumbered.len()),
        ("numbers held by two or more areas, joined", duplicates),
        ("numbered units", numbered_count),
        ("faces of the arrangement", arr.faces.len()),
        ("faces too thin to label, given to a neighbour", thin),
        ("slivers given to a neighbouring unit", slivers),
        ("fillers (no area there)", fillers),
        ("parent units without any numbered unit", without),
        ("numbers missing below a parent's highest", gaps),
    ] {
        let _ = writeln!(s, "| {what} | {n} |");
    }
    let _ = writeln!(s, "\nFillers cover {filler_ha:.0} ha.");
    snap_line(&mut s, &report);
    for (id, why) in osm.broken.iter().take(20) {
        let _ = writeln!(s, "- relation {id}: {why}");
    }
    fs::write(dir.join("summary.md"), &s).map_err(|e| e.to_string())?;
    Ok(s)
}

/// What a face of the arrangement is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Label {
    /// In no parent.
    Outside,
    /// Too thin for a point inside it: it takes a neighbour's label.
    Thin,
    /// A parent's land no numbered area holds.
    Gap(usize),
    /// A parent's numbered unit.
    Unit(usize, u32),
}

/// Twice the area (even–odd, holes subtracted by orientation) and the length of `rings`.
fn area2_perimeter(rings: &[Vec<V>]) -> (i128, f64) {
    let (mut a, mut p) = (0i128, 0.0f64);
    for r in rings {
        for (x, y) in r.iter().zip(r.iter().cycle().skip(1)) {
            a += i128::from(x.0) * i128::from(y.1) - i128::from(y.0) * i128::from(x.1);
            p += (f64::from(y.0) - f64::from(x.0)).hypot(f64::from(y.1) - f64::from(x.1));
        }
    }
    (a.abs(), p)
}

/// The snap rounding line of the report.
fn snap_line(s: &mut String, r: &super::snap::Snapped) {
    let _ = writeln!(
        s,
        "\nSnap rounding: {} edge pairs crossed or touched away from a vertex, {} new hot pixels, {} edges moved, {} vertices added, {} rings dropped.",
        r.bad_pairs, r.new_pixels, r.edges_moved, r.vertices_added, r.rings_dropped
    );
}

/// A grid value as decimal degrees that parse back to it exactly.
fn deg(v: i32) -> String {
    let sign = if v < 0 { "-" } else { "" };
    let a = v.unsigned_abs();
    format!("{sign}{}.{:07}", a / 10_000_000, a % 10_000_000)
}

/// `admin<level>.geojson`: the new level in the boundary source's format (`adm<l>_pcode` for
/// every ancestor, `adm<level>_pcode`, `_name` and `_number`).
fn write_level(
    dir: &Path,
    level: u8,
    parents: &[boundaries::Unit],
    units: &[Out],
    pattern: &str,
) -> Result<(), String> {
    let path = dir.join(format!("admin{level}.geojson"));
    let file = fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut w = BufWriter::with_capacity(1 << 20, file);
    let io = |e: std::io::Error| format!("{}: {e}", path.display());
    w.write_all(b"{\"type\":\"FeatureCollection\",\"features\":[")
        .map_err(io)?;
    let mut fill_no: BTreeMap<usize, usize> = BTreeMap::new();
    for (i, u) in units.iter().enumerate() {
        let p = parents
            .get(u.parent)
            .ok_or("a unit's parent out of range")?;
        let (code, name) = if let Some(n) = u.number {
            (
                format!("{}{n:02}", p.pcode),
                pattern
                    .replace("{parent}", &p.name)
                    .replace("{number}", &n.to_string()),
            )
        } else {
            let k = fill_no.entry(u.parent).or_default();
            *k += 1;
            (format!("{}{}.{k}", pov::FILLER, p.pcode), String::new())
        };
        let mut props = serde_json::Map::new();
        for (l, a) in p.parents.iter().enumerate() {
            props.insert(format!("adm{l}_pcode"), a.clone().into());
        }
        props.insert(format!("adm{}_pcode", level - 1), p.pcode.clone().into());
        props.insert(format!("adm{level}_pcode"), code.into());
        props.insert(format!("adm{level}_name"), name.into());
        if let Some(n) = u.number {
            props.insert(format!("adm{level}_number"), n.into());
        }
        if i > 0 {
            w.write_all(b",").map_err(io)?;
        }
        write!(
            w,
            "{{\"type\":\"Feature\",\"properties\":{},\"geometry\":{{\"type\":\"Polygon\",\"coordinates\":[",
            serde_json::Value::Object(props)
        )
        .map_err(io)?;
        for (j, ring) in u.rings.iter().enumerate() {
            w.write_all(if j > 0 { b",[" } else { b"[" }).map_err(io)?;
            let closed = ring.first() == ring.last();
            let tail = if closed { None } else { ring.first() };
            for (k, &(x, y)) in ring.iter().chain(tail).enumerate() {
                if k > 0 {
                    w.write_all(b",").map_err(io)?;
                }
                write!(w, "[{},{}]", deg(x), deg(y)).map_err(io)?;
            }
            w.write_all(b"]").map_err(io)?;
        }
        w.write_all(b"]}}").map_err(io)?;
    }
    w.write_all(b"]}").map_err(io)?;
    w.flush().map_err(io)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use super::*;

    #[test]
    fn degrees_parse_back_to_the_grid_exactly() {
        for v in [
            0,
            1,
            -1,
            853_240_000,
            -853_240_001,
            1_799_999_999,
            277_172_123,
        ] {
            let d: f64 = deg(v).parse().unwrap();
            assert_eq!(crate::record::e7(d), Some(v), "{v}");
        }
    }

    #[test]
    fn a_number_comes_from_its_tag_then_its_name() {
        let area = |tags: &[(&str, &str)]| osm_areas::Area {
            tags: tags
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            rings: Vec::new(),
        };
        assert_eq!(
            number(&area(&[("ward", "7"), ("name", "X-03")]), "ward"),
            Some(7)
        );
        assert_eq!(number(&area(&[("name", "Kirtipur-03")]), "ward"), Some(3));
        assert_eq!(number(&area(&[("name", "Kirtipur")]), "ward"), None);
        assert_eq!(
            number(&area(&[("ward", "1433"), ("name", "Bheri-01")]), "ward"),
            Some(1)
        );
        assert_eq!(number(&area(&[("ward", "0")]), "ward"), None);
    }
}
