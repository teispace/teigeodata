//! The REGION writer (feature `write`): shared arcs from the region rings, then the quadtree.
#![allow(
    clippy::indexing_slicing,
    clippy::many_single_char_names,
    reason = "builder side; every index is bounded by construction: vertex IDs below the vertex \
              count, edge IDs below the edge count, CSR offsets from those counts; geometry \
              reads as a, b, r, c"
)]

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use teigeodata_core::bitpack::{bias, encode_direct};
use teigeodata_core::table;

use super::{
    Cell, DIR, KEY, LEVEL, LEVELS, MAX_SET, MAX_TOP, MIN_TOP, NUDGE, NUDGES, RUN_EDGE, RUN_LEN,
    RUN_SCALE, RUN_X, RUN_Y, RUN_ZONES, RUNS, SET_START, SET_ZONES, TOP, TOP_ZONES, X, X_MIN,
    X_SPAN, Y, Y_MIN, Y_SPAN, ZONES, cell, crosses, orient, reference, touches,
};
use crate::Error;

type Pt = (i32, i32);

/// The most edges a cell keeps unsplit when splitting makes no progress (see `Tree::node`).
const STUCK: usize = 16;

/// How [`encode`] shapes the index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// A cell with more edges than this is split, down to [`LEVELS`].
    pub leaf_max: usize,
    /// The top-grid level, [`MIN_TOP`] to [`MAX_TOP`].
    pub top: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            leaf_max: 32,
            top: 10,
        }
    }
}

/// Encodes a REGION payload. `regions[id]` is the rings of region `id` (outer rings and holes
/// alike: membership is even–odd), each a list of `[x, y]` = `[lon, lat]` on the 1e-7° grid,
/// closed or not. IDs are the precedence order: put the region that wins an overlap first. The
/// payload's item count is `regions.len()`.
///
/// Deterministic: the same input gives the same bytes. Memory is about 60 bytes per input edge.
///
/// # Errors
/// [`Error::Write`] if a vertex is outside the domain, the options are out of range, or a leaf
/// would need more than [`MAX_SET`] regions or no reference point can be placed.
pub fn encode(regions: &[Vec<Vec<[i32; 2]>>], opts: &Options) -> Result<Vec<u8>, Error> {
    let mut sets = start(regions.len(), opts)?;
    let edges = border_edges(regions, &mut sets)?;
    let arcs = Arcs::build(&edges);
    drop(edges);
    finish(&arcs, sets, opts, None)
}

/// Encodes a REGION payload that reads its vertices from an ARCS section: `arcs` are that
/// section's arcs in its order (each `[x, y]` on the 1e-7° grid), `scales` their decimal scales
/// (empty when the section has none), and `regions[id]` the arcs region `id`'s rings use. An
/// arc borders the regions that use it an odd number of times. Runs then number ARCS's
/// vertices, and each run stores its first vertex; the reader opens the payload with
/// [`Region::open_on_arcs`](super::Region::open_on_arcs). The item count is `regions.len()`.
///
/// # Errors
/// As [`encode`], and if an arc has fewer than 2 vertices or a repeated vertex in a row, an
/// arc reference is past the arcs, or `scales` is neither empty nor one per arc.
pub fn encode_on_arcs(
    arcs: &[Vec<[i32; 2]>],
    scales: &[u32],
    regions: &[Vec<u32>],
    opts: &Options,
) -> Result<Vec<u8>, Error> {
    let mut sets = start(regions.len(), opts)?;
    if !scales.is_empty() && scales.len() != arcs.len() {
        return Err(Error::Write("one scale per arc, or none"));
    }
    let mut uses: Vec<Vec<u32>> = vec![Vec::new(); arcs.len()];
    for (z, list) in (0_u32..).zip(regions) {
        for &a in list {
            uses.get_mut(a as usize)
                .ok_or(Error::Write("an arc reference past the arcs"))?
                .push(z);
        }
    }
    let n: usize = arcs.iter().map(Vec::len).sum();
    let mut shared = Arcs {
        x: Vec::with_capacity(n),
        y: Vec::with_capacity(n),
        set: Vec::with_capacity(n),
    };
    let mut starts = Vec::with_capacity(arcs.len());
    for (pts, users) in arcs.iter().zip(&mut uses) {
        if pts.len() < 2 || pts.windows(2).any(|w| w[0] == w[1]) {
            return Err(Error::Write(
                "an arc has fewer than 2 vertices or a repeated vertex",
            ));
        }
        if !pts.iter().all(|p| in_domain(*p)) {
            return Err(Error::Write("a region vertex is outside the domain"));
        }
        // Sorted, an odd multiplicity keeps a region and an even one cancels.
        users.sort_unstable();
        let mut odd: Vec<u32> = Vec::new();
        for &z in users.iter() {
            if odd.last() == Some(&z) {
                odd.pop();
            } else {
                odd.push(z);
            }
        }
        let s = if odd.is_empty() {
            u32::MAX
        } else {
            sets.id(&odd)?
        };
        starts.push(shared.x.len());
        for (i, p) in pts.iter().enumerate() {
            shared.x.push(p[0]);
            shared.y.push(p[1]);
            shared
                .set
                .push(if i + 1 < pts.len() { s } else { u32::MAX });
        }
    }
    finish(&shared, sets, opts, Some((scales, &starts)))
}

