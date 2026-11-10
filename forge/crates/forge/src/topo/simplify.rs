//! Levels of detail for shared arcs: Douglas–Peucker importance per vertex, then a guard that
//! keeps every level free of crossings and of degenerate rings (D-088 §3.2).
//!
//! - **Importance** of a vertex is the distance, in metres on the arc's local scale, at which the
//!   recursive Douglas–Peucker split removes it, capped by its parent split's importance so that
//!   levels nest (mapshaper's scheme). Arc ends are pinned. A closed arc also pins its farthest
//!   vertex from its anchor and the farthest from that chord, so it never falls below a triangle.
//! - **Level** `l` (1 to 6) keeps a vertex whose importance exceeds tolerance `l`
//!   ([`TOLERANCE_CM`]); level 7 is pinned vertices only.
//! - **Guard,** from the coarsest level to the finest (a vertex added at a coarse level is kept at
//!   every finer one, so finer levels are checked after): kept segments that cross or touch away
//!   from a shared end, and rings left with no area or the wrong orientation, get back the most
//!   important vertex removed between their kept ends, until none is left.
//!
//! The arithmetic is IEEE (+, −, ×, ÷, √) with the scale from [`crate::num::cos_deg`], so the
//! levels are the same on every machine.

#![allow(
    clippy::indexing_slicing,
    reason = "indices come from the same vectors (ring positions, arc vertices, segment ids): an out-of-range index is a forge bug and must stop the build, not be skipped"
)]

use std::collections::{BTreeSet, HashMap};

use super::arcs::{Ref, V, area2, orient};
use crate::num::cos_deg;

/// Tolerances of levels 1 to 6, centimetres: 5 m, 20 m, 100 m, 500 m, 2 km, 5 km (the
/// geometry note's ladder, z4 to z14 and beyond).
pub(crate) const TOLERANCE_CM: [u32; 6] = [500, 2_000, 10_000, 50_000, 200_000, 500_000];
/// The pinned level.
pub(crate) const PINNED: u8 = 7;
/// Metres per 1e-7° of latitude.
const M_PER_UNIT: f64 = 111_195.08e-7;
/// The most guard rounds per level.
const ROUNDS: usize = 64;

