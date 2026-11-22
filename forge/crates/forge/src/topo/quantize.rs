//! Decimal grid snap of simplified arcs (TG-16, D-229): an arc simplified to a tolerance of `t`
//! does not need its vertices to a centimetre. Snapped to the largest decimal grid no wider than
//! [`SHARE`] of `t`, its deltas divide by 10^k and ARCS stores them ÷ 10^k (the `SCALE`
//! column), so a 1 km outline costs about 10 bits less per coordinate, and `ADMIN_REGION` on
//! those arcs follows (`RUN_SCALE`).
//!
//! The snap is guarded: an arc whose snapped segments cross or touch another segment away from a
//! shared end, or that collapses (an open arc onto one point, a closed one below four points),
//! keeps its exact points, and so do the ends it shares with its neighbours, round after round
//! until no snapped arc makes a bad pair. Pairs already in the exact arcs are left as they are
//! (this changes nothing it was given). Every test is exact integer arithmetic
//! ([`super::simplify::bad_pair`]), so the result is the same on every machine.

use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::BTreeSet;

use teigeodata_core::{E7, Point};
use teigeodata_format::arcs::Arcs;
use teigeodata_format::extract::ArcRewrite;

use super::arcs::V;
use super::faces::ccw;
use super::simplify::crossings;

/// The grid's largest share of the tolerance (D-229): the world's 500 m level snaps to 1e-4°
/// (11 m, 2.2%), Nepal's 20 m level to 1e-6° (11 cm). Measured on the world part on
/// 2026-11-12: geometry 3,540,816 B → 2,913,680 B, 7 of 75,167 cities' `admin_at` answers
/// changed (each within 8 m of a border Natural Earth draws 0.6–10 km off); at 2% (1e-5°)
/// 3,207,968 B and 1 changed.
pub(crate) const SHARE: f64 = 0.05;

/// Centimetres per 1e-7° of latitude.
const CM_PER_UNIT: f64 = 1.113_2;

/// The decimal grid for a tolerance: the largest `k` (0 to 7) whose grid of 10^k units of 1e-7°
/// is at most [`SHARE`] of `tolerance_cm`. 0 means no snap.
#[must_use]
pub(crate) fn grid_k(tolerance_cm: u32) -> u32 {
    let max = f64::from(tolerance_cm) * SHARE;
    (1..=7)
        .rev()
        .find(|&k| 10_f64.powi(k) * CM_PER_UNIT <= max)
        .map_or(0, i32::unsigned_abs)
}

/// What a snap did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Quantized {
    /// Arcs on the grid.
    pub(crate) snapped: usize,
    /// Arcs kept exact by the guard.
    pub(crate) exact: usize,
    /// Rounds until no snapped arc made a bad pair.
    pub(crate) rounds: usize,
}

impl Quantized {
    /// The summary's line.
    pub(crate) fn line(&self, k: u32) -> String {
        if k == 0 {
            return "Outlines as the part stores them (no level to snap to).".into();
        }
        format!(
            "Outlines snapped to 1e-{}° (TG-16): {} arcs on the grid, {} kept exact by the guard, {} rounds.",
            7 - k.min(7),
            self.snapped,
            self.exact,
            self.rounds
        )
    }
}

/// The snap for one part of a merge: its grid from its level's tolerance, and what it did.
#[derive(Debug, Default)]
pub(crate) struct Snapper {
    /// The grid: 10^k units of 1e-7°, 0 for none.
    pub(crate) k: u32,
    pub(crate) done: Cell<(usize, usize, usize)>,
}

impl Snapper {
    /// The snap for outlines at level `lod` of `arcs` (none at full resolution).
    pub(crate) fn at(arcs: Option<&Arcs<'_>>, lod: u8) -> Self {
        let k = arcs
            .filter(|_| lod > 0)
            .and_then(|a| a.tolerance_cm(lod))
            .map_or(0, grid_k);
        Self {
            k,
            ..Self::default()
        }
    }