/// Checks the options and the region count; the sets with set `z` = the single region `z`.
fn start(regions: usize, opts: &Options) -> Result<Sets, Error> {
    if !(MIN_TOP..=MAX_TOP).contains(&opts.top) || opts.leaf_max == 0 {
        return Err(Error::Write("the region options are out of range"));
    }
    let count = u32::try_from(regions).map_err(|_| Error::Write("too many regions"))?;
    let mut sets = Sets::default();
    // Set z is the single region z (the reader skips the set tables for those).
    for z in 0..count {
        sets.id(&[z])?;
    }
    Ok(sets)
}

/// The quadtree over `arcs` and the payload; `on_arcs` is the scales and first vertex of each
/// arc when the vertices live in ARCS.
fn finish(
    arcs: &Arcs,
    sets: Sets,
    opts: &Options,
    on_arcs: Option<(&[u32], &[usize])>,
) -> Result<Vec<u8>, Error> {
    let mut tree = Tree {
        arcs,
        sets,
        opts: *opts,
        leaves: Vec::new(),
        run_edge: Vec::new(),
        run_len: Vec::new(),
        run_zones: Vec::new(),
    };
    tree.build()?;
    tree.encode(on_arcs)
}

/// Whether `p` is inside the 1e-7° world rectangle.
fn in_domain(p: [i32; 2]) -> bool {
    (X_MIN..=X_MIN + X_SPAN).contains(&i64::from(p[0]))
        && (Y_MIN..=Y_MIN + Y_SPAN).contains(&i64::from(p[1]))
}

/// Interned region sets: each distinct sorted set once, numbered by first use.
#[derive(Default)]
struct Sets {
    ids: BTreeMap<Vec<u32>, u32>,
    list: Vec<Vec<u32>>,
}

impl Sets {
    fn id(&mut self, set: &[u32]) -> Result<u32, Error> {
        if let Some(&id) = self.ids.get(set) {
            return Ok(id);
        }
        let id = u32::try_from(self.list.len()).map_err(|_| Error::Write("too many sets"))?;
        self.ids.insert(set.to_vec(), id);
        self.list.push(set.to_vec());
        Ok(id)
    }

    fn get(&self, id: u32) -> &[u32] {
        self.list.get(id as usize).map_or(&[], Vec::as_slice)
    }
}

/// Toggles every member of `set` in the sorted `zones`.
fn toggle_all(zones: &mut Vec<u32>, set: &[u32]) {
    for &z in set {
        match zones.binary_search(&z) {
            Ok(i) => {
                zones.remove(i);
            }
            Err(i) => zones.insert(i, z),
        }
    }
}