/// Distance from `p` to segment `a`–`b`, metres, with `kx` metres per unit of longitude.
fn seg_dist(p: V, a: V, b: V, kx: f64) -> f64 {
    let (px, py) = (f64::from(p.0 - a.0) * kx, f64::from(p.1 - a.1) * M_PER_UNIT);
    let (bx, by) = (f64::from(b.0 - a.0) * kx, f64::from(b.1 - a.1) * M_PER_UNIT);
    let len2 = bx * bx + by * by;
    let t = if len2 > 0.0 {
        ((px * bx + py * by) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (dx, dy) = (px - t * bx, py - t * by);
    (dx * dx + dy * dy).sqrt()
}

/// Each vertex's importance in metres (`f64::INFINITY` for pinned vertices).
pub(crate) fn importance(arc: &[V]) -> Vec<f64> {
    let n = arc.len();
    let mut imp = vec![0.0; n];
    if n == 0 {
        return imp;
    }
    let lat = arc.iter().map(|p| f64::from(p.1)).sum::<f64>()
        / f64::from(u32::try_from(n).unwrap_or(u32::MAX))
        * 1e-7;
    let kx = M_PER_UNIT * cos_deg(lat);
    imp[0] = f64::INFINITY;
    imp[n - 1] = f64::INFINITY;
    let mut stack: Vec<(usize, usize, f64)> = Vec::new();
    if n >= 4 && arc[0] == arc[n - 1] {
        // A closed arc: pin the farthest vertex from the anchor, then the farthest from that chord.
        let far = (1..n - 1).fold(1, |best, i| {
            if seg_dist(arc[i], arc[0], arc[0], kx) > seg_dist(arc[best], arc[0], arc[0], kx) {
                i
            } else {
                best
            }
        });
        let third =
            (1..n - 1)
                .filter(|&i| i != far)
                .fold(None, |best: Option<usize>, i| match best {
                    Some(b)
                        if seg_dist(arc[b], arc[0], arc[far], kx)
                            >= seg_dist(arc[i], arc[0], arc[far], kx) =>
                    {
                        Some(b)
                    }
                    _ => Some(i),
                });
        imp[far] = f64::INFINITY;
        let mut cuts = vec![0, far, n - 1];
        if let Some(t) = third {
            imp[t] = f64::INFINITY;
            cuts.push(t);
        }
        cuts.sort_unstable();
        for w in cuts.windows(2) {
            stack.push((w[0], w[1], f64::INFINITY));
        }
    } else {
        stack.push((0, n - 1, f64::INFINITY));
    }
    while let Some((i, j, cap)) = stack.pop() {
        if j <= i + 1 {
            continue;
        }
        let (mut k, mut d) = (i + 1, -1.0);
        for m in i + 1..j {
            let dm = seg_dist(arc[m], arc[i], arc[j], kx);
            if dm > d {
                (k, d) = (m, dm);
            }
        }
        imp[k] = d.min(cap);
        stack.push((i, k, imp[k]));
        stack.push((k, j, imp[k]));
    }
    imp
}

/// The level of a vertex of importance `imp`.
pub(crate) fn level(imp: f64) -> u8 {
    if imp.is_infinite() {
        return PINNED;
    }
    let n = TOLERANCE_CM
        .iter()
        .filter(|&&t| imp > f64::from(t) / 100.0)
        .count();
    u8::try_from(n).unwrap_or(PINNED - 1)
}

/// A kept segment: arc, and the indices of its two kept ends.
type Seg = (usize, usize, usize);

/// The kept vertex indices of arc `a` at `lod`.
fn kept(levels: &[u8], lod: u8) -> impl Iterator<Item = usize> + '_ {
    levels
        .iter()
        .enumerate()
        .filter(move |(_, l)| **l >= lod)
        .map(|(i, _)| i)
}

/// Whether two segments meet anywhere but at a shared end (crossing, touching or overlapping).
#[allow(
    clippy::many_single_char_names,
    reason = "segments a–b and c–d, ends o, p, q"
)]
pub(crate) fn bad_pair(s: (V, V), t: (V, V)) -> bool {
    let ((a, b), (c, d)) = (s, t);
    let in_box = |p: V, q: V, r: V| {
        r.0 >= p.0.min(q.0) && r.0 <= p.0.max(q.0) && r.1 >= p.1.min(q.1) && r.1 <= p.1.max(q.1)
    };
    let shared = [a == c, a == d, b == c, b == d];
    if shared.iter().filter(|&&x| x).count() >= 2 {
        // The same segment twice (or a zero-length one): an overlap.
        return true;
    }
    if let Some(k) = shared.iter().position(|&x| x) {
        // One shared end: bad only if the other ends overlap along the same ray.
        let (o, p, q) = match k {
            0 => (a, b, d),
            1 => (a, b, c),
            2 => (b, a, d),
            _ => (b, a, c),
        };
        let dot = i128::from(p.0 - o.0) * i128::from(q.0 - o.0)
            + i128::from(p.1 - o.1) * i128::from(q.1 - o.1);
        return orient(o, p, q) == 0 && dot > 0;
    }
    let (o1, o2, o3, o4) = (
        orient(a, b, c),
        orient(a, b, d),
        orient(c, d, a),
        orient(c, d, b),
    );
    if o1 * o2 < 0 && o3 * o4 < 0 {
        return true;
    }
    (o1 == 0 && in_box(a, b, c))
        || (o2 == 0 && in_box(a, b, d))
        || (o3 == 0 && in_box(c, d, a))
        || (o4 == 0 && in_box(c, d, b))
}

/// Every pair of segments that is [`bad_pair`], each pair once, through a uniform grid.
pub(crate) fn crossings(segs: &[(V, V)]) -> Vec<(usize, usize)> {
    if segs.len() < 2 {
        return Vec::new();
    }
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    let mut lens: Vec<i64> = Vec::with_capacity(segs.len());
    for &(a, b) in segs {
        x0 = x0.min(i64::from(a.0.min(b.0)));
        y0 = y0.min(i64::from(a.1.min(b.1)));
        x1 = x1.max(i64::from(a.0.max(b.0)));
        y1 = y1.max(i64::from(a.1.max(b.1)));
        lens.push(i64::from((a.0 - b.0).abs().max((a.1 - b.1).abs())));
    }
    lens.sort_unstable();
    let cell = lens[lens.len() / 2].max(((x1 - x0).max(y1 - y0) / 4096).max(1));
    let cells = |a: V, b: V| {
        let c = |v: i32, o: i64| (i64::from(v) - o) / cell;
        (
            c(a.0.min(b.0), x0),
            c(a.1.min(b.1), y0),
            c(a.0.max(b.0), x0),
            c(a.1.max(b.1), y0),
        )
    };
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, &(a, b)) in segs.iter().enumerate() {
        let (cx0, cy0, cx1, cy1) = cells(a, b);
        for cx in cx0..=cx1 {
            for cy in cy0..=cy1 {
                grid.entry((cx, cy)).or_default().push(i);
            }
        }
    }
    let mut out = Vec::new();
    let mut keys: Vec<&(i64, i64)> = grid.keys().collect();
    keys.sort_unstable();
    for k in keys {
        let v = &grid[k];
        for (x, &i) in v.iter().enumerate() {
            for &j in &v[x + 1..] {
                let (ci, cj) = (cells(segs[i].0, segs[i].1), cells(segs[j].0, segs[j].1));
                // Test a pair only in the first cell both cover.
                if (ci.0.max(cj.0), ci.1.max(cj.1)) == *k && bad_pair(segs[i], segs[j]) {
                    out.push((i.min(j), i.max(j)));
                }
            }
        }
    }
    out.sort_unstable();
    out
}

