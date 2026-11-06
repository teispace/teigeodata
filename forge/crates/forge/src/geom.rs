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
        let p = (lon, lat);
        let mut inside = false;
        for ring in &self.rings {
            // Every edge, including the closing one (a no-op when the ring is closed).
            let closing = ring.last().zip(ring.first());
            let edges = ring
                .windows(2)
                .filter_map(|w| Some((*w.first()?, *w.get(1)?)));
            for (from, to) in edges.chain(closing.map(|(l, f)| (*l, *f))) {
                if on_segment(from, to, p) {
                    return true;
                }
                if (from.1 > lat) != (to.1 > lat) {
                    // Sign of (to − from) × (p − from), exact.
                    let side = cross(from, to, p);
                    if (to.1 > from.1 && side > 0) || (to.1 < from.1 && side < 0) {
                        inside = !inside;
                    }
                }
            }
        }
        inside
    }
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