/// Every distinct undirected edge of the rings with the regions it borders an odd number of
/// times (the others cancel), as `(a, b, set)` with `a < b`.
fn border_edges(
    regions: &[Vec<Vec<[i32; 2]>>],
    sets: &mut Sets,
) -> Result<Vec<(Pt, Pt, u32)>, Error> {
    let inside = |p: &[i32; 2]| in_domain(*p);
    let mut rec: Vec<(Pt, Pt, u32)> = Vec::new();
    for (zone, rings) in (0_u32..).zip(regions) {
        for ring in rings {
            if !ring.iter().all(inside) {
                return Err(Error::Write("a region vertex is outside the domain"));
            }
            let closing = match (ring.first(), ring.last()) {
                (Some(f), Some(l)) if f != l => Some([*l, *f]),
                _ => None,
            };
            for w in ring.windows(2).map(|w| [w[0], w[1]]).chain(closing) {
                let (a, b) = ((w[0][0], w[0][1]), (w[1][0], w[1][1]));
                if a != b {
                    rec.push((a.min(b), a.max(b), zone));
                }
            }
        }
    }
    rec.sort_unstable();
    let mut out = Vec::new();
    let mut zones: Vec<u32> = Vec::new();
    let mut i = 0;
    while let Some(&(a, b, _)) = rec.get(i) {
        zones.clear();
        while let Some(&(a2, b2, z)) = rec.get(i) {
            if (a2, b2) != (a, b) {
                break;
            }
            // Sorted by zone within the edge: toggling keeps odd multiplicities.
            if zones.last() == Some(&z) {
                zones.pop();
            } else {
                zones.push(z);
            }
            i += 1;
        }
        if !zones.is_empty() {
            out.push((a, b, sets.id(&zones)?));
        }
    }
    Ok(out)
}

/// The borders as arcs: maximal chains of edges with the same region set through vertices
/// of degree 2, concatenated. Edge `e` joins vertices `e` and `e + 1` when `set[e]` is a set.
struct Arcs {
    x: Vec<i32>,
    y: Vec<i32>,
    /// The set of the edge starting at each vertex; `u32::MAX` at an arc's last vertex.
    set: Vec<u32>,
}