/// What the guard did at one level.
#[derive(Clone, Debug, Default)]
pub(crate) struct Guarded {
    /// Vertices kept at this level.
    pub(crate) kept: usize,
    /// Crossings found before the fixes.
    pub(crate) crossings: usize,
    /// Degenerate rings found before the fixes.
    pub(crate) rings: usize,
    /// Vertices added back.
    pub(crate) added: usize,
    /// Problems left that no removed vertex could fix.
    pub(crate) left: usize,
}

/// Raises the most important removed vertex between kept ends `i` and `j` of arc `a` to `lod`;
/// whether there was one.
fn add_back(
    levels: &mut [Vec<u8>],
    imp: &[Vec<f64>],
    a: usize,
    i: usize,
    j: usize,
    lod: u8,
) -> bool {
    let Some(k) = (i + 1..j)
        .filter(|&k| levels[a][k] < lod)
        .max_by(|&x, &y| imp[a][x].total_cmp(&imp[a][y]).then(y.cmp(&x)))
    else {
        return false;
    };
    levels[a][k] = lod;
    true
}

/// The kept segments of every arc at `lod`.
fn segments(arcs: &[Vec<V>], levels: &[Vec<u8>], lod: u8) -> (Vec<(V, V)>, Vec<Seg>) {
    let (mut pts, mut ids) = (Vec::new(), Vec::new());
    for (a, arc) in arcs.iter().enumerate() {
        let mut prev: Option<usize> = None;
        for k in kept(&levels[a], lod) {
            if let Some(p) = prev {
                pts.push((arc[p], arc[k]));
                ids.push((a, p, k));
            }
            prev = Some(k);
        }
    }
    (pts, ids)
}

/// Twice the signed area of a ring at `lod`, from its kept vertices.
fn ring_area2(arcs: &[Vec<V>], levels: &[Vec<u8>], refs: &[Ref], lod: u8) -> (i128, usize) {
    let mut pts: Vec<V> = Vec::new();
    for &r in refs {
        let a = (r >> 1) as usize;
        let mut ks: Vec<usize> = kept(&levels[a], lod).collect();
        if r & 1 == 1 {
            ks.reverse();
        }
        let skip = usize::from(!pts.is_empty());
        pts.extend(ks.into_iter().skip(skip).map(|k| arcs[a][k]));
    }
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    (area2(&pts), pts.len())
}

/// Fixes the rings at `lod` that lost their area or orientation; how many failed and how many
/// could not be fixed.
fn fix_rings(
    arcs: &[Vec<V>],
    levels: &mut [Vec<u8>],
    imp: &[Vec<f64>],
    rings: &[(Vec<Ref>, bool)],
    lod: u8,
) -> (usize, usize, usize) {
    let (mut bad, mut added, mut stuck) = (0, 0, 0);
    for (refs, outer) in rings {
        let (a2, n) = ring_area2(arcs, levels, refs, lod);
        if n >= 3 && (a2 > 0) == *outer && a2 != 0 {
            continue;
        }
        bad += 1;
        // The most important removed vertex of the ring's arcs.
        let best = refs
            .iter()
            .map(|&r| (r >> 1) as usize)
            .flat_map(|a| {
                (0..arcs[a].len())
                    .filter(move |&k| k > 0)
                    .map(move |k| (a, k))
            })
            .filter(|&(a, k)| levels[a][k] < lod)
            .max_by(|&(a, k), &(b, m)| imp[a][k].total_cmp(&imp[b][m]).then((b, m).cmp(&(a, k))));
        match best {
            Some((a, k)) => {
                levels[a][k] = lod;
                added += 1;
            }
            None => stuck += 1,
        }
    }
    (bad, added, stuck)
}

