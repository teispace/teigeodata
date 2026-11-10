//! Shared arcs from a partition's rings, and polygons rebuilt from arc references (D-088 §3.4).
//!
//! - **Node rule:** a vertex is a node where its degree in the edge graph is not 2, or where the
//!   set of units on the two edges through it changes (REGION's arc rule, D-083). A ring with
//!   no node is one closed arc, anchored at its smallest vertex so that both sides find the same
//!   arc.
//! - **Rings to polygons:** a set of directed arc references, with each arc used once in each
//!   direction cancelled (an inner border of a dissolve), is chained into rings. Where a ring
//!   touches itself, the chain takes the sharpest left turn, so every ring stays simple. Rings of
//!   positive area are outer rings; each hole goes to the smallest outer ring holding it.
//!
//! Everything is exact integer arithmetic on the 1e-7° grid, so the result is the same on every
//! machine.

#![allow(
    clippy::indexing_slicing,
    reason = "indices come from the same vectors (ring positions, arc vertices, segment ids): an out-of-range index is a forge bug and must stop the build, not be skipped"
)]

use std::collections::{BTreeMap, HashMap};

/// A vertex: (longitude, latitude) on the 1e-7° grid.
pub(crate) type V = (i32, i32);

/// An arc reference: `2 · arc + reversed`.
pub(crate) type Ref = u32;

/// A polygon as parts, each a list of rings (the outer ring first), each a list of references.
pub(crate) type Parts = Vec<Vec<Vec<Ref>>>;

/// Arcs, and each input ring's references.
pub(crate) type Built = (Vec<Vec<V>>, Vec<Vec<Ref>>);

/// Twice the signed area of an open ring (positive counter-clockwise), exact.
pub(crate) fn area2(ring: &[V]) -> i128 {
    let Some(&o) = ring.first() else { return 0 };
    let rel = |p: V| {
        (
            i128::from(p.0) - i128::from(o.0),
            i128::from(p.1) - i128::from(o.1),
        )
    };
    let n = ring.len();
    (0..n)
        .map(|i| {
            let (a, b) = (rel(ring[i]), rel(ring[(i + 1) % n]));
            a.0 * b.1 - b.0 * a.1
        })
        .sum()
}

/// The orientation of `c` against the line `a`→`b`: 1 left, −1 right, 0 on it.
pub(crate) fn orient(a: V, b: V, c: V) -> i32 {
    let v = (i128::from(b.0) - i128::from(a.0)) * (i128::from(c.1) - i128::from(a.1))
        - (i128::from(b.1) - i128::from(a.1)) * (i128::from(c.0) - i128::from(a.0));
    v.signum() as i32
}

/// Where `p` lies against an open ring: `Some(true)` inside, `Some(false)` outside, `None` on
/// its border.
pub(crate) fn ring_side(ring: &[V], p: V) -> Option<bool> {
    let n = ring.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if orient(a, b, p) == 0
            && p.0 >= a.0.min(b.0)
            && p.0 <= a.0.max(b.0)
            && p.1 >= a.1.min(b.1)
            && p.1 <= a.1.max(b.1)
        {
            return None;
        }
        if (a.1 > p.1) != (b.1 > p.1) {
            let lhs = (i128::from(p.0) - i128::from(a.0)) * (i128::from(b.1) - i128::from(a.1));
            let rhs = (i128::from(b.0) - i128::from(a.0)) * (i128::from(p.1) - i128::from(a.1));
            if if b.1 > a.1 { lhs < rhs } else { lhs > rhs } {
                inside = !inside;
            }
        }
    }
    Some(inside)
}

