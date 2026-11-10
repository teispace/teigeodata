//! A polygon's area on the WGS 84 ellipsoid and its label point (D-088 §3.5).
//!
//! - **Area:** each vertex's latitude goes to the authalic sphere, which preserves area exactly
//!   (q from the ellipsoid's series in sin φ), and the ring's area is the trapezoid sum
//!   A = −(a²/4) Σ (λᵢ₊₁ − λᵢ₋₁) qᵢ, exact up to the edge path (geodesic against the authalic
//!   parallel trapezoid), negligible for edges under a few km.
//! - **Label:** the pole of inaccessibility (Garcia-Castellanos & Lombardo 2007; the polylabel
//!   method): a best-first search over square cells for the point inside the polygon farthest
//!   from its border, in metres on a local plane, to a precision of 1/1000 of the polygon's
//!   size and at least 10 m. The point is rounded to the grid and kept only if exactly inside;
//!   otherwise the polygon's interior point is used.
//!
//! Only IEEE +, −, ×, ÷ and √ (and [`crate::num`]'s series), so results are the same on every
//! machine.

#![allow(
    clippy::indexing_slicing,
    reason = "indices come from the same vectors (ring positions, arc vertices, segment ids): an out-of-range index is a forge bug and must stop the build, not be skipped"
)]

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use super::arcs::{V, ring_side};
use crate::num::{cos_deg, sin_deg};

/// WGS 84 semi-major axis, metres.
const A: f64 = 6_378_137.0;
/// WGS 84 first eccentricity squared.
const E2: f64 = 0.006_694_379_990_141_317;
/// Radians per 1e-7°.
const RAD_PER_UNIT: f64 = std::f64::consts::PI / 1.8e9;
/// Metres per 1e-7° of latitude.
const M_PER_UNIT: f64 = 111_195.08e-7;

/// The authalic q(φ) from sin φ: (1 − e²) [s / (1 − e² s²) + atanh(e s) / e], the series of
/// atanh to e¹⁰ (terms below 10⁻¹⁵).
fn q(s: f64) -> f64 {
    let (e2s2, mut term, mut sum) = (E2 * s * s, s, s);
    for k in 1..=6_u8 {
        term *= e2s2;
        sum += term / f64::from(2 * k + 1);
    }
    (1.0 - E2) * (s / (1.0 - e2s2) + sum)
}

/// The signed area of an open ring, m² (positive counter-clockwise).
pub(crate) fn ring_area_m2(ring: &[V]) -> f64 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }
    let qs: Vec<f64> = ring
        .iter()
        .map(|p| q(sin_deg(f64::from(p.1) * 1e-7)))
        .collect();
    let mut sum = 0.0;
    for i in 0..n {
        let (prev, next) = (ring[(i + n - 1) % n], ring[(i + 1) % n]);
        sum += (f64::from(next.0) - f64::from(prev.0)) * RAD_PER_UNIT * qs[i];
    }
    -(A * A / 4.0) * sum
}

/// A cell of the label search.
struct Cell {
    x: f64,
    y: f64,
    h: f64,
    d: f64,
    max: f64,
}

impl PartialEq for Cell {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}
impl Eq for Cell {}
impl PartialOrd for Cell {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Cell {
    fn cmp(&self, o: &Self) -> Ordering {
        self.max
            .total_cmp(&o.max)
            .then(o.x.total_cmp(&self.x))
            .then(o.y.total_cmp(&self.y))
    }
}

/// A part on a local plane in metres: rings of (x, y), and the plane's origin and scale.
struct Plane {
    rings: Vec<Vec<(f64, f64)>>,
    o: V,
    kx: f64,
}

impl Plane {
    fn new(rings: &[Vec<V>]) -> Option<Self> {
        let o = *rings.first()?.first()?;
        // The scale at the outer ring's middle latitude, never below cos 89°: a ring that
        // starts at a pole (Antarctica's) would otherwise have no width at all.
        let (lo, hi) = rings
            .first()?
            .iter()
            .fold((i32::MAX, i32::MIN), |(lo, hi), p| {
                (lo.min(p.1), hi.max(p.1))
            });
        let mid = (f64::from(lo) + f64::from(hi)) * 0.5e-7;
        let kx = M_PER_UNIT * cos_deg(mid).max(cos_deg(89.0));
        let rings = rings
            .iter()
            .map(|r| {
                r.iter()
                    .map(|p| (f64::from(p.0 - o.0) * kx, f64::from(p.1 - o.1) * M_PER_UNIT))
                    .collect()
            })
            .collect();
        Some(Self { rings, o, kx })
    }