    /// Snaps one part's arcs (the merge's [`teigeodata_format::extract::ArcRewrite`]).
    pub(crate) fn run(&self, arcs: &mut [Vec<Point<E7>>]) {
        let q = snap(arcs, self.k);
        self.done.set((q.snapped, q.exact, q.rounds));
    }

    /// Calls `f` with this snap as a merge part's arc rewrite.
    pub(crate) fn with<R>(&self, f: impl FnOnce(&ArcRewrite<'_>) -> R) -> R {
        let run = |a: &mut [Vec<Point<E7>>]| self.run(a);
        f(&ArcRewrite { run: &run })
    }

    /// What it did.
    pub(crate) fn report(&self) -> Quantized {
        let (snapped, exact, rounds) = self.done.get();
        Quantized {
            snapped,
            exact,
            rounds,
        }
    }
}

fn round(v: i32, g: i64) -> i32 {
    let r = (i64::from(v) + g / 2).div_euclid(g) * g;
    i32::try_from(r).unwrap_or(v)
}

/// One arc on the grid, its exact ends kept where `exact_nodes` holds them; `None` if it
/// collapses or leaves the grid's range.
fn snap_one(arc: &[Point<E7>], g: i64, exact_nodes: &BTreeSet<V>) -> Option<Vec<Point<E7>>> {
    let last = arc.len().checked_sub(1)?;
    let mut out: Vec<Point<E7>> = Vec::with_capacity(arc.len());
    for (i, p) in arc.iter().enumerate() {
        let end = i == 0 || i == last;
        let q = if end && exact_nodes.contains(&(p.lon(), p.lat())) {
            *p
        } else {
            Point::from_units(round(p.lat(), g), round(p.lon(), g))?
        };
        if out.last() != Some(&q) {
            out.push(q);
        }
    }
    let closed = arc.first() == arc.last();
    let ok = if closed {
        // A closed arc is a whole ring: it keeps its turn and some area. A small triangle's
        // vertex can cross its far edge without any two segments crossing away from a shared
        // end, which the pair guard below would not see.
        out.len() >= 4 && out.first() == out.last() && signed(&out).signum() == signed(arc).signum()
    } else {
        out.len() >= 2 && out.first() != out.last()
    };
    ok.then_some(out)
}

/// The arcs at every node (an end shared by three or more arc ends) whose counter-clockwise
/// order around it differs between the exact arcs and the candidates (`None`: the arc as it
/// is), or where two candidate ends leave in one direction.
fn turned_nodes(arcs: &[Vec<Point<E7>>], cand: &[Option<Vec<Point<E7>>>]) -> Vec<usize> {
    type Dir = (i64, i64);
    let dir = |from: &Point<E7>, to: &Point<E7>| -> Dir {
        (
            i64::from(to.lon()) - i64::from(from.lon()),
            i64::from(to.lat()) - i64::from(from.lat()),
        )
    };
    // Each arc end at its exact node: (node, arc, exact direction, candidate direction).
    let mut ends: Vec<(V, usize, Dir, Dir)> = Vec::with_capacity(arcs.len() * 2);
    for (a, (arc, c)) in arcs.iter().zip(cand).enumerate() {
        let new = c.as_deref().unwrap_or(arc);
        let (Some(s), Some(s1), Some(e), Some(e1), Some(ns), Some(ns1), Some(ne), Some(ne1)) = (
            arc.first(),
            arc.get(1),
            arc.last(),
            arc.len().checked_sub(2).and_then(|i| arc.get(i)),
            new.first(),
            new.get(1),
            new.last(),
            new.len().checked_sub(2).and_then(|i| new.get(i)),
        ) else {
            continue;
        };
        ends.push(((s.lon(), s.lat()), a, dir(s, s1), dir(ns, ns1)));
        ends.push(((e.lon(), e.lat()), a, dir(e, e1), dir(ne, ne1)));
    }
    ends.sort_unstable_by_key(|e| e.0);
    let mut out = Vec::new();
    for group in ends.chunk_by(|x, y| x.0 == y.0).filter(|g| g.len() >= 3) {
        let mut exact: Vec<usize> = (0..group.len()).collect();
        exact.sort_by(|&i, &j| ccw(group[i].2, group[j].2).then(i.cmp(&j)));
        let mut new = exact.clone();
        new.sort_by(|&i, &j| ccw(group[i].3, group[j].3).then(i.cmp(&j)));
        let tie = new
            .windows(2)
            .any(|w| matches!(w, [i, j] if ccw(group[*i].3, group[*j].3) == Ordering::Equal));
        // The same cyclic order: `new` is `exact` turned to start where `exact`'s first is.
        let at = new
            .iter()
            .position(|&i| Some(&i) == exact.first())
            .unwrap_or(0);
        let same = new.iter().cycle().skip(at).take(new.len()).eq(exact.iter());
        if tie || !same {
            out.extend(group.iter().map(|e| e.1));
        }
    }
    out
}

/// Twice the signed area of a closed run of points (positive counter-clockwise), exact; zero
/// for no area.
fn signed(ring: &[Point<E7>]) -> i128 {
    ring.windows(2)
        .map(|w| match w {
            [a, b] => {
                i128::from(a.lon()) * i128::from(b.lat())
                    - i128::from(b.lon()) * i128::from(a.lat())
            }
            _ => 0,
        })
        .sum()
}

/// Snaps `arcs` to the grid of 10^k units of 1e-7°, guarded (see the [module](self)).
pub(crate) fn snap(arcs: &mut [Vec<Point<E7>>], k: u32) -> Quantized {
    if k == 0 || arcs.is_empty() {
        return Quantized {
            exact: arcs.len(),
            ..Quantized::default()
        };
    }
    let g = 10_i64.pow(k.min(7));
    let node = |p: &Point<E7>| (p.lon(), p.lat());
    let mut exact = vec![false; arcs.len()];
    let mut exact_nodes: BTreeSet<V> = BTreeSet::new();
    let mut rounds = 0;
    loop {
        rounds += 1;
        let mut changed = false;
        let mut cand: Vec<Option<Vec<Point<E7>>>> = Vec::with_capacity(arcs.len());
        for (a, arc) in arcs.iter().enumerate() {
            if exact[a] {
                cand.push(None);
                continue;
            }
            let s = snap_one(arc, g, &exact_nodes);
            if s.is_none() {
                exact[a] = true;
                changed = true;
                exact_nodes.extend(arc.first().map(node));
                exact_nodes.extend(arc.last().map(node));
            }
            cand.push(s);
        }
        if changed {
            continue;
        }
        let mut segs: Vec<(V, V)> = Vec::new();
        let mut seg_arc: Vec<usize> = Vec::new();
        for (a, (arc, c)) in arcs.iter().zip(&cand).enumerate() {
            let pts = c.as_deref().unwrap_or(arc);
            for w in pts.windows(2) {
                if let [p, q] = w {
                    segs.push((node(p), node(q)));
                    seg_arc.push(a);
                }
            }
        }
        for (i, j) in crossings(&segs) {
            for a in [seg_arc[i], seg_arc[j]] {
                if !exact[a] {
                    exact[a] = true;
                    changed = true;
                    exact_nodes.extend(arcs[a].first().map(node));
                    exact_nodes.extend(arcs[a].last().map(node));
                }
            }
        }
        // Around every node the arcs keep their counter-clockwise order: with no crossings
        // and every closed arc keeping its turn, each ring then keeps its orientation. A thin
        // lens between two arcs can turn over with no two segments crossing away from a
        // shared end.
        for a in turned_nodes(arcs, &cand) {
            if !exact[a] {
                exact[a] = true;
                changed = true;
                exact_nodes.extend(arcs[a].first().map(node));
                exact_nodes.extend(arcs[a].last().map(node));
            }
        }
        if !changed {
            let mut q = Quantized {
                rounds,
                ..Quantized::default()
            };
            for (arc, c) in arcs.iter_mut().zip(cand) {
                match c {
                    Some(s) => {
                        *arc = s;
                        q.snapped += 1;
                    }
                    None => q.exact += 1,
                }
            }
            return q;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests {
    use super::{grid_k, snap};
    use teigeodata_core::{E7, Point};

    fn p(lat: i32, lon: i32) -> Point<E7> {
        Point::from_units(lat, lon).unwrap()
    }

    #[test]
    fn the_grid_is_a_small_share_of_the_tolerance() {
        assert_eq!(grid_k(50_000), 3, "500 m: 1e-4°, 11 m");
        assert_eq!(grid_k(100_000), 3, "1 km: 1e-4°, 11 m");
        assert_eq!(grid_k(2_000), 1, "20 m: 1e-6°, 11 cm");
        assert_eq!(grid_k(50), 0, "50 cm: no snap");
    }

    #[test]
    fn shared_ends_stay_shared_and_arcs_land_on_the_grid() {
        let mut arcs = vec![
            vec![p(1_234, 5_678), p(20_444, 30_555), p(40_001, 60_002)],
            vec![p(40_001, 60_002), p(70_123, 10_987)],
        ];
        let q = snap(&mut arcs, 3);
        assert_eq!((q.snapped, q.exact), (2, 0));
        assert_eq!(arcs[0].last(), arcs[1].first());
        assert!(
            arcs.iter()
                .flatten()
                .all(|v| v.lat() % 1_000 == 0 && v.lon() % 1_000 == 0)
        );
    }

    #[test]
    fn an_arc_that_would_cross_keeps_its_exact_points() {
        // Apart exactly; on a 1,000-unit grid a's middle lands inside b's first segment, so both stay exact.
        let a = vec![p(0, 0), p(1_600, 5_000), p(0, 10_000)];
        let b = vec![p(2_000, 0), p(1_700, 5_600), p(2_000, 10_000)];
        let mut arcs = vec![a.clone(), b.clone()];
        let q = snap(&mut arcs, 3);
        assert_eq!(q.exact, 2);
        assert_eq!(arcs, [a, b]);
    }

    #[test]
    fn a_ring_that_would_turn_over_keeps_its_exact_points() {
        // Counter-clockwise exactly; on a 1,000-unit grid the third vertex rounds past the
        // first edge and the triangle runs clockwise, with no two segments crossing.
        let ring = vec![p(1_135, 852), p(-569, -112), p(-244, 99), p(1_135, 852)];
        let mut arcs = vec![ring.clone()];
        assert_eq!(snap(&mut arcs, 3).exact, 1);
        assert_eq!(arcs[0], ring);
    }

    #[test]
    fn a_lens_that_would_turn_over_keeps_its_exact_points() {
        // Two arcs from A to B bound a thin lens; A and B each meet a third arc. On a
        // 1,000-unit grid C rounds below the line A–B, the lens runs clockwise, and no two
        // segments cross away from a shared end: only the order of the arcs around A shows it.
        let (a, b, c) = (p(0, 0), p(900, 2_000), p(480, 1_000));
        let straight = vec![a, b];
        let bent = vec![a, c, b];
        let mut arcs = vec![
            straight.clone(),
            bent.clone(),
            vec![a, p(0, -2_000)],
            vec![b, p(900, 4_000)],
        ];
        let q = snap(&mut arcs, 3);
        assert!(q.exact >= 2, "{q:?}");
        assert_eq!(arcs[0], straight);
        assert_eq!(arcs[1], bent);
    }

    #[test]
    fn a_collapsing_ring_keeps_its_exact_points() {
        let ring = vec![p(0, 0), p(100, 0), p(100, 100), p(0, 0)];
        let mut arcs = vec![ring.clone()];
        assert_eq!(snap(&mut arcs, 3).exact, 1);
        assert_eq!(arcs[0], ring);
    }
}