/// A unit's rings cleaned for topology: consecutive repeats and the closing vertex dropped,
/// rings of fewer than 3 vertices or no area dropped, outer rings counter-clockwise and holes
/// (inside an odd number of the unit's other rings) clockwise.
pub(crate) fn clean_rings(rings: &[Vec<V>]) -> Vec<Vec<V>> {
    let mut out: Vec<Vec<V>> = rings
        .iter()
        .filter_map(|r| {
            let mut v = r.clone();
            v.dedup();
            if v.len() > 1 && v.first() == v.last() {
                v.pop();
            }
            (v.len() >= 3 && area2(&v) != 0).then_some(v)
        })
        .collect();
    let holes: Vec<bool> = (0..out.len())
        .map(|i| {
            let depth = (0..out.len())
                .filter(|&j| j != i)
                .filter(|&j| out[i].iter().find_map(|&p| ring_side(&out[j], p)) == Some(true))
                .count();
            depth % 2 == 1
        })
        .collect();
    for (r, hole) in out.iter_mut().zip(holes) {
        if (area2(r) > 0) == hole {
            r.reverse();
        }
    }
    out
}

/// The undirected key of an edge.
fn key(a: V, b: V) -> (V, V) {
    if a < b { (a, b) } else { (b, a) }
}

/// Arcs and each ring's references, from the rings of a partition: `rings[i] = (unit, ring)`.
///
/// Memory is flat records, sorted, not hash maps (D-155): about 24 B per ring vertex, freed
/// before the arcs are cut, so the world's 36 M vertices fit where maps took about 7 GB.
///
/// # Errors
/// An edge used by more than two rings, or twice in one direction (overlapping units).
#[allow(
    clippy::many_single_char_names,
    reason = "ring r of n vertices, edge a, b, index i"
)]
pub(crate) fn build(rings: &[(u32, Vec<V>)]) -> Result<Built, String> {
    check_edges(rings)?;
    let nodes = nodes(rings);
    let is_node = |v: &V| nodes.binary_search(v).is_ok();
    let mut arcs: Vec<Vec<V>> = Vec::new();
    // The directed first edge of every arc, and of every arc walked backwards.
    let mut start: HashMap<(V, V), Ref> = HashMap::new();
    let mut refs = Vec::with_capacity(rings.len());
    for (_, r) in rings {
        let n = r.len();
        let at: Vec<usize> = (0..n).filter(|&i| is_node(&r[i])).collect();
        let chains: Vec<Vec<V>> = if at.is_empty() {
            // One closed arc, from the smallest vertex (both sides find the same one).
            let m = (0..n).min_by_key(|&i| r[i]).unwrap_or(0);
            vec![(0..=n).map(|k| r[(m + k) % n]).collect()]
        } else {
            at.iter()
                .enumerate()
                .map(|(k, &s)| {
                    let e = at[(k + 1) % at.len()];
                    let len = (e + n - s) % n;
                    let len = if len == 0 { n } else { len };
                    (0..=len).map(|j| r[(s + j) % n]).collect()
                })
                .collect()
        };
        let mut ring_refs = Vec::with_capacity(chains.len());
        for c in chains {
            ring_refs.push(arc_ref(&mut arcs, &mut start, c, at.is_empty(), r));
        }
        refs.push(ring_refs);
    }
    Ok((arcs, refs))
}

/// Every edge borders at most two rings, and no two rings run it the same way.
fn check_edges(rings: &[(u32, Vec<V>)]) -> Result<(), String> {
    use rayon::slice::ParallelSliceMut;
    // (undirected edge, runs from its larger end, unit)
    let mut edges: Vec<((V, V), bool, u32)> =
        Vec::with_capacity(rings.iter().map(|(_, r)| r.len()).sum());
    for (unit, r) in rings {
        let n = r.len();
        for i in 0..n {
            let (a, b) = (r[i], r[(i + 1) % n]);
            edges.push((key(a, b), a > b, *unit));
        }
    }
    edges.par_sort_unstable();
    for g in edges.chunk_by(|x, y| x.0 == y.0) {
        let ((a, b), _, _) = g[0];
        if g.len() > 2 {
            let units: Vec<u32> = g.iter().map(|x| x.2).collect();
            return Err(format!(
                "the edge {a:?}–{b:?} borders more than two units (rings of units {units:?})"
            ));
        }
        if let [x, y] = g
            && x.1 == y.1
        {
            let (a, b) = if x.1 { (b, a) } else { (a, b) };
            return Err(format!(
                "units {} and {} both run {a:?}→{b:?}: they overlap",
                x.2, y.2
            ));
        }
    }
    Ok(())
}

