//! Snap rounding on the 1e-7° grid (Hobby 1999), for a source that is not noded.
//!
//! - **Hot pixels:** the unit square around every vertex, and around every proper crossing of
//!   two edges rounded to the nearest grid point. Every edge that meets a hot pixel's closed
//!   square is routed through the pixel's centre, in order along the edge. Rounded edges then
//!   meet only at shared vertices or along shared pieces, never cross (Hobby; Guibas & Marimont
//!   1998), so the arcs stage can cut them.
//! - **Small moves:** no input vertex moves. An edge moves by at most half a pixel diagonal
//!   (0.71 units, about 8 mm), only where it passes a hot pixel.
//! - **Cleanup:** a rounded ring loses consecutive repeats and the spikes (`p, q, p`) of edges
//!   that collapsed onto each other; a ring left with fewer than 3 vertices or no area is dropped.
//! - **Cost:** edges and hot pixels go into one uniform grid (the cell is the median edge
//!   length); an edge visits only the cells along its own slab, so a long edge is linear in its
//!   length, not in its box. A noded input (no bad pair) is returned unchanged.
//!
//! All tests are exact integer arithmetic (i128), so the result is the same on every machine.

#![allow(
    clippy::indexing_slicing,
    reason = "indices come from the same vectors (edge ids, ring positions): an out-of-range index is a forge bug and must stop the build, not be skipped"
)]

use std::collections::HashMap;

use super::arcs::{V, area2};
use super::simplify::crossings;

/// What snap rounding did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Snapped {
    /// Edge pairs that crossed or touched away from a shared vertex.
    pub(crate) bad_pairs: usize,
    /// Hot pixels made by rounding a crossing (not already a vertex).
    pub(crate) new_pixels: usize,
    /// Distinct edges routed through at least one hot pixel off their ends.
    pub(crate) edges_moved: usize,
    /// Vertices added to rings.
    pub(crate) vertices_added: usize,
    /// Rings dropped because they collapsed.
    pub(crate) rings_dropped: usize,
}

/// The undirected key of an edge.
fn key(a: V, b: V) -> (V, V) {
    if a < b { (a, b) } else { (b, a) }
}

/// The pixel holding `n / d`, `d > 0`: pixels are half-open, `[k − ½, k + ½)`, so this is
/// ⌊n / d + ½⌋.
fn pixel_of(n: i128, d: i128) -> i128 {
    (2 * n + d).div_euclid(2 * d)
}

/// The pixel of the crossing point of segments `a`–`b` and `c`–`d`; `None` when they
/// are parallel or do not cross in both interiors.
#[allow(
    clippy::many_single_char_names,
    reason = "segments a–b and c–d, their coordinates, parameters t and u"
)]
fn crossing(a: V, b: V, c: V, d: V) -> Option<V> {
    let f = |p: V| (i128::from(p.0), i128::from(p.1));
    let ((ax, ay), (bx, by), (cx, cy), (dx, dy)) = (f(a), f(b), f(c), f(d));
    let (rx, ry, sx, sy) = (bx - ax, by - ay, dx - cx, dy - cy);
    let den = rx * sy - ry * sx;
    if den == 0 {
        return None;
    }
    let t = (cx - ax) * sy - (cy - ay) * sx;
    let u = (cx - ax) * ry - (cy - ay) * rx;
    let (t, u, den) = if den < 0 { (-t, -u, -den) } else { (t, u, den) };
    if t <= 0 || t >= den || u <= 0 || u >= den {
        return None;
    }
    let x = pixel_of(ax * den + rx * t, den);
    let y = pixel_of(ay * den + ry * t, den);
    Some((i32::try_from(x).ok()?, i32::try_from(y).ok()?))
}