impl Arcs {
    fn build(edges: &[(Pt, Pt, u32)]) -> Self {
        let mut verts: Vec<Pt> = edges.iter().flat_map(|e| [e.0, e.1]).collect();
        verts.sort_unstable();
        verts.dedup();
        let id = |p: Pt| verts.binary_search(&p).unwrap_or(0);
        let ends: Vec<(usize, usize)> = edges.iter().map(|e| (id(e.0), id(e.1))).collect();
        // Incidence lists (CSR), each in edge order.
        let mut start = vec![0_usize; verts.len() + 1];
        for &(a, b) in &ends {
            start[a + 1] += 1;
            start[b + 1] += 1;
        }
        for i in 0..verts.len() {
            start[i + 1] += start[i];
        }
        let mut fill = start.clone();
        let mut inc = vec![0_usize; start[verts.len()]];
        for (e, &(a, b)) in ends.iter().enumerate() {
            for v in [a, b] {
                inc[fill[v]] = e;
                fill[v] += 1;
            }
        }
        let incident = |v: usize| &inc[start[v]..start[v + 1]];
        // A vertex ends arcs unless exactly two edges of one set meet there.
        let joint = |v: usize| match incident(v) {
            [e, f] => edges[*e].2 != edges[*f].2,
            _ => true,
        };
        let mut used = vec![false; edges.len()];
        let mut arcs: Vec<(Vec<usize>, u32)> = Vec::new();
        let mut walk = |from: usize, first: usize, used: &mut Vec<bool>| {
            let mut path = vec![from];
            let (mut v, mut e) = (from, first);
            loop {
                used[e] = true;
                let (a, b) = ends[e];
                v = if a == v { b } else { a };
                path.push(v);
                if v == from || joint(v) {
                    break;
                }
                match incident(v).iter().find(|&&f| !used[f]) {
                    Some(&f) => e = f,
                    None => break,
                }
            }
            arcs.push((path, edges[first].2));
        };
        for v in (0..verts.len()).filter(|&v| joint(v)) {
            for &e in incident(v) {
                if !used[e] {
                    walk(v, e, &mut used);
                }
            }
        }
        // What is left are closed rings through degree-2 vertices of one set.
        for v in 0..verts.len() {
            for &e in incident(v) {
                if !used[e] {
                    walk(v, e, &mut used);
                }
            }
        }
        // Arcs in Morton order of their first vertex: neighbouring arcs, short deltas.
        let key = |p: Pt| {
            super::morton(
                super::index(X_MIN, X_SPAN, p.0),
                super::index(Y_MIN, Y_SPAN, p.1),
            )
        };
        arcs.sort_by_key(|(path, _)| {
            let p = verts[path[0]];
            (key(p), p, path.get(1).map(|&v| verts[v]))
        });
        let n: usize = arcs.iter().map(|a| a.0.len()).sum();
        let (mut x, mut y, mut set) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );
        for (path, s) in arcs {
            for (i, &v) in path.iter().enumerate() {
                x.push(verts[v].0);
                y.push(verts[v].1);
                set.push(if i + 1 < path.len() { s } else { u32::MAX });
            }
        }
        Self { x, y, set }
    }

    fn edge(&self, e: u32) -> (Pt, Pt) {
        let e = e as usize;
        ((self.x[e], self.y[e]), (self.x[e + 1], self.y[e + 1]))
    }

    fn edges(&self) -> impl Iterator<Item = u32> + '_ {
        (0_u32..)
            .zip(&self.set)
            .filter(|(_, s)| **s != u32::MAX)
            .map(|(e, _)| e)
    }
}

struct Leaf {
    key: u64,
    level: u32,
    zones: u32,
    nudge: u32,
    first_run: u32,
}

struct Tree<'a> {
    arcs: &'a Arcs,
    sets: Sets,
    opts: Options,
    leaves: Vec<Leaf>,
    run_edge: Vec<u32>,
    run_len: Vec<u32>,
    run_zones: Vec<u32>,
}