/// The pairs of units (smaller first, sorted, each once) that run an edge the same way: they
/// overlap there. After snap rounding these are folds, sub-pixel overlaps that rounding turned
/// into a shared edge (D-155). A unit that runs an edge twice itself (a zero-width bridge)
/// comes as the pair `(u, u)`.
pub(crate) fn same_way(rings: &[(u32, Vec<V>)]) -> Vec<(u32, u32)> {
    use rayon::slice::ParallelSliceMut;
    let mut edges: Vec<((V, V), bool, u32)> =
        Vec::with_capacity(rings.iter().map(|(_, r)| r.len()).sum());
    for (unit, r) in rings {
        let n = r.len();
        for i in 0..n {
            let (a, b) = (r[i], r[(i + 1) % n]);
            edges.push((key(a, b), a > b, *unit));
        }
    }
    edges.par_sort_unstable();
    let mut out: Vec<(u32, u32)> = edges
        .chunk_by(|x, y| x.0 == y.0 && x.1 == y.1)
        .filter(|g| g.len() > 1)
        .flat_map(|g| {
            g.iter()
                .flat_map(move |x| g.iter().map(move |y| (x.2, y.2)))
                .filter(|(a, b)| a < b)
        })
        .collect();
    // A unit on both sides of an edge.
    out.extend(
        edges
            .chunk_by(|x, y| x.0 == y.0)
            .filter(|g| g.len() > 1)
            .flat_map(|g| {
                g.iter().enumerate().flat_map(move |(i, x)| {
                    g.iter()
                        .skip(i + 1)
                        .filter(move |y| y.2 == x.2)
                        .map(move |_| (x.2, x.2))
                })
            }),
    );
    out.sort_unstable();
    out.dedup();
    out
}

/// The topology nodes, sorted: a vertex met three or more times, or twice with different
/// neighbours, or once between two copies of one neighbour (a spike). On a partition whose
/// edges pass [`check_edges`], this is the rule of the module docs: a vertex met once has two
/// neighbours and one owner set; met twice with the same neighbours, two neighbours and both
/// edges owned by the same two rings; anything else has other than two neighbours or owners
/// that change.
fn nodes(rings: &[(u32, Vec<V>)]) -> Vec<V> {
    use rayon::slice::ParallelSliceMut;
    // (vertex, smaller neighbour, larger neighbour)
    let mut seen: Vec<(V, V, V)> = Vec::with_capacity(rings.iter().map(|(_, r)| r.len()).sum());
    for (_, r) in rings {
        let n = r.len();
        for i in 0..n {
            let (p, q) = (r[(i + n - 1) % n], r[(i + 1) % n]);
            seen.push((r[i], p.min(q), p.max(q)));
        }
    }
    seen.par_sort_unstable();
    seen.chunk_by(|x, y| x.0 == y.0)
        .filter(|g| match g {
            [one] => one.1 == one.2,
            [x, y] => (x.1, x.2) != (y.1, y.2) || x.1 == x.2,
            _ => true,
        })
        .map(|g| g[0].0)
        .collect()
}

/// The reference to the arc of chain `c`, adding the arc if it is new. A closed arc is stored
/// counter-clockwise from its smallest vertex; a ring walking it the other way references it
/// reversed.
fn arc_ref(
    arcs: &mut Vec<Vec<V>>,
    start: &mut HashMap<(V, V), Ref>,
    c: Vec<V>,
    closed: bool,
    ring: &[V],
) -> Ref {
    let (first, second) = (c[0], c[1]);
    if let Some(&r) = start.get(&(first, second)) {
        return r;
    }
    let id = u32::try_from(arcs.len()).unwrap_or(u32::MAX);
    let forward = !closed || area2(ring) > 0;
    let mut stored = c;
    if !forward {
        stored.reverse();
    }
    let n = stored.len();
    start.insert((stored[0], stored[1]), id << 1);
    start.insert((stored[n - 1], stored[n - 2]), id << 1 | 1);
    arcs.push(stored);
    if forward { id << 1 } else { id << 1 | 1 }
}

