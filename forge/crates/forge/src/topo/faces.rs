//! A planar overlay by noding (D-363): rings of several sources snap-rounded together
//! ([`snap::snap_round`]), their segments made one planar graph, and its faces walked. Every
//! face is a cycle of half-edges with the face on its left; a unit made of faces is the set of
//! their half-edges whose twins it does not hold, so two units from one arrangement share each
//! border edge exactly and never overlap, by construction. Overlaying sources piece by piece
//! cannot promise that: each overlay rounds its crossings to the grid on its own, a centimetre
//! apart, and the topology stage refuses the overlaps that leaves.
//!
//! - Half-edge `h` runs along segment `h / 2`; its twin is `h ^ 1`.
//! - At each vertex the outgoing half-edges are sorted counter-clockwise (exact, `i128`), and
//!   a face's next half-edge after `u → v` is the one at `v` just clockwise of `v → u`.
//! - A bounded face's cycle runs counter-clockwise (positive area). A clockwise cycle bounds a
//!   connected part of the graph from outside: a hole of the face around that part, or the
//!   outside of everything.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use super::arcs::V;
use super::snap;

/// The arrangement of a set of rings.
pub(crate) struct Arrangement {
    /// Vertices.
    pts: Vec<V>,
    /// Each half-edge's start vertex.
    from: Vec<u32>,
    /// Each vertex's outgoing half-edges, counter-clockwise.
    out: Vec<Vec<u32>>,
    /// Each half-edge's position in its start vertex's `out`.
    pos: Vec<u32>,
    /// The cycles: every half-edge is in exactly one.
    pub(crate) faces: Vec<Face>,
    /// Each half-edge's face.
    pub(crate) face_of: Vec<u32>,
}

/// One cycle of half-edges.
pub(crate) struct Face {
    pub(crate) edges: Vec<u32>,
    /// Twice the signed area: positive for a bounded face.
    pub(crate) area2: i128,
    /// Its length, grid units.
    pub(crate) perimeter: f64,
}

/// The half-plane (upper incl. +x axis, lower) of a direction, then the cross product.
pub(crate) fn ccw(a: (i64, i64), b: (i64, i64)) -> Ordering {
    let half = |d: (i64, i64)| u8::from(d.1 < 0 || (d.1 == 0 && d.0 < 0));
    half(a).cmp(&half(b)).then_with(|| {
        let cross = i128::from(a.0) * i128::from(b.1) - i128::from(a.1) * i128::from(b.0);
        0.cmp(&cross)
    })
}

impl Arrangement {
    /// The arrangement of `rings` (any tags), snap-rounded together; also the rounded rings
    /// (tagged as given) and what rounding did.
    pub(crate) fn build(rings: &[(u32, Vec<V>)]) -> (Self, Vec<(u32, Vec<V>)>, snap::Snapped) {
        let (snapped, report) = snap::snap_round(rings);
        let mut segs: Vec<(V, V)> = snapped
            .iter()
            .flat_map(|(_, r)| {
                r.iter()
                    .zip(r.iter().cycle().skip(1))
                    .map(|(&a, &b)| if a < b { (a, b) } else { (b, a) })
            })
            .filter(|(a, b)| a != b)
            .collect();
        segs.sort_unstable();
        segs.dedup();
        let mut pts: Vec<V> = segs.iter().flat_map(|&(a, b)| [a, b]).collect();
        pts.sort_unstable();
        pts.dedup();
        let id = |p: V| -> u32 {
            u32::try_from(pts.binary_search(&p).unwrap_or_default()).unwrap_or(u32::MAX)
        };
        let mut from = Vec::with_capacity(segs.len() * 2);
        for &(a, b) in &segs {
            from.push(id(a));
            from.push(id(b));
        }
        let to = |h: u32| from.get((h ^ 1) as usize).copied().unwrap_or_default();
        let mut out: Vec<Vec<u32>> = vec![Vec::new(); pts.len()];
        for (h, &v) in from.iter().enumerate() {
            if let Some(o) = out.get_mut(v as usize) {
                o.push(u32::try_from(h).unwrap_or(u32::MAX));
            }
        }
        let dir = |h: u32| {
            let (Some(a), Some(b)) = (
                from.get(h as usize).and_then(|&v| pts.get(v as usize)),
                pts.get(to(h) as usize),
            ) else {
                return (0, 0);
            };
            (
                i64::from(b.0) - i64::from(a.0),
                i64::from(b.1) - i64::from(a.1),
            )
        };
        let mut pos = vec![0u32; from.len()];
        for o in &mut out {
            o.sort_by(|&x, &y| ccw(dir(x), dir(y)));
            for (i, &h) in o.iter().enumerate() {
                if let Some(p) = pos.get_mut(h as usize) {
                    *p = u32::try_from(i).unwrap_or(u32::MAX);
                }
            }
        }
        let mut a = Self {
            pts,
            from,
            out,
            pos,
            faces: Vec::new(),
            face_of: Vec::new(),
        };
        a.walk();
        (a, snapped, report)
    }

