//! Pre-simplification of a partition as shared arcs (D-155): the units of one country, each
//! a set of rings, thinned to one level of detail before the world's overlay, so a border two
//! units share stays one line.
//!
//! The rings are cleaned and cut into arcs ([`super::arcs::build`]); every arc gets its
//! Douglas–Peucker importance and the guard keeps the level free of crossings and degenerate
//! rings ([`super::simplify::levels_to`]); each unit's rings are rebuilt from the vertices kept
//! at that level. Arc ends (topology nodes) are always kept, so a junction of three units stays
//! where it is.

use super::arcs::{self, V, area2};
use super::simplify::{self, Guarded};

/// Each unit's rings.
pub(crate) type Rings = Vec<Vec<V>>;

/// What thinning did to one partition.
#[derive(Clone, Debug, Default)]
pub(crate) struct Thinned {
    /// Vertices in and out (open rings, after cleaning).
    pub(crate) vertices: (usize, usize),
    /// Shared arcs.
    pub(crate) arcs: usize,
    /// The guard at the kept level.
    pub(crate) guard: Guarded,
}

/// Each unit's rings thinned to level `lod` ([`simplify::TOLERANCE_CM`]), in input order.
///
/// # Errors
/// Overlapping units: an edge two units run the same way, or an edge of three units.
pub(crate) fn partition(units: &[&[Vec<V>]], lod: u8) -> Result<(Vec<Rings>, Thinned), String> {
    let mut rings: Vec<(u32, Vec<V>)> = Vec::new();
    for (u, r) in units.iter().enumerate() {
        let u = u32::try_from(u).map_err(|_| "too many units")?;
        rings.extend(arcs::clean_rings(r).into_iter().map(|r| (u, r)));
    }
    let before = rings.iter().map(|(_, r)| r.len()).sum();
    let (all, refs) = arcs::build(&rings)?;
    let shape: Vec<(Vec<arcs::Ref>, bool)> = refs
        .iter()
        .zip(&rings)
        .map(|(refs, (_, r))| (refs.clone(), area2(r) > 0))
        .collect();
    let (levels, report) = simplify::levels_to(&all, &shape, &[], lod);
    let kept: Vec<Vec<V>> = all
        .iter()
        .zip(&levels)
        .map(|(a, l)| {
            a.iter()
                .zip(l)
                .filter(|&(_, &x)| x >= lod)
                .map(|(&p, _)| p)
                .collect()
        })
        .collect();
    let mut out = vec![Vec::new(); units.len()];
    let mut after = 0;
    for ((u, _), refs) in rings.iter().zip(&refs) {
        let ring = arcs::ring_points(&kept, refs);
        after += ring.len();
        if let Some(o) = out.get_mut(*u as usize) {
            o.push(ring);
        }
    }
    let guard = report
        .into_iter()
        .find(|(l, _)| *l == lod)
        .map(|(_, g)| g)
        .unwrap_or_default();
    Ok((
        out,
        Thinned {
            vertices: (before, after),
            arcs: all.len(),
            guard,
        },
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "test code")]

    use super::partition;
    use crate::topo::arcs::V;

    /// Two squares sharing a wiggly border of tiny teeth (well under 100 m): thinned at level 3
    /// (100 m), the border loses its teeth and both units keep the same vertices along it.
    #[test]
    fn a_shared_border_thins_to_one_line() {
        // 0.1° squares; the border at x = 1_000_000 has 1 m teeth every 100 units.
        let teeth: Vec<V> = (0..=100)
            .map(|k| (1_000_000 + if k % 2 == 1 { 90 } else { 0 }, k * 10_000))
            .collect();
        let mut left: Vec<V> = vec![(0, 0)];
        left.extend(teeth.iter().copied());
        left.push((0, 1_000_000));
        let mut right: Vec<V> = teeth.iter().rev().copied().collect();
        right.extend([(2_000_000, 0), (2_000_000, 1_000_000)]);
        let (l, r) = (vec![left], vec![right]);
        let (out, t) = partition(&[&l, &r], 3).unwrap();
        assert!(t.vertices.1 < t.vertices.0 / 5, "{t:?}");
        assert_eq!(t.guard.left, 0);
        let on_border = |rings: &[Vec<V>]| -> Vec<V> {
            let mut v: Vec<V> = rings[0]
                .iter()
                .copied()
                .filter(|p| (999_000..=1_001_000).contains(&p.0))
                .collect();
            v.sort_unstable();
            v
        };
        assert_eq!(on_border(&out[0]), on_border(&out[1]));
        assert!(on_border(&out[0]).len() <= 3, "{:?}", on_border(&out[0]));
    }

    #[test]
    fn overlapping_units_are_refused() {
        let sq: Vec<V> = vec![(0, 0), (10, 0), (10, 10), (0, 10)];
        let a = vec![sq.clone()];
        let b = vec![sq];
        assert!(partition(&[&a, &b], 3).is_err());
    }
}
