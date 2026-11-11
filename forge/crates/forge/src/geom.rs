//! Polygons on the 1e-7° grid (ADR-0029) and exact point-in-polygon. Predicates use `i128`, so
//! no point is misclassified by rounding; a point exactly on an edge counts as inside. A
//! uniform grid over bounding boxes keeps lookups sub-linear in the number of polygons.

use std::collections::BTreeMap;

/// A polygon: an outer ring and its holes, each closed or not, as (lon, lat) on the grid.
#[derive(Clone, Debug, Default)]
pub(crate) struct Polygon {
    pub(crate) rings: Vec<Vec<(i32, i32)>>,
}

/// A bounding box (min lon, min lat, max lon, max lat).
pub(crate) type Bbox = (i32, i32, i32, i32);

impl Polygon {
    pub(crate) fn bbox(&self) -> Option<Bbox> {
        let mut it = self.rings.iter().flatten();
        let &(x, y) = it.next()?;
        Some(it.fold((x, y, x, y), |(a, b, c, d), &(x, y)| {
            (a.min(x), b.min(y), c.max(x), d.max(y))
        }))
    }

    /// Even-odd containment over all rings; on an edge counts as inside.
    pub(crate) fn contains(&self, lon: i32, lat: i32) -> bool {
        let mut inside = false;
        for ring in &self.rings {
            match ring_parity(ring, (lon, lat)) {
                None => return true,
                Some(odd) => inside ^= odd,
            }
        }
        inside
    }

    /// The area centroid (lon, lat on the grid), holes subtracted. A ring is a hole when an
    /// odd number of the other rings contain it, so ring orientation does not matter. Sums are
    /// exact in `i128`, relative to each ring's first vertex.
    pub(crate) fn centroid(&self) -> Option<(f64, f64)> {
        let (mut area2, mut cx, mut cy) = (0.0_f64, 0.0_f64, 0.0_f64);
        for (i, ring) in self.rings.iter().enumerate() {
            let Some(&o) = ring.first() else { continue };
            let nesting = self
                .rings
                .iter()
                .enumerate()
                .filter(|(j, other)| *j != i && ring_parity(other, o) == Some(true))
                .count();
            let (a, sx, sy) = ring_moments(ring, o);
            // Orientation-free: the ring's own sign is dropped and its nesting sets it.
            let sign = if nesting % 2 == 0 { 1.0 } else { -1.0 };
            let flip = if a < 0 { -1.0 } else { 1.0 };
            #[allow(
                clippy::cast_precision_loss,
                reason = "sums are formed exactly, then scaled once"
            )]
            let (a, sx, sy) = (a.abs() as f64, sx as f64, sy as f64);
            area2 += sign * a;
            // Moments are relative to `o`: shift back by o·A.
            cx += sign * (flip * sx / 3.0 + f64::from(o.0) * a);
            cy += sign * (flip * sy / 3.0 + f64::from(o.1) * a);
        }
        (area2 != 0.0).then(|| (cx / area2, cy / area2))
    }

    /// A point inside the polygon and off its edges, found the same way on every run: on a
    /// horizontal line through the box (the middle first, then the quarters and eighths), the
    /// middle of the widest even–odd inside interval, checked with [`Polygon::contains`] and by
    /// not lying on an edge. `None` when no line gives one (a polygon thinner than the grid).
    /// Phase 3's polylabel replaces it for labels (D-088); this one only promises "inside".
    #[allow(clippy::many_single_char_names, reason = "the scanline's coordinates")]
    pub(crate) fn interior_point(&self) -> Option<(i32, i32)> {
        let (_, y0, _, y1) = self.bbox()?;
        let (y0, h) = (i64::from(y0), i64::from(y1) - i64::from(y0));
        for k in [4_i64, 2, 6, 1, 3, 5, 7] {
            let Ok(y) = i32::try_from(y0 + h * k / 8) else {
                continue;
            };
            let mut xs: Vec<f64> = Vec::new();
            for ring in &self.rings {
                for (a, b) in edges(ring) {
                    // Half-open in y, so a vertex on the line is counted once.
                    if (a.1 <= y) != (b.1 <= y) {
                        let t = (f64::from(y) - f64::from(a.1)) / (f64::from(b.1) - f64::from(a.1));
                        xs.push(f64::from(a.0) + t * (f64::from(b.0) - f64::from(a.0)));
                    }
                }
            }
            xs.sort_by(f64::total_cmp);
            let widest = xs
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&[l, r]| (l, r))
                .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)));
            let Some((l, r)) = widest else { continue };
            #[allow(
                clippy::cast_possible_truncation,
                reason = "the middle of two grid longitudes is on the grid"
            )]
            let x = f64::midpoint(l, r).round() as i32;
            let on_edge = self
                .rings
                .iter()
                .any(|ring| ring_parity(ring, (x, y)).is_none());
            if r - l >= 2.0 && !on_edge && self.contains(x, y) {
                return Some((x, y));
            }
        }
        None
    }
}