/// Levels for every arc, guarded against crossings and degenerate `rings` (each with whether it
/// is an outer ring) at every level; the report per level, coarsest first. An arc marked in
/// `pinned` keeps every vertex at every level (D-154): it is a line another pack draws.
pub(crate) fn levels(
    arcs: &[Vec<V>],
    rings: &[(Vec<Ref>, bool)],
    pinned: &[bool],
) -> (Vec<Vec<u8>>, Vec<(u8, Guarded)>) {
    levels_to(arcs, rings, pinned, 1)
}

/// [`levels`] guarded from the coarsest level down to `finest` only: a level finer than
/// `finest` is left unguarded (D-155's pre-simplification keeps one level).
pub(crate) fn levels_to(
    arcs: &[Vec<V>],
    rings: &[(Vec<Ref>, bool)],
    pinned: &[bool],
    finest: u8,
) -> (Vec<Vec<u8>>, Vec<(u8, Guarded)>) {
    use rayon::prelude::*;
    let imp: Vec<Vec<f64>> = arcs
        .par_iter()
        .enumerate()
        .map(|(i, a)| {
            if pinned.get(i).copied().unwrap_or(false) {
                vec![f64::INFINITY; a.len()]
            } else {
                importance(a)
            }
        })
        .collect();
    let mut levels: Vec<Vec<u8>> = imp
        .iter()
        .map(|v| v.iter().map(|&i| level(i)).collect())
        .collect();
    let mut report = Vec::new();
    for lod in (finest.max(1)..=PINNED).rev() {
        let mut g = Guarded::default();
        for round in 0..ROUNDS {
            let (pts, ids) = segments(arcs, &levels, lod);
            let bad = crossings(&pts);
            let mut fix: BTreeSet<Seg> = BTreeSet::new();
            for &(i, j) in &bad {
                fix.insert(ids[i]);
                fix.insert(ids[j]);
            }
            let mut added = 0;
            for &(a, i, j) in &fix {
                if add_back(&mut levels, &imp, a, i, j, lod) {
                    added += 1;
                }
            }
            let (bad_rings, ring_added, stuck) = fix_rings(arcs, &mut levels, &imp, rings, lod);
            if round == 0 {
                (g.crossings, g.rings) = (bad.len(), bad_rings);
            }
            g.added += added + ring_added;
            if added + ring_added == 0 {
                g.left = bad.len() + stuck;
                break;
            }
        }
        g.kept = levels
            .iter()
            .map(|l| l.iter().filter(|&&x| x >= lod).count())
            .sum();
        report.push((lod, g));
    }
    (levels, report)
}

#[cfg(test)]
mod tests {
    use super::{PINNED, bad_pair, crossings, importance, level};

    #[test]
    fn importance_nests_and_pins() {
        // A zigzag: the middle spike is the most important interior vertex.
        let arc = [
            (0, 0),
            (10_000, 100),
            (20_000, 50_000),
            (30_000, 100),
            (40_000, 0),
        ];
        let imp = importance(&arc);
        assert!(imp[0].is_infinite() && imp[4].is_infinite());
        assert!(imp[2] > imp[1] && imp[2] > imp[3]);
        assert!(
            imp[1] <= imp[2] && imp[3] <= imp[2],
            "children are capped by their parent"
        );
        assert_eq!(level(f64::INFINITY), PINNED);
        assert_eq!((level(4.0), level(6.0), level(1e9)), (0, 1, 6));
        // A closed arc pins two vertices inside.
        let ring = [(0, 0), (1000, 0), (1000, 1000), (0, 1000), (0, 0)];
        assert_eq!(
            importance(&ring).iter().filter(|v| v.is_infinite()).count(),
            4
        );
    }

    #[test]
    fn crossing_rules() {
        let s = ((0, 0), (10, 10));
        assert!(bad_pair(s, ((0, 10), (10, 0))), "a proper crossing");
        assert!(!bad_pair(s, ((10, 10), (20, 0))), "a shared end");
        assert!(
            bad_pair(s, ((10, 10), (5, 5))),
            "an overlap from a shared end"
        );
        assert!(
            bad_pair(s, ((5, 5), (20, 0))),
            "a vertex on the other segment"
        );
        assert!(!bad_pair(s, ((20, 20), (30, 30))), "collinear, apart");
        let segs = [s, ((0, 10), (10, 0)), ((100, 100), (110, 100))];
        assert_eq!(crossings(&segs), vec![(0, 1)]);
    }
}