/// Whether segment `a`–`b` meets the half-open pixel of `h`, `[x − ½, x + ½) × [y − ½, y + ½)`
/// (Hobby's rule: closed squares let two routed pieces cross at a shared pixel corner).
///
/// Exact: in doubled coordinates the square's sides are integers. The segment is clipped to
/// the closed square (Liang–Barsky, parameters as fractions); it meets the half-open pixel
/// unless the clipped piece is empty or lies wholly on the top or the right side.
#[allow(
    clippy::many_single_char_names,
    reason = "segment a–b, pixel h, the doubling d, entry e and exit x"
)]
fn meets(a: V, b: V, h: V) -> bool {
    let d = |v: i32| 2 * i128::from(v);
    let (ax, ay, bx, by) = (d(a.0), d(a.1), d(b.0), d(b.1));
    let (x0, x1, y0, y1) = (d(h.0) - 1, d(h.0) + 1, d(h.1) - 1, d(h.1) + 1);
    if ax.max(bx) < x0 || ax.min(bx) > x1 || ay.max(by) < y0 || ay.min(by) > y1 {
        return false;
    }
    // t as (numerator, denominator > 0); lo = max of entries, hi = min of exits.
    let (mut lo, mut hi) = ((0_i128, 1_i128), (1_i128, 1_i128));
    let less = |p: (i128, i128), q: (i128, i128)| p.0 * q.1 < q.0 * p.1;
    for (a0, delta, s0, s1) in [(ax, bx - ax, x0, x1), (ay, by - ay, y0, y1)] {
        if delta == 0 {
            continue; // inside the slab already (the box test above)
        }
        let (mut e, mut x) = ((s0 - a0, delta), (s1 - a0, delta));
        if delta < 0 {
            (e, x) = ((a0 - s1, -delta), (a0 - s0, -delta));
        }
        if less(lo, e) {
            lo = e;
        }
        if less(x, hi) {
            hi = x;
        }
    }
    if less(hi, lo) {
        return false;
    }
    // The clipped piece's ends on a side: a0 + delta · t == side.
    let on =
        |a0: i128, delta: i128, side: i128, t: (i128, i128)| a0 * t.1 + delta * t.0 == side * t.1;
    let top = on(ay, by - ay, y1, lo) && on(ay, by - ay, y1, hi);
    let right = on(ax, bx - ax, x1, lo) && on(ax, bx - ax, x1, hi);
    !(top || right)
}

/// A uniform grid of cell `cell` from origin `(x0, y0)`.
pub(super) struct Grid {
    pub(super) x0: i64,
    pub(super) y0: i64,
    pub(super) cell: i64,
}

impl Grid {
    pub(super) fn cell_of(&self, p: V) -> (i64, i64) {
        (
            (i64::from(p.0) - self.x0).div_euclid(self.cell),
            (i64::from(p.1) - self.y0).div_euclid(self.cell),
        )
    }

    /// The cells within one unit of segment `a`–`b`: per column, the rows its slab spans (in
    /// f64 with a two-unit margin, far above its rounding error), so a long edge visits cells
    /// along itself only.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "grid coordinates are far below 2^52; the margin covers the f64 rounding"
    )]
    pub(super) fn cells(&self, a: V, b: V, out: &mut Vec<(i64, i64)>) {
        out.clear();
        let (ax, ay, bx, by) = (
            i64::from(a.0),
            i64::from(a.1),
            i64::from(b.0),
            i64::from(b.1),
        );
        let (lo, hi) = (ax.min(bx) - 1, ax.max(bx) + 1);
        let (c0, c1) = (
            (lo - self.x0).div_euclid(self.cell),
            (hi - self.x0).div_euclid(self.cell),
        );
        let y_at = |x: i64| {
            if ax == bx {
                (ay.min(by), ay.max(by))
            } else {
                let t = ((x - ax) as f64 / (bx - ax) as f64).clamp(0.0, 1.0);
                let y = (ay as f64 + t * (by - ay) as f64).round() as i64;
                (y, y)
            }
        };
        for cx in c0..=c1 {
            let xs = (self.x0 + cx * self.cell).max(lo);
            let xe = (self.x0 + (cx + 1) * self.cell - 1).min(hi);
            // A pixel in this column is met from up to one unit outside it.
            let (p, q) = (y_at(xs - 1), y_at(xe + 1));
            let (ylo, yhi) = (p.0.min(q.0) - 2, p.1.max(q.1) + 2);
            let (ylo, yhi) = (ylo.max(ay.min(by) - 1), yhi.min(ay.max(by) + 1));
            for cy in (ylo - self.y0).div_euclid(self.cell)..=(yhi - self.y0).div_euclid(self.cell)
            {
                out.push((cx, cy));
            }
        }
    }
}