    /// Signed distance from (x, y) to the border: positive inside.
    #[allow(
        clippy::many_single_char_names,
        reason = "a point x, y against segments a, b"
    )]
    fn dist(&self, x: f64, y: f64) -> f64 {
        let (mut inside, mut best) = (false, f64::INFINITY);
        for r in &self.rings {
            let n = r.len();
            for i in 0..n {
                let (a, b) = (r[i], r[(i + 1) % n]);
                if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
                    inside = !inside;
                }
                let (bx, by) = (b.0 - a.0, b.1 - a.1);
                let len2 = bx * bx + by * by;
                let t = if len2 > 0.0 {
                    (((x - a.0) * bx + (y - a.1) * by) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (dx, dy) = (x - a.0 - t * bx, y - a.1 - t * by);
                best = best.min(dx * dx + dy * dy);
            }
        }
        let d = best.sqrt();
        if inside { d } else { -d }
    }

    /// The plane with every ring thinned by Douglas–Peucker at `eps` metres; a ring that would
    /// fall below 4 vertices is kept whole.
    fn thinned(self, eps: f64) -> Self {
        let rings = self
            .rings
            .into_iter()
            .map(|r| {
                let t = thin_ring(&r, eps);
                if t.len() >= 4 { t } else { r }
            })
            .collect();
        Self { rings, ..self }
    }

    fn cell(&self, x: f64, y: f64, h: f64) -> Cell {
        let d = self.dist(x, y);
        Cell {
            x,
            y,
            h,
            d,
            max: d + h * std::f64::consts::SQRT_2,
        }
    }

    fn to_grid(&self, x: f64, y: f64) -> V {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a point inside the part, on the grid"
        )]
        let p = (
            (x / self.kx).round() as i32 + self.o.0,
            (y / M_PER_UNIT).round() as i32 + self.o.1,
        );
        p
    }
}

/// The label search's work budget in edge distances (4 per step and edge): about a second.
const LABEL_BUDGET: usize = 50_000_000;

/// Douglas–Peucker on an open ring (the last vertex joins the first): split at the vertex
/// farthest from the first, then each chain keeps the vertices more than `eps` from its chord.
#[allow(
    clippy::many_single_char_names,
    reason = "a chain a..b, its ends p and q, vertex v: the algorithm's own names"
)]
fn thin_ring(r: &[(f64, f64)], eps: f64) -> Vec<(f64, f64)> {
    let n = r.len();
    if n < 8 {
        return r.to_vec();
    }
    let d2 = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2);
    let far = (1..n)
        .max_by(|&i, &j| d2(r[0], r[i]).total_cmp(&d2(r[0], r[j])))
        .unwrap_or(n / 2);
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[far] = true;
    // Chains as (start, end) indices into the ring closed by index n ≡ 0.
    let mut stack = vec![(0, far), (far, n)];
    let at = |i: usize| r[i % n];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (p, q) = (at(a), at(b));
        let (dx, dy) = (q.0 - p.0, q.1 - p.1);
        let len2 = dx * dx + dy * dy;
        let (mut best, mut worst) = (a, 0.0_f64);
        for i in a + 1..b {
            let v = at(i);
            let d = if len2 > 0.0 {
                let t = (((v.0 - p.0) * dx + (v.1 - p.1) * dy) / len2).clamp(0.0, 1.0);
                d2(v, (p.0 + t * dx, p.1 + t * dy))
            } else {
                d2(v, p)
            };
            if d > worst {
                (best, worst) = (i, d);
            }
        }
        if worst > eps * eps {
            keep[best % n] = true;
            stack.push((a, best));
            stack.push((best, b));
        }
    }
    r.iter()
        .zip(&keep)
        .filter_map(|(&v, &k)| k.then_some(v))
        .collect()
}

