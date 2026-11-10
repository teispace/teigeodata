//! Pre-simplification of a partition as shared arcs (D-155): the units of one country, each
//! a set of rings, thinned to one level of detail before the world's overlay, so a border two
//! units share stays one line.
//!
//! The rings are cleaned, snap rounded when not noded ([`super::snap`]) and cut into arcs ([`super::arcs::build`]); every arc gets its
//! Douglas–Peucker importance and the guard keeps the level free of crossings and degenerate
//! rings ([`super::simplify::levels_to`]); each unit's rings are rebuilt from the vertices kept
//! at that level. Arc ends (topology nodes) are always kept, so a junction of three units stays
//! where it is. A pinned unit (a pack country's outline, D-154) keeps every vertex, and a fold
//! is cut from its neighbour, never from it.

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
    /// Snap rounding of the input, when it was not noded.
    pub(crate) snapped: super::snap::Snapped,
    /// Units cut to undo a fold rounding made (see [`unfold`]).
    pub(crate) folds: usize,
}

/// The most rounds of [`unfold`]; a fold left after them stops the stage in `arcs::build`.
const UNFOLD_ROUNDS: usize = 4;

/// Undoes folds: snap rounding can turn two units that touch, or overlap by less than a pixel,
/// into two rings that run one edge the same way, and a narrow neck of one unit into a
/// zero-width bridge it runs both ways. Snapped rings are noded (every crossing is a
/// vertex), so the later unit cut by the earlier one, or a bridged unit normalised, by an
/// exact overlay adds no rounding.
/// The later unit of a pair is cut, unless only the earlier one may be (`pinned`).
/// Returns how many units were cut.
///
/// # Errors
/// Two units that overlap in a part at least [`super::pov::MIN_WIDTH`] wide: that is the
/// data, not rounding.
fn unfold(rings: &mut Vec<(u32, Vec<V>)>, pinned: &[bool]) -> Result<usize, String> {
    let is_pinned = |u: u32| pinned.get(u as usize).copied().unwrap_or(false);
    let mut cut = 0;
    for _ in 0..UNFOLD_ROUNDS {
        let pairs = arcs::same_way(rings);
        if pairs.is_empty() {
            break;
        }
        let mut late: std::collections::BTreeMap<u32, Vec<u32>> = std::collections::BTreeMap::new();
        for (a, b) in pairs {
            let (cut, by) = if is_pinned(b) && !is_pinned(a) {
                (a, b)
            } else {
                (b, a)
            };
            late.entry(cut).or_default().push(by);
        }
        let of = |u: u32, rings: &[(u32, Vec<V>)]| -> Vec<Vec<V>> {
            rings
                .iter()
                .filter(|(x, _)| *x == u)
                .map(|(_, r)| r.clone())
                .collect()
        };
        let mut replaced: Vec<(u32, Vec<Vec<V>>)> = Vec::new();
        for (&b, earlier) in &late {
            let subject = of(b, rings);
            let clips: Vec<Vec<Vec<V>>> = earlier
                .iter()
                .filter(|&&a| a != b)
                .map(|&a| of(a, rings))
                .collect();
            let clip_refs: Vec<&[Vec<V>]> = clips.iter().map(Vec::as_slice).collect();
            for (c, &a) in clip_refs.iter().zip(earlier.iter().filter(|&&a| a != b)) {
                if !super::pov::common(&subject, c, super::pov::MIN_WIDTH).is_empty() {
                    return Err(format!("units {a} and {b} overlap"));
                }
            }
            // With no other unit, this normalises the unit: its bridges go.
            let parts = super::pov::minus(&subject, &clip_refs, 0)
                .unwrap_or_else(|| super::pov::normal(&subject));
            replaced.push((b, parts.into_iter().flatten().collect()));
            cut += 1;
        }
        if replaced.is_empty() {
            break;
        }
        let gone: std::collections::BTreeSet<u32> = replaced.iter().map(|(u, _)| *u).collect();
        rings.retain(|(u, _)| !gone.contains(u));
        for (u, rs) in replaced {
            rings.extend(arcs::clean_rings(&rs).into_iter().map(|r| (u, r)));
        }
    }
    Ok(cut)
}

/// Each unit's rings thinned to level `lod` ([`simplify::TOLERANCE_CM`]), in input order; a
/// unit marked in `pinned` keeps every vertex.
///
/// # Errors
/// Overlapping units: an edge two units run the same way, or an edge of three units.
pub(crate) fn partition(
    units: &[&[Vec<V>]],
    pinned: &[bool],
    lod: u8,
) -> Result<(Vec<Rings>, Thinned), String> {
    let mut rings: Vec<(u32, Vec<V>)> = Vec::new();
    for (u, r) in units.iter().enumerate() {
        let u = u32::try_from(u).map_err(|_| "too many units")?;
        rings.extend(arcs::clean_rings(r).into_iter().map(|r| (u, r)));
    }
    let before = rings.iter().map(|(_, r)| r.len()).sum();
    // A cut leaves vertices on one side of a border only (T-junctions): snap rounding nodes
    // the input as the topology stage does (D-103); a noded input comes back unchanged.
    let (mut rings, snapped) = super::snap::snap_round(&rings);
    let folds = unfold(&mut rings, pinned)?;
    let (all, refs) = arcs::build(&rings)?;
    let mut pins = vec![false; all.len()];
    for ((u, _), refs) in rings.iter().zip(&refs) {
        if pinned.get(*u as usize).copied().unwrap_or(false) {
            for &r in refs {
                if let Some(p) = pins.get_mut((r >> 1) as usize) {
                    *p = true;
                }
            }
        }
    }
    let shape: Vec<(Vec<arcs::Ref>, bool)> = refs
        .iter()
        .zip(&rings)
        .map(|(refs, (_, r))| (refs.clone(), area2(r) > 0))
        .collect();
    let (levels, report) = simplify::levels_to(&all, &shape, &pins, lod);
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
            snapped,
            folds,
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
        let (out, t) = partition(&[&l, &r], &[], 3).unwrap();
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

    /// The same border with the right unit pinned: both keep every tooth.
    #[test]
    fn a_pinned_unit_keeps_every_vertex() {
        let teeth: Vec<V> = (0..=100)
            .map(|k| (1_000_000 + if k % 2 == 1 { 90 } else { 0 }, k * 10_000))
            .collect();
        let mut left: Vec<V> = vec![(0, 0)];
        left.extend(teeth.iter().copied());
        left.push((0, 1_000_000));
        let mut right: Vec<V> = teeth.iter().rev().copied().collect();
        right.extend([(2_000_000, 0), (2_000_000, 1_000_000)]);
        let (l, r) = (vec![left], vec![right.clone()]);
        let (out, _) = partition(&[&l, &r], &[false, true], 3).unwrap();
        let mut kept = out[1][0].clone();
        kept.sort_unstable();
        right.sort_unstable();
        assert_eq!(kept, right);
    }

    #[test]
    fn overlapping_units_are_refused() {
        // About 110 m across: an overlap that is data, not rounding.
        let sq: Vec<V> = vec![(0, 0), (10_000, 0), (10_000, 10_000), (0, 10_000)];
        let a = vec![sq.clone()];
        let b = vec![sq];
        assert!(partition(&[&a, &b], &[], 3).is_err());
    }
}