/// Snap-rounds `rings` (`(unit, ring)`, open rings). Returns them unchanged when no two edges
/// cross or touch away from a shared vertex.
pub(crate) fn snap_round(rings: &[(u32, Vec<V>)]) -> (Vec<(u32, Vec<V>)>, Snapped) {
    let mut ids: HashMap<(V, V), usize> = HashMap::new();
    let mut edges: Vec<(V, V)> = Vec::new();
    for (_, r) in rings {
        for i in 0..r.len() {
            let k = key(r[i], r[(i + 1) % r.len()]);
            ids.entry(k).or_insert_with(|| {
                edges.push(k);
                edges.len() - 1
            });
        }
    }
    let bad = crossings(&edges);
    let mut rep = Snapped {
        bad_pairs: bad.len(),
        ..Snapped::default()
    };
    if bad.is_empty() {
        return (rings.to_vec(), rep);
    }
    // Hot pixels: every vertex, and every proper crossing rounded.
    let mut pixels: Vec<V> = edges.iter().flat_map(|&(a, b)| [a, b]).collect();
    pixels.sort_unstable();
    pixels.dedup();
    let vertices = pixels.len();
    pixels.extend(bad.iter().filter_map(|&(i, j)| {
        let ((a, b), (c, d)) = (edges[i], edges[j]);
        crossing(a, b, c, d)
    }));
    pixels.sort_unstable();
    pixels.dedup();
    rep.new_pixels = pixels.len() - vertices;
    // One grid for the pixels; each edge looks up the cells along it.
    let mut lens: Vec<i64> = edges
        .iter()
        .map(|&(a, b)| i64::from((a.0 - b.0).abs().max((a.1 - b.1).abs())))
        .collect();
    lens.sort_unstable();
    let x0 = pixels.iter().map(|p| i64::from(p.0)).min().unwrap_or(0) - 2;
    let y0 = pixels.iter().map(|p| i64::from(p.1)).min().unwrap_or(0) - 2;
    let grid = Grid {
        x0,
        y0,
        cell: lens[lens.len() / 2].max(16),
    };
    let mut cells: HashMap<(i64, i64), Vec<V>> = HashMap::new();
    for &p in &pixels {
        cells.entry(grid.cell_of(p)).or_default().push(p);
    }
    // Each edge's route: its pixels in order along it (by projection; ties by coordinates).
    let mut routes: Vec<Vec<V>> = Vec::with_capacity(edges.len());
    let mut scratch = Vec::new();
    for &(a, b) in &edges {
        grid.cells(a, b, &mut scratch);
        let mut on: Vec<(i128, V)> = Vec::new();
        for c in &scratch {
            for &h in cells.get(c).map_or(&[][..], Vec::as_slice) {
                if h != a && h != b && meets(a, b, h) {
                    let t = (i128::from(h.0) - i128::from(a.0))
                        * (i128::from(b.0) - i128::from(a.0))
                        + (i128::from(h.1) - i128::from(a.1)) * (i128::from(b.1) - i128::from(a.1));
                    on.push((t, h));
                }
            }
        }
        on.sort_unstable();
        on.dedup();
        if !on.is_empty() {
            rep.edges_moved += 1;
        }
        routes.push(on.into_iter().map(|(_, h)| h).collect());
    }
    // Rings through the routes, then cleaned.
    let mut out = Vec::with_capacity(rings.len());
    for (unit, r) in rings {
        let mut v: Vec<V> = Vec::with_capacity(r.len());
        for i in 0..r.len() {
            let (a, b) = (r[i], r[(i + 1) % r.len()]);
            v.push(a);
            let route = &routes[ids[&key(a, b)]];
            if a < b {
                v.extend(route);
            } else {
                v.extend(route.iter().rev());
            }
        }
        rep.vertices_added += v.len() - r.len();
        let v = despike(v);
        if v.len() >= 3 && area2(&v) != 0 {
            out.push((*unit, v));
        } else {
            rep.rings_dropped += 1;
        }
    }
    (out, rep)
}