    fn to(&self, h: u32) -> u32 {
        self.from.get((h ^ 1) as usize).copied().unwrap_or_default()
    }

    /// The start of half-edge `h`.
    pub(crate) fn origin(&self, h: u32) -> V {
        self.from
            .get(h as usize)
            .and_then(|&v| self.pts.get(v as usize))
            .copied()
            .unwrap_or_default()
    }

    /// The half-edge after `h` among those `keep` accepts: at `h`'s end, turning clockwise
    /// from its twin.
    fn next_by(&self, h: u32, keep: impl Fn(u32) -> bool) -> Option<u32> {
        let o = self.out.get(self.to(h) as usize)?;
        let n = o.len();
        let at = *self.pos.get((h ^ 1) as usize)? as usize;
        (1..=n)
            .filter_map(|k| o.get((at + n - k) % n).copied())
            .find(|&e| keep(e))
    }

    #[allow(
        clippy::many_single_char_names,
        reason = "face f, half-edge h and e, ends a b"
    )]
    fn walk(&mut self) {
        let n = self.from.len();
        let mut face_of = vec![u32::MAX; n];
        let mut faces = Vec::new();
        for start in 0..n {
            if face_of.get(start).is_some_and(|&f| f != u32::MAX) {
                continue;
            }
            let f = u32::try_from(faces.len()).unwrap_or(u32::MAX);
            let mut edges = Vec::new();
            let mut h = u32::try_from(start).unwrap_or(u32::MAX);
            loop {
                match face_of.get_mut(h as usize) {
                    Some(slot) if *slot == u32::MAX => *slot = f,
                    _ => break,
                }
                edges.push(h);
                match self.next_by(h, |_| true) {
                    Some(e) => h = e,
                    None => break,
                }
            }
            let (mut area2, mut perimeter) = (0i128, 0.0f64);
            for &e in &edges {
                let (a, b) = (self.origin(e), self.origin(e ^ 1));
                area2 += i128::from(a.0) * i128::from(b.1) - i128::from(b.0) * i128::from(a.1);
                perimeter +=
                    (f64::from(b.0) - f64::from(a.0)).hypot(f64::from(b.1) - f64::from(a.1));
            }
            faces.push(Face {
                edges,
                area2,
                perimeter,
            });
        }
        self.faces = faces;
        self.face_of = face_of;
    }

    /// The vertices of a face, in order.
    pub(crate) fn ring(&self, f: &Face) -> Vec<V> {
        f.edges.iter().map(|&h| self.origin(h)).collect()
    }

    /// The face across half-edge `h`.
    pub(crate) fn across(&self, h: u32) -> u32 {
        self.face_of
            .get((h ^ 1) as usize)
            .copied()
            .unwrap_or(u32::MAX)
    }

    /// The boundary of a union of cycles (`edges`: all their half-edges) as rings: the
    /// half-edges whose twins are not in the set, joined end to end turning clockwise at each
    /// vertex, so a union touching itself at a vertex gives separate rings.
    pub(crate) fn rings_of(&self, edges: &[u32]) -> Vec<Vec<V>> {
        let set: HashSet<u32> = edges.iter().copied().collect();
        let keep: HashSet<u32> = set
            .iter()
            .copied()
            .filter(|h| !set.contains(&(h ^ 1)))
            .collect();
        let mut sorted: Vec<u32> = keep.iter().copied().collect();
        sorted.sort_unstable();
        let mut used: HashSet<u32> = HashSet::new();
        let mut rings = Vec::new();
        for &start in &sorted {
            if used.contains(&start) {
                continue;
            }
            let mut ring = Vec::new();
            let mut h = start;
            while used.insert(h) {
                ring.push(self.origin(h));
                match self.next_by(h, |e| keep.contains(&e)) {
                    Some(e) => h = e,
                    None => break,
                }
            }
            if ring.len() >= 3 {
                rings.push(ring);
            }
        }
        rings
    }

    /// For each clockwise cycle, the bounded face around it (the smallest whose outline holds
    /// one of its vertices strictly), `None` for the outside of everything.
    pub(crate) fn holes(&self) -> HashMap<u32, Option<u32>> {
        let comp = self.components();
        let bounded: Vec<(u32, crate::geom::Polygon, i128)> = self
            .faces
            .iter()
            .enumerate()
            .filter(|(_, f)| f.area2 > 0)
            .map(|(i, f)| {
                (
                    u32::try_from(i).unwrap_or(u32::MAX),
                    crate::geom::Polygon {
                        rings: vec![self.ring(f)],
                    },
                    f.area2,
                )
            })
            .collect();
        let mut out = HashMap::new();
        for (i, f) in self.faces.iter().enumerate() {
            if f.area2 >= 0 {
                continue;
            }
            let Some(&h) = f.edges.first() else { continue };
            let c = comp.get(self.from.get(h as usize).copied().unwrap_or_default() as usize);
            let p = self.origin(h);
            let around = bounded
                .iter()
                .filter(|(g, _, _)| {
                    let gh = self.faces.get(*g as usize).and_then(|g| g.edges.first());
                    gh.and_then(|&gh| {
                        comp.get(self.from.get(gh as usize).copied().unwrap_or_default() as usize)
                    }) != c
                })
                .filter(|(_, poly, _)| poly.contains(p.0, p.1))
                .min_by_key(|(_, _, a)| *a)
                .map(|(g, _, _)| *g);
            out.insert(u32::try_from(i).unwrap_or(u32::MAX), around);
        }
        out
    }

    /// Each vertex's connected part of the graph.
    fn components(&self) -> Vec<u32> {
        let mut comp = vec![u32::MAX; self.pts.len()];
        let mut next = 0u32;
        for s in 0..self.pts.len() {
            if comp.get(s).is_some_and(|&c| c != u32::MAX) {
                continue;
            }
            let mut stack = vec![s];
            while let Some(v) = stack.pop() {
                match comp.get_mut(v) {
                    Some(c) if *c == u32::MAX => *c = next,
                    _ => continue,
                }
                for &h in self.out.get(v).into_iter().flatten() {
                    stack.push(self.to(h) as usize);
                }
            }
            next += 1;
        }
        comp
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use super::*;

    fn sq(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<V> {
        vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    }

    #[test]
    fn two_overlapping_squares_make_three_bounded_faces() {
        let (a, _, _) = Arrangement::build(&[(0, sq(0, 0, 20, 20)), (1, sq(10, 10, 30, 30))]);
        let bounded: Vec<i128> = a.faces.iter().map(|f| f.area2).filter(|&x| x > 0).collect();
        assert_eq!(bounded.len(), 3);
        // The whole area: 400 + 400 − 100, twice.
        assert_eq!(bounded.iter().sum::<i128>(), 2 * 700);
        assert_eq!(a.faces.iter().filter(|f| f.area2 < 0).count(), 1);
    }

    #[test]
    fn a_union_of_faces_is_one_ring() {
        let (a, _, _) = Arrangement::build(&[(0, sq(0, 0, 20, 20)), (1, sq(10, 0, 30, 20))]);
        let all: Vec<u32> = a
            .faces
            .iter()
            .filter(|f| f.area2 > 0)
            .flat_map(|f| f.edges.iter().copied())
            .collect();
        let rings = a.rings_of(&all);
        assert_eq!(rings.len(), 1);
        let p = crate::geom::Polygon { rings };
        assert!(p.contains(25, 10) && p.contains(5, 10) && !p.contains(35, 10));
    }

    #[test]
    fn an_island_is_a_hole_of_the_face_around_it() {
        let (a, _, _) = Arrangement::build(&[(0, sq(0, 0, 100, 100)), (1, sq(40, 40, 60, 60))]);
        let holes = a.holes();
        // The outside of everything, and the island's outline inside the big square's face.
        assert_eq!(holes.len(), 2);
        assert_eq!(holes.values().filter(|v| v.is_none()).count(), 1);
        let around = holes.values().find_map(|v| *v).unwrap();
        assert_eq!(a.faces[around as usize].area2, 2 * 100 * 100);
    }
}