/// `ring` cut to the box `area` (Sutherland–Hodgman, one box side at a time), closed, without
/// repeated vertices; empty when fewer than three vertices remain. Every point strictly inside
/// `area` has the same winding number about the cut ring as about `ring`, so containment there
/// is unchanged. Cut points lie exactly on the box's sides; their other coordinate is rounded to
/// the grid, which tilts a cut edge by under half a unit (5 mm).
pub(crate) fn clip_ring(ring: &[(i32, i32)], area: Bbox) -> Vec<(i32, i32)> {
    let (west, south, east, north) = area;
    let mut pts: Vec<(i32, i32)> = ring.to_vec();
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    // (axis 0 = lon / 1 = lat, the side's value, keep the side above it).
    for (axis, at, above) in [
        (0, west, true),
        (0, east, false),
        (1, south, true),
        (1, north, false),
    ] {
        let coord = |p: (i32, i32)| if axis == 0 { p.0 } else { p.1 };
        let inside = |p: (i32, i32)| {
            if above {
                coord(p) >= at
            } else {
                coord(p) <= at
            }
        };
        let cut = |a: (i32, i32), b: (i32, i32)| {
            let (ca, cb) = (i128::from(coord(a)), i128::from(coord(b)));
            let (oa, ob) = if axis == 0 { (a.1, b.1) } else { (a.0, b.0) };
            let (oa, ob) = (i128::from(oa), i128::from(ob));
            // oa + (ob − oa)·(at − ca)/(cb − ca), rounded half away from zero; cb ≠ ca since a
            // and b are on either side of `at`.
            let (num, den) = ((ob - oa) * (i128::from(at) - ca), cb - ca);
            let (num, den) = if den < 0 { (-num, -den) } else { (num, den) };
            let q = if num >= 0 {
                (2 * num + den) / (2 * den)
            } else {
                -((2 * -num + den) / (2 * den))
            };
            let other = i32::try_from(oa + q).unwrap_or(if q > 0 { i32::MAX } else { i32::MIN });
            if axis == 0 { (at, other) } else { (other, at) }
        };
        let Some(&last) = pts.last() else { break };
        let mut out = Vec::with_capacity(pts.len() + 4);
        let mut prev = last;
        for &cur in &pts {
            match (inside(prev), inside(cur)) {
                (true, true) => out.push(cur),
                (true, false) => out.push(cut(prev, cur)),
                (false, true) => {
                    out.push(cut(prev, cur));
                    out.push(cur);
                }
                (false, false) => {}
            }
            prev = cur;
        }
        pts = out;
    }
    pts.dedup();
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    if pts.len() < 3 {
        return Vec::new();
    }
    if let Some(&first) = pts.first() {
        pts.push(first);
    }
    pts
}

/// Edges of a ring, including the closing one (a zero-length no-op when the ring is closed).
fn edges(ring: &[(i32, i32)]) -> impl Iterator<Item = ((i32, i32), (i32, i32))> + '_ {
    let closing = ring.last().zip(ring.first()).map(|(l, f)| (*l, *f));
    ring.windows(2)
        .filter_map(|w| Some((*w.first()?, *w.get(1)?)))
        .chain(closing)
}

/// Whether a ray from `p` crosses the ring an odd number of times; `None` when `p` is on it.
fn ring_parity(ring: &[(i32, i32)], p: (i32, i32)) -> Option<bool> {
    let mut odd = false;
    for (from, to) in edges(ring) {
        if on_segment(from, to, p) {
            return None;
        }
        if (from.1 > p.1) != (to.1 > p.1) {
            // Sign of (to − from) × (p − from), exact.
            let side = cross(from, to, p);
            if (to.1 > from.1 && side > 0) || (to.1 < from.1 && side < 0) {
                odd = !odd;
            }
        }
    }
    Some(odd)
}

/// Twice the signed area and the first moments (before the /3) of a ring, relative to `o`:
/// Σ c, Σ (x₀+x₁)·c, Σ (y₀+y₁)·c with c = x₀y₁ − x₁y₀.
fn ring_moments(ring: &[(i32, i32)], o: (i32, i32)) -> (i128, i128, i128) {
    let w = |v: i32, base: i32| i128::from(v) - i128::from(base);
    edges(ring).fold((0, 0, 0), |(a, sx, sy), (p, q)| {
        let (x0, y0, x1, y1) = (w(p.0, o.0), w(p.1, o.1), w(q.0, o.0), w(q.1, o.1));
        let c = x0 * y1 - x1 * y0;
        (a + c, sx + (x0 + x1) * c, sy + (y0 + y1) * c)
    })
}