/// Drops consecutive repeats and spikes (`p, q, p` → `p`) of a closed ring, until none is left.
fn despike(ring: Vec<V>) -> Vec<V> {
    let mut st: Vec<V> = Vec::with_capacity(ring.len());
    for p in ring {
        if st.last() == Some(&p) {
            continue;
        }
        if st.len() >= 2 && st[st.len() - 2] == p {
            st.pop();
            continue;
        }
        st.push(p);
    }
    // The seam: the ring wraps from its last vertex to its first.
    loop {
        let n = st.len();
        if n >= 2 && st[0] == st[n - 1] {
            st.pop();
        } else if n >= 3 && st[n - 2] == st[0] {
            st.pop();
            st.pop();
        } else if n >= 3 && st[1] == st[n - 1] {
            st.remove(0);
            st.pop();
        } else {
            break;
        }
    }
    st
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "test code: small model values"
    )]

    use super::*;

    #[test]
    fn crossings_round_to_the_nearest_point() {
        assert_eq!(crossing((0, 0), (10, 10), (0, 10), (10, 0)), Some((5, 5)));
        assert_eq!(crossing((0, 0), (3, 1), (0, 1), (3, 0)), Some((2, 1)));
        // Halves round up, on either side of zero.
        assert_eq!(crossing((-2, -2), (1, 1), (-2, 1), (1, -2)), Some((0, 0)));
        assert_eq!(crossing((-3, -3), (0, 0), (-3, 0), (0, -3)), Some((-1, -1)));
        assert_eq!(crossing((0, 0), (10, 0), (0, 1), (10, 1)), None);
        // Touching at an end is not a proper crossing.
        assert_eq!(crossing((0, 0), (10, 0), (5, 0), (5, 5)), None);
    }

    #[test]
    fn a_pixel_is_half_open() {
        assert!(meets((0, 0), (10, 0), (5, 0)));
        // Half a unit away touches the square's edge; just beyond, not.
        assert!(meets((0, 0), (10, 1), (5, 0)));
        assert!(!meets((0, 0), (10, 0), (5, 2)));
        assert!(!meets((0, 0), (10, 0), (12, 0)));
        // Corners: a segment through only the bottom-left corner meets the pixel; through only
        // the top-left, top-right or bottom-right corner, not.
        assert!(meets((4, 0), (5, -1), (5, 0)));
        assert!(!meets((4, 0), (5, 1), (5, 0)));
        assert!(!meets((5, 1), (6, 0), (5, 0)));
        assert!(!meets((5, -1), (6, 0), (5, 0)));
        assert!(!meets((0, 1), (1, 0), (0, 0)));
        assert!(!meets((0, 2), (2, 0), (0, 0)));
    }

    #[test]
    fn spikes_and_seams_are_removed() {
        assert_eq!(
            despike(vec![(0, 0), (5, 0), (6, 0), (5, 0), (5, 5)]),
            vec![(0, 0), (5, 0), (5, 5)]
        );
        assert_eq!(
            despike(vec![(1, 0), (2, 2), (0, 2), (0, 0), (1, 0), (0, 0)]),
            vec![(1, 0), (2, 2), (0, 2), (0, 0)]
        );
        assert!(despike(vec![(0, 0), (1, 1), (0, 0)]).len() < 3);
    }

    struct Rng(u64);
    impl Rng {
        fn below(&mut self, n: u64) -> i32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n) as i32
        }
    }

    /// Random overlapping quadrilaterals, wide (few collapses) and tiny (many): after rounding,
    /// no two edges cross or touch away from a shared vertex, and every kept ring keeps every
    /// input vertex.
    #[test]
    fn rounded_rings_never_cross() {
        for (size, spread) in [(40, 200), (2, 8)] {
            for seed in 1..2_000_u64 {
                let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
                let rings: Vec<(u32, Vec<V>)> = (0..6)
                    .map(|u| {
                        let (cx, cy) = (r.below(spread), r.below(spread));
                        let mut d = || size + r.below(size as u64 * 3 / 4 + 1);
                        let ring = vec![
                            (cx - d(), cy - d()),
                            (cx + d(), cy - d()),
                            (cx + d(), cy + d()),
                            (cx - d(), cy + d()),
                        ];
                        (u, ring)
                    })
                    .collect();
                let (out, rep) = snap_round(&rings);
                let mut segs: Vec<(V, V)> = out
                    .iter()
                    .flat_map(|(_, r)| (0..r.len()).map(|i| key(r[i], r[(i + 1) % r.len()])))
                    .collect();
                segs.sort_unstable();
                segs.dedup();
                let bad: Vec<_> = crossings(&segs)
                    .iter()
                    .map(|&(i, j)| (segs[i], segs[j]))
                    .collect();
                assert_eq!(bad, Vec::new(), "{size} seed {seed}: {rep:?}");
                for (u, b) in &out {
                    let a = &rings[*u as usize].1;
                    assert!(a.iter().all(|p| b.contains(p)), "{size} seed {seed}");
                }
            }
        }
    }

    #[test]
    fn a_noded_input_is_returned_unchanged() {
        let rings = vec![
            (0, vec![(0, 0), (10, 0), (10, 10), (0, 10)]),
            (1, vec![(10, 0), (20, 0), (20, 10), (10, 10)]),
        ];
        let (out, rep) = snap_round(&rings);
        assert_eq!(out, rings);
        assert_eq!(rep.bad_pairs, 0);
    }
}