impl Tree<'_> {
    /// The first reference candidate of `c` on no line of `list`'s edges.
    fn nudge(&self, c: Cell, list: &[u32]) -> Result<(u32, Pt), Error> {
        for k in 0..NUDGES {
            let r = reference(c, k);
            if list.iter().all(|&e| {
                let (a, b) = self.arcs.edge(e);
                orient(a, b, r) != 0
            }) {
                return Ok((k, r));
            }
        }
        Err(Error::Write(
            "no reference point avoids the edges of a cell",
        ))
    }

    fn build(&mut self) -> Result<(), Error> {
        let all: Vec<u32> = self.arcs.edges().collect();
        let root = cell(0, 0);
        let (k, r) = self.nudge(root, &all)?;
        // The regions at the root's point: an even–odd ray cast to the east.
        let mut zones = Vec::new();
        for &e in &all {
            let (a, b) = self.arcs.edge(e);
            if (a.1 > r.1) != (b.1 > r.1) {
                let lhs = (i128::from(r.0) - i128::from(a.0)) * (i128::from(b.1) - i128::from(a.1));
                let rhs = (i128::from(b.0) - i128::from(a.0)) * (i128::from(r.1) - i128::from(a.1));
                let east = if b.1 > a.1 { lhs < rhs } else { lhs > rhs };
                if east {
                    let set = self.sets.get(self.arcs.set[e as usize]).to_vec();
                    toggle_all(&mut zones, &set);
                }
            }
        }
        self.node(0, 0, all, &zones, (k, r))
    }

    fn node(
        &mut self,
        key: u64,
        level: u32,
        list: Vec<u32>,
        zones: &[u32],
        (k, r): (u32, Pt),
    ) -> Result<(), Error> {
        if list.len() <= self.opts.leaf_max || level == LEVELS {
            return self.leaf(key, level, list, zones, k);
        }
        let step = 1_u64 << (2 * (LEVELS - level - 1));
        let kids: [Vec<u32>; 4] = core::array::from_fn(|q| {
            let c = cell(key + q as u64 * step, level + 1);
            list.iter()
                .copied()
                .filter(|&e| {
                    let (a, b) = self.arcs.edge(e);
                    touches(c, a, b)
                })
                .collect()
        });
        // Stuck: a few edges that meet at a point or overlap stay together at every level, so
        // splitting would run to the deepest level along them. A child keeping the whole list
        // while another child has edges too is that case; a cluster inside one child is not.
        let whole = kids.iter().any(|k| k.len() == list.len());
        let busy = kids.iter().filter(|k| !k.is_empty()).count();
        if list.len() <= STUCK && whole && busy >= 2 {
            return self.leaf(key, level, list, zones, k);
        }
        for (q, child) in (0_u64..).zip(kids) {
            let ck = key + q * step;
            let c = cell(ck, level + 1);
            // The child's point avoids the parent's edges: the segment between the two points
            // is counted against them.
            let (ckn, rc) = self.nudge(c, &list)?;
            let mut cz = zones.to_vec();
            for &e in &list {
                let (a, b) = self.arcs.edge(e);
                if crosses(r, rc, a, b) {
                    let set = self.sets.get(self.arcs.set[e as usize]).to_vec();
                    toggle_all(&mut cz, &set);
                }
            }
            self.node(ck, level + 1, child, &cz, (ckn, rc))?;
        }
        Ok(())
    }

    fn leaf(
        &mut self,
        key: u64,
        level: u32,
        mut list: Vec<u32>,
        zones: &[u32],
        k: u32,
    ) -> Result<(), Error> {
        list.sort_unstable();
        let first_run =
            u32::try_from(self.run_edge.len()).map_err(|_| Error::Write("too many runs"))?;
        let mut all = zones.to_vec();
        let mut i = 0;
        while let Some(&start) = list.get(i) {
            let mut len = 1;
            while list.get(i + len).copied() == u32::try_from(len).ok().map(|l| start + l) {
                len += 1;
            }
            let set = self.arcs.set[start as usize];
            for &z in self.sets.get(set) {
                if let Err(at) = all.binary_search(&z) {
                    all.insert(at, z);
                }
            }
            self.run_edge.push(start);
            self.run_len
                .push(u32::try_from(len).map_err(|_| Error::Write("a run is too long"))?);
            self.run_zones.push(set);
            i += len;
        }
        if all.len() > MAX_SET {
            return Err(Error::Write("a leaf borders too many regions"));
        }
        let zones = self.sets.id(zones)?;
        self.leaves.push(Leaf {
            key,
            level,
            zones,
            nudge: if list.is_empty() { 0 } else { k },
            first_run,
        });
        Ok(())
    }

    /// The fast answer for top cell `c` whose first owned leaf is `first`: 1 + the region set
    /// of the whole cell if no edge touches it, else 0. The cell then lies inside one leaf
    /// (the only reason to split it is edges); its set is the leaf's, toggled by the leaf's
    /// edges crossed on the way from the leaf's reference point to the cell's.
    fn top_zone(&mut self, c: usize, first: usize) -> Result<u32, Error> {
        let shift = 2 * (LEVELS - self.opts.top);
        let start = (c as u64) << shift;
        let h = if self.leaves.get(first).is_some_and(|l| l.key == start) {
            first
        } else {
            match first.checked_sub(1) {
                Some(h) => h,
                None => return Ok(0),
            }
        };
        let Some(leaf) = self.leaves.get(h) else {
            return Ok(0);
        };
        if leaf.level > self.opts.top {
            return Ok(0);
        }
        let end = self
            .leaves
            .get(h + 1)
            .map_or(self.run_edge.len(), |m| m.first_run as usize);
        if end == leaf.first_run as usize {
            return Ok(leaf.zones + 1);
        }
        let top = cell(start, self.opts.top);
        let (from, to) = (leaf.first_run as usize, end);
        let mut edges = Vec::new();
        for run in from..to {
            for e in self.run_edge[run]..self.run_edge[run] + self.run_len[run] {
                let (a, b) = self.arcs.edge(e);
                if touches(top, a, b) {
                    return Ok(0);
                }
                edges.push(e);
            }
        }
        let mut zones = self.sets.get(leaf.zones).to_vec();
        if !edges.is_empty() {
            let r = reference(cell(leaf.key, leaf.level), leaf.nudge);
            let p = reference(top, 0);
            for e in edges {
                let (a, b) = self.arcs.edge(e);
                if crosses(r, p, a, b) {
                    let set = self.sets.get(self.arcs.set[e as usize]).to_vec();
                    toggle_all(&mut zones, &set);
                }
            }
        }
        Ok(self.sets.id(&zones)? + 1)
    }

    fn encode(&mut self, on_arcs: Option<(&[u32], &[usize])>) -> Result<Vec<u8>, Error> {
        let to32 = |v: usize| u32::try_from(v).map_err(|_| Error::Write("too many leaves"));
        let shift = 2 * (LEVELS - self.opts.top);
        let cells = 1_usize << (2 * self.opts.top);
        let mut dir = Vec::with_capacity(cells + 1);
        let mut i = 0;
        for c in 0..cells as u64 {
            while self.leaves.get(i).is_some_and(|l| l.key < c << shift) {
                i += 1;
            }
            dir.push(to32(i)?);
        }
        dir.push(to32(self.leaves.len())?);
        let mut top_zones = Vec::with_capacity(cells);
        for (c, &first) in dir.iter().enumerate().take(cells) {
            let z = self.top_zone(c, first as usize)?;
            top_zones.push(z);
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "below 2^32 with the top at MIN_TOP or more"
        )]
        let key: Vec<u32> = self
            .leaves
            .iter()
            .map(|l| (l.key & ((1 << shift) - 1)) as u32)
            .collect();
        let level: Vec<u32> = self.leaves.iter().map(|l| l.level).collect();
        let zones: Vec<u32> = self.leaves.iter().map(|l| l.zones).collect();
        let nudge: Vec<u32> = self.leaves.iter().map(|l| l.nudge).collect();
        let mut runs: Vec<u32> = self.leaves.iter().map(|l| l.first_run).collect();
        runs.push(to32(self.run_edge.len())?);
        let mut set_start = vec![0_u32];
        let mut set_zones = Vec::new();
        for s in &self.sets.list {
            set_zones.extend_from_slice(s);
            set_start.push(to32(set_zones.len())?);
        }
        // Own vertices, or on ARCS each run's first vertex (and its arc's scale when any is set).
        let (x, y, scale): (Vec<u32>, Vec<u32>, Vec<u32>) = match on_arcs {
            None => (
                self.arcs.x.iter().map(|&v| bias(v)).collect(),
                self.arcs.y.iter().map(|&v| bias(v)).collect(),
                Vec::new(),
            ),
            Some((scales, starts)) => {
                let x = self
                    .run_edge
                    .iter()
                    .map(|&e| bias(self.arcs.x[e as usize]))
                    .collect();
                let y = self
                    .run_edge
                    .iter()
                    .map(|&e| bias(self.arcs.y[e as usize]))
                    .collect();
                let scale = if scales.iter().any(|&s| s != 0) {
                    self.run_edge
                        .iter()
                        .map(|e| {
                            let a = starts.partition_point(|&s| s <= *e as usize) - 1;
                            scales[a]
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                (x, y, scale)
            }
        };
        let (x_id, y_id) = if on_arcs.is_some() {
            (RUN_X, RUN_Y)
        } else {
            (X, Y)
        };
        let cols = [
            (TOP, encode_direct(&[self.opts.top])),
            (DIR, encode_direct(&dir)),
            (KEY, encode_direct(&key)),
            (LEVEL, encode_direct(&level)),
            (ZONES, encode_direct(&zones)),
            (NUDGE, encode_direct(&nudge)),
            (RUNS, encode_direct(&runs)),
            (RUN_EDGE, encode_direct(&self.run_edge)),
            (RUN_LEN, encode_direct(&self.run_len)),
            (RUN_ZONES, encode_direct(&self.run_zones)),
            (SET_START, encode_direct(&set_start)),
            (SET_ZONES, encode_direct(&set_zones)),
            (TOP_ZONES, encode_direct(&top_zones)),
            (x_id, encode_direct(&x)),
            (y_id, encode_direct(&y)),
            (RUN_SCALE, encode_direct(&scale)),
        ];
        let mut keep: Vec<(u16, &[u8])> = cols
            .iter()
            .filter(|(id, _)| *id != NUDGE || nudge.iter().any(|&n| n != 0))
            .filter(|(id, _)| *id != RUN_SCALE || !scale.is_empty())
            .map(|(id, b)| (*id, b.as_slice()))
            .collect();
        keep.sort_by_key(|c| c.0);
        table::encode(&keep).map_err(|_| Error::Write("the region table could not be laid out"))
    }
}

/// Adds the [`PLACE`](super::PLACE) column to an encoded REGION payload: `places[id]` is the
/// PLACES row of region `id`'s place. Replaces an existing link.
///
/// # Errors
/// [`Error::Write`] if `body` is not a column table or `places` is empty.
pub fn link(body: &[u8], places: &[u32]) -> Result<Vec<u8>, Error> {
    let bad = Error::Write("the region payload is not a column table");
    if places.is_empty() {
        return Err(Error::Write("a region link needs a row for every region"));
    }
    let t = table::Table::parse(body).map_err(|_| bad)?;
    let column = encode_direct(places);
    let mut cols: Vec<(u16, table::Codec, &[u8])> = Vec::new();
    for id in t.ids().filter(|&id| id != super::PLACE) {
        let (codec, bytes) = t.column_raw(id).ok_or(bad)?;
        cols.push((id, codec, bytes));
    }
    cols.push((super::PLACE, table::Codec::Bitpack, &column));
    table::encode_with(&cols).map_err(|_| bad)
}

/// The reference answer for `regions` (as given to [`encode`]) at `at`: an even–odd ray cast to
/// the east over every ring edge with the rule of the module documentation, in increasing ID
/// order. O(edges); independent of the index, for tests and fuzzing.
#[must_use]
pub fn even_odd(
    regions: &[Vec<Vec<[i32; 2]>>],
    at: teigeodata_core::Point<teigeodata_core::E7>,
) -> Vec<u32> {
    // Its own reading of the domain's edges, not the reader's, so the two are compared.
    let x = if at.lon() == 1_800_000_000 {
        -1_800_000_000
    } else {
        at.lon()
    };
    let y = at.lat().min(899_999_999);
    let mut out = Vec::new();
    for (z, rings) in (0_u32..).zip(regions) {
        let mut inside = false;
        for ring in rings {
            let n = ring.len();
            for k in 0..n {
                let (a, b) = (ring[k], ring[(k + 1) % n]);
                if a == b || (a[1] > y) == (b[1] > y) {
                    continue;
                }
                let lhs =
                    (i128::from(x) - i128::from(a[0])) * (i128::from(b[1]) - i128::from(a[1]));
                let rhs =
                    (i128::from(b[0]) - i128::from(a[0])) * (i128::from(y) - i128::from(a[1]));
                if if b[1] > a[1] { lhs < rhs } else { lhs > rhs } {
                    inside = !inside;
                }
            }
        }
        if inside {
            out.push(z);
        }
    }
    out
}