/// (b − a) × (p − a), widened before subtracting: grid differences reach 3.6e9, past `i32`.
fn cross(a: (i32, i32), b: (i32, i32), p: (i32, i32)) -> i128 {
    let w = i128::from;
    (w(b.0) - w(a.0)) * (w(p.1) - w(a.1)) - (w(p.0) - w(a.0)) * (w(b.1) - w(a.1))
}

fn on_segment(a: (i32, i32), b: (i32, i32), p: (i32, i32)) -> bool {
    cross(a, b, p) == 0
        && p.0 >= a.0.min(b.0)
        && p.0 <= a.0.max(b.0)
        && p.1 >= a.1.min(b.1)
        && p.1 <= a.1.max(b.1)
}

/// A grid over polygon bounding boxes: cell → indexes of the polygons whose box touches it.
#[derive(Debug)]
pub(crate) struct GridIndex {
    cell: i32,
    cells: BTreeMap<(i32, i32), Vec<u32>>,
    boxes: Vec<Bbox>,
}

impl GridIndex {
    /// `cell_e7` is the cell size on the grid (1e7 = 1°).
    pub(crate) fn new(polys: &[Polygon], cell_e7: i32) -> Self {
        let mut cells: BTreeMap<(i32, i32), Vec<u32>> = BTreeMap::new();
        let mut boxes = Vec::with_capacity(polys.len());
        for (i, p) in polys.iter().enumerate() {
            let b = p.bbox().unwrap_or((1, 1, 0, 0));
            boxes.push(b);
            if b.0 > b.2 {
                continue;
            }
            let i = u32::try_from(i).unwrap_or(u32::MAX);
            for cx in b.0.div_euclid(cell_e7)..=b.2.div_euclid(cell_e7) {
                for cy in b.1.div_euclid(cell_e7)..=b.3.div_euclid(cell_e7) {
                    cells.entry((cx, cy)).or_default().push(i);
                }
            }
        }
        Self {
            cell: cell_e7,
            cells,
            boxes,
        }
    }