/// The vertices of a ring of references, open (no closing vertex).
pub(crate) fn ring_points(arcs: &[Vec<V>], refs: &[Ref]) -> Vec<V> {
    let mut out = Vec::new();
    for &r in refs {
        let a = &arcs[(r >> 1) as usize];
        let skip = usize::from(!out.is_empty());
        if r & 1 == 0 {
            out.extend(a.iter().skip(skip));
        } else {
            out.extend(a.iter().rev().skip(skip));
        }
    }
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

/// The first and last vertex of a reference.
fn ends(arcs: &[Vec<V>], r: Ref) -> (V, V) {
    let a = &arcs[(r >> 1) as usize];
    let (s, e) = (a[0], a[a.len() - 1]);
    if r & 1 == 0 { (s, e) } else { (e, s) }
}

/// The direction leaving the start of a reference, and the one arriving at its end.
fn dirs(arcs: &[Vec<V>], r: Ref) -> (V, V) {
    let a = &arcs[(r >> 1) as usize];
    let n = a.len();
    let d = |p: V, q: V| (q.0 - p.0, q.1 - p.1);
    if r & 1 == 0 {
        (d(a[0], a[1]), d(a[n - 2], a[n - 1]))
    } else {
        (d(a[n - 1], a[n - 2]), d(a[1], a[0]))
    }
}

/// Orders turns from `inc`: the larger, the more to the left (a U-turn last).
fn turn(inc: V, out: V) -> (u8, i128, i128) {
    let cross = i128::from(inc.0) * i128::from(out.1) - i128::from(inc.1) * i128::from(out.0);
    let dot = i128::from(inc.0) * i128::from(out.0) + i128::from(inc.1) * i128::from(out.1);
    // Half 1: strictly left, or straight back; half 0: straight on or right.
    let half = u8::from(cross > 0 || (cross == 0 && dot < 0));
    (half, cross, dot)
}

/// Whether `a` turns further left than `b` from the same incoming direction.
fn lefter(inc: V, a: V, b: V) -> bool {
    let (ha, ..) = turn(inc, a);
    let (hb, ..) = turn(inc, b);
    if ha != hb {
        return ha > hb;
    }
    // Same half: `a` is counter-clockwise of `b`.
    i128::from(b.0) * i128::from(a.1) - i128::from(b.1) * i128::from(a.0) > 0
}

/// A polygon's parts from directed references (inner borders cancel).
///
/// # Errors
/// References that do not close into rings.
pub(crate) fn polygon(
    arcs: &[Vec<V>],
    refs: impl IntoIterator<Item = Ref>,
) -> Result<Parts, String> {
    let mut count: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
    for r in refs {
        let e = count.entry(r >> 1).or_default();
        if r & 1 == 0 { e.0 += 1 } else { e.1 += 1 }
    }
    let mut left: BTreeMap<V, Vec<Ref>> = BTreeMap::new();
    for (&a, &(f, b)) in &count {
        let r = match f.cmp(&b) {
            std::cmp::Ordering::Greater => a << 1,
            std::cmp::Ordering::Less => a << 1 | 1,
            std::cmp::Ordering::Equal => continue,
        };
        if f.max(b) - f.min(b) != 1 {
            return Err(format!(
                "arc {a} is used {f} times forwards and {b} backwards"
            ));
        }
        left.entry(ends(arcs, r).0).or_default().push(r);
    }
    let mut rings: Vec<Vec<Ref>> = Vec::new();
    while let Some((&from, _)) = left.iter().find(|(_, v)| !v.is_empty()) {
        let mut ring = Vec::new();
        let mut at = from;
        let mut inc: Option<V> = None;
        loop {
            let outs = left.get_mut(&at).ok_or("a ring does not close")?;
            if outs.is_empty() {
                return Err(format!("a ring does not close at {at:?}"));
            }
            let pick = match inc {
                Some(d) => (0..outs.len())
                    .reduce(|best, i| {
                        if lefter(d, dirs(arcs, outs[i]).0, dirs(arcs, outs[best]).0) {
                            i
                        } else {
                            best
                        }
                    })
                    .unwrap_or(0),
                None => 0,
            };
            let r = outs.swap_remove(pick);
            ring.push(r);
            inc = Some(dirs(arcs, r).1);
            at = ends(arcs, r).1;
            if at == from {
                break;
            }
        }
        rings.push(ring);
    }
    // Outer rings and holes.
    let pts: Vec<Vec<V>> = rings.iter().map(|r| ring_points(arcs, r)).collect();
    let areas: Vec<i128> = pts.iter().map(|p| area2(p)).collect();
    let mut parts: Vec<(usize, Vec<usize>)> = (0..rings.len())
        .filter(|&i| areas[i] > 0)
        .map(|i| (i, Vec::new()))
        .collect();
    for h in (0..rings.len()).filter(|&i| areas[i] < 0) {
        let host = parts
            .iter()
            .enumerate()
            .filter(|(_, (o, _))| pts[h].iter().find_map(|&p| ring_side(&pts[*o], p)) == Some(true))
            .min_by_key(|(_, (o, _))| areas[*o])
            .map(|(k, _)| k)
            .ok_or_else(|| "a hole outside every outer ring".to_string())?;
        parts[host].1.push(h);
    }
    // Largest part first, then by first reference, for a stable order.
    parts.sort_by_key(|(o, _)| (std::cmp::Reverse(areas[*o]), rings[*o].clone()));
    Ok(parts
        .into_iter()
        .map(|(o, holes)| {
            std::iter::once(o)
                .chain(holes)
                .map(|i| rings[i].clone())
                .collect()
        })
        .collect())
}

#[cfg(test)]
/// The hash-map form of [`build`] that it replaced (D-155): the differential test's oracle.
///
/// # Errors
/// An edge used by more than two rings, or twice in one direction (overlapping units).
#[allow(
    clippy::many_single_char_names,
    reason = "ring r of n vertices, edge a, b, index i"
)]
fn build_reference(rings: &[(u32, Vec<V>)]) -> Result<Built, String> {
    // Units on each edge, and each vertex's neighbours.
    let mut owners: HashMap<(V, V), Vec<u32>> = HashMap::new();
    let mut directed: HashMap<(V, V), u32> = HashMap::new();
    let mut nbrs: HashMap<V, Vec<V>> = HashMap::new();
    for (unit, r) in rings {
        let n = r.len();
        for i in 0..n {
            let (a, b) = (r[i], r[(i + 1) % n]);
            let o = owners.entry(key(a, b)).or_default();
            o.push(*unit);
            o.sort_unstable();
            if o.len() > 2 {
                return Err(format!("the edge {a:?}–{b:?} borders more than two units"));
            }
            if let Some(u) = directed.insert((a, b), *unit) {
                return Err(format!(
                    "units {u} and {unit} both run {a:?}→{b:?}: they overlap"
                ));
            }
            for (p, q) in [(a, b), (b, a)] {
                let v = nbrs.entry(p).or_default();
                if !v.contains(&q) {
                    v.push(q);
                }
            }
        }
    }
    let is_node = |prev: V, v: V, next: V| {
        nbrs.get(&v).map_or(0, Vec::len) != 2
            || owners.get(&key(prev, v)) != owners.get(&key(v, next))
    };
    let mut arcs: Vec<Vec<V>> = Vec::new();
    // The directed first edge of every arc, and of every arc walked backwards.
    let mut start: HashMap<(V, V), Ref> = HashMap::new();
    let mut refs = Vec::with_capacity(rings.len());
    for (_, r) in rings {
        let n = r.len();
        let nodes: Vec<usize> = (0..n)
            .filter(|&i| is_node(r[(i + n - 1) % n], r[i], r[(i + 1) % n]))
            .collect();
        let chains: Vec<Vec<V>> = if nodes.is_empty() {
            // One closed arc, from the smallest vertex (both sides find the same one).
            let m = (0..n).min_by_key(|&i| r[i]).unwrap_or(0);
            vec![(0..=n).map(|k| r[(m + k) % n]).collect()]
        } else {
            nodes
                .iter()
                .enumerate()
                .map(|(k, &s)| {
                    let e = nodes[(k + 1) % nodes.len()];
                    let len = (e + n - s) % n;
                    let len = if len == 0 { n } else { len };
                    (0..=len).map(|j| r[(s + j) % n]).collect()
                })
                .collect()
        };
        let mut ring_refs = Vec::with_capacity(chains.len());
        for c in chains {
            ring_refs.push(arc_ref(&mut arcs, &mut start, c, nodes.is_empty(), r));
        }
        refs.push(ring_refs);
    }
    Ok((arcs, refs))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::many_single_char_names,
        reason = "test code: grid sizes and corners"
    )]

    use super::{V, build, build_reference};

    /// A small deterministic generator (an LCG): the cases are the same on every run.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self, n: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) % n
        }
    }

    /// The points strictly inside the grid edge `a`–`b`: the same for both cells that share it,
    /// whichever way they walk it.
    fn inner(a: V, b: V, seed: u64) -> Vec<V> {
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        let h = seed
            ^ (u64::from(lo.0.unsigned_abs()) * 31 + u64::from(lo.1.unsigned_abs()) * 17)
            ^ (u64::from(hi.0.unsigned_abs()) * 7 + u64::from(hi.1.unsigned_abs()));
        let k = i32::try_from(h % 4).unwrap();
        let mut v: Vec<V> = (1..=k)
            .map(|j| {
                let t = |x: i32, y: i32| x + (y - x) * j / (k + 1);
                // A nudge off the line, so the points are real vertices.
                let wob = i32::try_from((h >> j) % 3).unwrap() - 1;
                (t(lo.0, hi.0) + wob, t(lo.1, hi.1))
            })
            .collect();
        if a > b {
            v.reverse();
        }
        v
    }

    /// A grid of cells, some missing, each a ring with its edges subdivided; units drawn from a
    /// few IDs, so some units have several rings.
    fn grid(seed: u64) -> Vec<(u32, Vec<V>)> {
        let mut g = Lcg(seed);
        let (w, h) = (1 + g.next(5), 1 + g.next(5));
        let s = 1_000;
        let mut out = Vec::new();
        for i in 0..w {
            for j in 0..h {
                if g.next(5) == 0 {
                    continue;
                }
                let (x, y) = (i32::try_from(i).unwrap() * s, i32::try_from(j).unwrap() * s);
                let corners = [(x, y), (x + s, y), (x + s, y + s), (x, y + s)];
                let mut ring = Vec::new();
                for c in 0..4 {
                    let (a, b) = (corners[c], corners[(c + 1) % 4]);
                    ring.push(a);
                    ring.extend(inner(a, b, seed));
                }
                out.push((u32::try_from(g.next(4)).unwrap(), ring));
            }
        }
        out
    }

    #[test]
    fn the_sorted_build_matches_the_hash_map_build() {
        let mut nodes = 0;
        for seed in 0..300 {
            let rings = grid(seed);
            let (want, got) = (build_reference(&rings), build(&rings));
            assert_eq!(want, got, "seed {seed}");
            nodes += got.unwrap().0.len();
        }
        assert!(nodes > 1_000, "the cases have arcs: {nodes}");
    }

    #[test]
    fn overlaps_fail_as_before() {
        for seed in 0..50 {
            let mut rings = grid(seed);
            let Some(first) = rings.first().cloned() else {
                continue;
            };
            // The same ring twice: an overlap.
            rings.push((99, first.1.clone()));
            assert!(
                build_reference(&rings).is_err() && build(&rings).is_err(),
                "seed {seed}"
            );
            // A third ring on an edge.
            let mut rev = first.1;
            rev.reverse();
            rings.pop();
            rings.push((98, rev.clone()));
            rings.push((97, rev));
            assert!(
                build_reference(&rings).is_err() && build(&rings).is_err(),
                "seed {seed}"
            );
        }
    }
}