/// The label point of a part (its outer ring first, then holes): the pole of inaccessibility,
/// or `None` if it cannot be placed exactly inside.
pub(crate) fn label(rings: &[Vec<V>]) -> Option<V> {
    let plane = Plane::new(rings)?;
    let outer = plane.rings.first()?;
    let (mut x0, mut y0, mut x1, mut y1) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for &(x, y) in outer {
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    // The first cells are squares of the shorter side, at most 1,000 along the longer one.
    let size = (x1 - x0).min(y1 - y0).max((x1 - x0).max(y1 - y0) / 1000.0);
    if size <= 0.0 {
        return None;
    }
    let precision = (size / 1000.0).max(10.0);
    // The search measures every edge per cell: on rings thinned to a quarter of the precision
    // each distance moves by at most that, so the answer keeps its precision, and a country's
    // ring of 10^5 vertices shrinks by orders of magnitude. The result is checked on `rings`.
    let plane = plane.thinned(precision / 4.0);
    // A budget of edge distances, not of steps: a ring the search cannot settle (Antarctica's,
    // along the pole and the antimeridian) ends in bounded time like any other.
    let edges: usize = plane.rings.iter().map(Vec::len).sum();
    let max_steps = (LABEL_BUDGET / edges.max(1)).clamp(1_000, 200_000);
    let h = size / 2.0;
    let mut heap = BinaryHeap::new();
    let mut x = x0;
    while x < x1 {
        let mut y = y0;
        while y < y1 {
            heap.push(plane.cell(x + h, y + h, h));
            y += size;
        }
        x += size;
    }
    let mut best = plane.cell(f64::midpoint(x0, x1), f64::midpoint(y0, y1), 0.0);
    let mut steps = 0_usize;
    while let Some(c) = heap.pop() {
        steps += 1;
        if c.d > best.d {
            best = Cell { max: c.d, ..c };
        }
        if c.max - best.d <= precision || steps > max_steps {
            continue;
        }
        let h = c.h / 2.0;
        for (dx, dy) in [(-h, -h), (h, -h), (-h, h), (h, h)] {
            heap.push(plane.cell(c.x + dx, c.y + dy, h));
        }
    }
    let p = plane.to_grid(best.x, best.y);
    let inside = rings.first().and_then(|o| ring_side(o, p)) == Some(true)
        && rings.iter().skip(1).all(|h| ring_side(h, p) == Some(false));
    inside.then_some(p)
}

#[cfg(test)]
mod tests {
    use super::{label, ring_area_m2, thin_ring};

    #[test]
    fn thinning_keeps_corners_and_drops_collinear_vertices() {
        // A 100 × 100 square with 99 extra vertices on each side, 1 cm off the line.
        let mut r = Vec::new();
        for (a, b) in [
            ((0.0, 0.0), (100.0, 0.0)),
            ((100.0, 0.0), (100.0, 100.0)),
            ((100.0, 100.0), (0.0, 100.0)),
            ((0.0, 100.0), (0.0, 0.0)),
        ] {
            for i in 0..100 {
                let t = f64::from(i) / 100.0;
                let off = if i % 2 == 1 { 0.01 } else { 0.0 };
                r.push((a.0 + t * (b.0 - a.0) + off, a.1 + t * (b.1 - a.1) + off));
            }
        }
        let t = thin_ring(&r, 1.0);
        assert_eq!(t.len(), 4, "{t:?}");
        for c in [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)] {
            assert!(t.contains(&c), "{c:?} in {t:?}");
        }
        // Below the offset, everything stays.
        assert_eq!(thin_ring(&r, 0.001).len(), r.len());
    }

    #[test]
    fn area_of_a_degree_square_and_a_label_inside() {
        // 1° × 1° at the equator: the exact ellipsoidal zone, (a²/2) Δλ q(1°) with q's closed
        // form (log, not our series), is 12,308,463,894 m²; a quadrangle's edges are a parallel
        // and meridians, which the trapezoid sum integrates exactly.
        let sq = vec![
            (0, 0),
            (10_000_000, 0),
            (10_000_000, 10_000_000),
            (0, 10_000_000),
        ];
        let a = ring_area_m2(&sq);
        assert!((a - 12_308_463_894.0).abs() < 1.0, "{a}");
        let mut cw = sq.clone();
        cw.reverse();
        assert!((ring_area_m2(&cw) + a).abs() < 1.0);
        // A U shape: the label lies in an arm, not in the notch.
        let u = vec![
            (0, 0),
            (3000, 0),
            (3000, 3000),
            (2000, 3000),
            (2000, 1000),
            (1000, 1000),
            (1000, 3000),
            (0, 3000),
        ];
        let p = label(&[u]).unwrap_or_default();
        assert!(!(p.0 > 1000 && p.0 < 2000 && p.1 > 1000), "{p:?}");
        // A hole in the middle pushes the label off-centre.
        let outer = vec![(0, 0), (9000, 0), (9000, 9000), (0, 9000)];
        let hole = vec![(3000, 3000), (3000, 6000), (6000, 6000), (6000, 3000)];
        let p = label(&[outer, hole]).unwrap_or_default();
        assert!(
            !(p.0 > 3000 && p.0 < 6000 && p.1 > 3000 && p.1 < 6000),
            "{p:?}"
        );
    }
}