    /// Indexes of polygons whose bounding box contains the point.
    pub(crate) fn candidates(&self, x: i32, y: i32) -> impl Iterator<Item = usize> + '_ {
        let key = (x.div_euclid(self.cell), y.div_euclid(self.cell));
        self.cells
            .get(&key)
            .into_iter()
            .flatten()
            .map(|&i| i as usize)
            .filter(move |&i| {
                self.boxes
                    .get(i)
                    .is_some_and(|b| x >= b.0 && x <= b.2 && y >= b.1 && y <= b.3)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{GridIndex, Polygon};

    #[test]
    fn a_clipped_ring_keeps_containment_inside_the_box() {
        use super::clip_ring;
        // A concave ring (a U) over the box (2, 2)–(8, 8), with a hole-like notch.
        let ring = vec![
            (0, 0),
            (10, 0),
            (10, 10),
            (6, 10),
            (6, 4),
            (4, 4),
            (4, 10),
            (0, 10),
            (0, 0),
        ];
        let area = (2, 2, 8, 8);
        let cut = clip_ring(&ring, area);
        assert_eq!(cut.first(), cut.last());
        assert!(
            cut.iter()
                .all(|&(x, y)| (2..=8).contains(&x) && (2..=8).contains(&y))
        );
        let whole = Polygon { rings: vec![ring] };
        let part = Polygon { rings: vec![cut] };
        for x in 3..8 {
            for y in 3..8 {
                if x == 4 || x == 6 || y == 4 {
                    continue; // on an edge of the notch: inside both ways
                }
                assert_eq!(whole.contains(x, y), part.contains(x, y), "({x}, {y})");
            }
        }
        // A ring off the box is gone; a box inside the ring is the box.
        assert_eq!(
            clip_ring(&[(20, 20), (30, 20), (30, 30), (20, 20)], area),
            []
        );
        let boxed = clip_ring(&[(0, 0), (10, 0), (10, 10), (0, 10), (0, 0)], area);
        assert_eq!(boxed.len(), 5);
        // A slanted edge is cut at its rounded crossing.
        let tri = clip_ring(&[(0, 0), (10, 0), (0, 10), (0, 0)], (0, 0, 10, 3));
        assert!(tri.contains(&(7, 3)) && tri.contains(&(0, 3)), "{tri:?}");
    }

    fn square(x0: i32, y0: i32, s: i32) -> Vec<(i32, i32)> {
        vec![
            (x0, y0),
            (x0 + s, y0),
            (x0 + s, y0 + s),
            (x0, y0 + s),
            (x0, y0),
        ]
    }

    #[test]
    fn contains_with_holes_and_edges() {
        let p = Polygon {
            rings: vec![square(0, 0, 100), square(40, 40, 20)],
        };
        assert!(p.contains(10, 10));
        assert!(!p.contains(50, 50), "inside the hole");
        assert!(p.contains(0, 50), "on the outer edge");
        assert!(p.contains(40, 50), "on the hole's edge");
        assert!(!p.contains(101, 50));
        assert!(!p.contains(-1, -1));
        // Edges spanning most of the grid: differences overflow i32.
        let wide = Polygon {
            rings: vec![vec![
                (-1_799_999_999, -899_999_999),
                (1_799_999_999, -899_999_999),
                (1_799_999_999, 899_999_999),
                (-1_799_999_999, 899_999_999),
            ]],
        };
        assert!(wide.contains(0, 0));
        assert!(!wide.contains(1_800_000_000, 0));
    }

    #[test]
    fn interior_point_avoids_holes_and_concavities() {
        let off_edge = |p: &Polygon, (x, y): (i32, i32)| {
            p.contains(x, y)
                && p.rings
                    .iter()
                    .all(|r| super::ring_parity(r, (x, y)).is_some())
        };
        // A hole across the middle line: the point lands beside it, never in it.
        let holed = Polygon {
            rings: vec![square(0, 0, 100), square(30, 30, 40)],
        };
        let p = holed.interior_point().expect("a point");
        assert!(off_edge(&holed, p) && !(30..=70).contains(&p.0), "{p:?}");
        // A C whose centroid and box middle are both outside it.
        let c = Polygon {
            rings: vec![vec![
                (0, 0),
                (100, 0),
                (100, 20),
                (20, 20),
                (20, 80),
                (100, 80),
                (100, 100),
                (0, 100),
                (0, 0),
            ]],
        };
        let p = c.interior_point().expect("a point");
        assert!(off_edge(&c, p), "{p:?}");
        // The same on every run, and on hostile grids far from the origin.
        assert_eq!(c.interior_point(), Some(p));
        let far = Polygon {
            rings: vec![square(1_799_999_000, 899_999_000, 900)],
        };
        assert!(far.interior_point().is_some_and(|q| off_edge(&far, q)));
        // Thinner than the grid, or empty: no point rather than a wrong one.
        let thin = Polygon {
            rings: vec![vec![(0, 0), (1, 0), (1, 100), (0, 100), (0, 0)]],
        };
        assert_eq!(thin.interior_point(), None);
        assert_eq!(Polygon { rings: vec![] }.interior_point(), None);
    }

    #[test]
    fn centroid_subtracts_holes_in_any_orientation() {
        let near = |a: Option<(f64, f64)>, b: (f64, f64)| {
            a.is_some_and(|(x, y)| (x - b.0).abs() < 1e-9 && (y - b.1).abs() < 1e-9)
        };
        let plain = Polygon {
            rings: vec![square(0, 0, 100)],
        };
        assert!(near(plain.centroid(), (50.0, 50.0)));
        let mut reversed = square(0, 0, 100);
        reversed.reverse();
        assert!(near(
            Polygon {
                rings: vec![reversed]
            }
            .centroid(),
            (50.0, 50.0)
        ));
        // A 100² square minus a 50² hole at its top right: the centroid moves down-left.
        let expect = (100.0 * 100.0 * 50.0 - 50.0 * 50.0 * 75.0) / (100.0 * 100.0 - 50.0 * 50.0);
        let holed = Polygon {
            rings: vec![square(0, 0, 100), square(50, 50, 50)],
        };
        assert!(near(holed.centroid(), (expect, expect)));
        let mut hole = square(50, 50, 50);
        hole.reverse();
        let holed = Polygon {
            rings: vec![square(0, 0, 100), hole],
        };
        assert!(
            near(holed.centroid(), (expect, expect)),
            "hole orientation does not matter"
        );
        // Two disjoint parts (a multipolygon flattened into rings).
        let two = Polygon {
            rings: vec![square(0, 0, 10), square(100, 0, 10)],
        };
        assert!(near(two.centroid(), (55.0, 5.0)));
        assert!(Polygon::default().centroid().is_none());
    }

    #[test]
    fn grid_finds_candidates() {
        let polys = vec![
            Polygon {
                rings: vec![square(0, 0, 100)],
            },
            Polygon {
                rings: vec![square(200, 0, 100)],
            },
        ];
        let g = GridIndex::new(&polys, 64);
        assert_eq!(g.candidates(10, 10).collect::<Vec<_>>(), vec![0]);
        assert_eq!(g.candidates(250, 50).collect::<Vec<_>>(), vec![1]);
        assert!(g.candidates(150, 50).next().is_none());
    }
}
