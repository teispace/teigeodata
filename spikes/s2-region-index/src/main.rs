//! Phase 0 spike S2: an exact region index on the world timezone boundaries (ADR-0013).
//!
//! Geometry is quantised once to the 1e-7° grid (ADR-0029). A quadtree splits the world until a
//! cell holds at most `LEAF_MAX` source edges. Every cell knows the zone parity at a reference
//! point near its centre, derived from its parent by counting crossings along the segment
//! between the two reference points (an S2ShapeIndex-style construction). A query descends to
//! its leaf and counts crossings from the leaf's reference point to the query point against the
//! leaf's edges. Predicates are exact integer arithmetic (`i128`).
//!
//! Checks: zero mismatches against an independent ray-casting reference on uniform random
//! points and on adversarial points 1 grid unit from random edges; agreement and speed against
//! `tzf-rs` (simplified data). Reports build time, index shape, size estimate and ns/lookup.
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::manual_midpoint,
    clippy::panic,
    clippy::type_complexity,
    clippy::explicit_iter_loop,
    missing_docs,
    unreachable_pub,
    reason = "a measurement spike: prints its report and fails loudly on bad input"
)]

use std::hint::black_box;
use std::time::Instant;

use serde::Deserialize;

const MAX_DEPTH: u32 = 28;
const WORLD: Cell = Cell {
    x0: -1_800_000_000,
    y0: -900_000_000,
    x1: 1_800_000_000,
    y1: 900_000_000,
};
/// A point outside every zone, to the east of the world.
const FAR_X: i32 = 1_900_000_000;
const NONE: u16 = u16::MAX;

// ---------- input ----------

#[derive(Deserialize)]
struct Collection {
    features: Vec<Feature>,
}

#[derive(Deserialize)]
struct Feature {
    properties: Props,
    geometry: Geometry,
}

#[derive(Deserialize)]
struct Props {
    tzid: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", content = "coordinates")]
enum Geometry {
    Polygon(Vec<Vec<[f64; 2]>>),
    MultiPolygon(Vec<Vec<Vec<[f64; 2]>>>),
}

#[derive(Clone, Copy)]
struct Edge {
    ax: i32,
    ay: i32,
    bx: i32,
    by: i32,
    zone: u16,
}

fn q(v: f64) -> i32 {
    (v * 1e7).round() as i32
}

fn load(path: &str) -> (Vec<String>, Vec<Edge>, usize) {
    let text = std::fs::read(path).expect("read the GeoJSON");
    let fc: Collection = serde_json::from_slice(&text).expect("parse the GeoJSON");
    let mut names = Vec::new();
    let mut edges = Vec::new();
    let mut vertices = 0;
    for (zone, f) in fc.features.into_iter().enumerate() {
        names.push(f.properties.tzid);
        let polys = match f.geometry {
            Geometry::Polygon(p) => vec![p],
            Geometry::MultiPolygon(m) => m,
        };
        for poly in polys {
            for ring in poly {
                vertices += ring.len();
                let pts: Vec<(i32, i32)> = ring.iter().map(|c| (q(c[0]), q(c[1]))).collect();
                for w in pts.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    if a != b {
                        edges.push(Edge {
                            ax: a.0,
                            ay: a.1,
                            bx: b.0,
                            by: b.1,
                            zone: zone as u16,
                        });
                    }
                }
            }
        }
    }
    (names, edges, vertices)
}

// ---------- exact predicates ----------

/// Sign of the cross product (b - a) × (c - a).
fn orient(ax: i32, ay: i32, bx: i32, by: i32, cx: i32, cy: i32) -> i32 {
    let v = (i128::from(bx) - i128::from(ax)) * (i128::from(cy) - i128::from(ay))
        - (i128::from(by) - i128::from(ay)) * (i128::from(cx) - i128::from(ax));
    v.signum() as i32
}

/// Does segment c→p cross edge a→b? Half-open on the edge's endpoints relative to line c–p, so
/// a vertex lying exactly on the segment is counted once.
fn crosses(e: &Edge, cx: i32, cy: i32, px: i32, py: i32) -> bool {
    let o1 = orient(cx, cy, px, py, e.ax, e.ay) > 0;
    let o2 = orient(cx, cy, px, py, e.bx, e.by) > 0;
    if o1 == o2 {
        return false;
    }
    let o3 = orient(e.ax, e.ay, e.bx, e.by, cx, cy);
    let o4 = orient(e.ax, e.ay, e.bx, e.by, px, py);
    o3 * o4 < 0
}

fn on_segment(e: &Edge, x: i32, y: i32) -> bool {
    orient(e.ax, e.ay, e.bx, e.by, x, y) == 0
        && x >= e.ax.min(e.bx)
        && x <= e.ax.max(e.bx)
        && y >= e.ay.min(e.by)
        && y <= e.ay.max(e.by)
}

#[derive(Clone, Copy)]
struct Cell {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl Cell {
    /// The reference point: the centre, nudged off the round numbers tz borders follow, always
    /// strictly inside the cell and never exactly on one of `edges` (a reference point on an
    /// edge would make that edge's crossing invisible to both segments that end there).
    fn reference(self, all: &[Edge], edges: &[u32]) -> (i32, i32) {
        let cx = ((i64::from(self.x0) + i64::from(self.x1)) / 2) as i32;
        let cy = ((i64::from(self.y0) + i64::from(self.y1)) / 2) as i32;
        let hx = (self.x1 - self.x0) / 2;
        let hy = (self.y1 - self.y0) / 2;
        for k in 0..64i32 {
            let x = cx + (7 + 31 * k).rem_euclid(hx.max(1));
            let y = cy + (13 + 17 * k).rem_euclid(hy.max(1));
            if x < self.x1
                && y < self.y1
                && !edges.iter().any(|&i| on_segment(&all[i as usize], x, y))
            {
                return (x, y);
            }
        }
        panic!("no free reference point in a cell");
    }

    fn children(self) -> [Cell; 4] {
        let mx = ((i64::from(self.x0) + i64::from(self.x1)) / 2) as i32;
        let my = ((i64::from(self.y0) + i64::from(self.y1)) / 2) as i32;
        [
            Cell {
                x0: self.x0,
                y0: self.y0,
                x1: mx,
                y1: my,
            },
            Cell {
                x0: mx,
                y0: self.y0,
                x1: self.x1,
                y1: my,
            },
            Cell {
                x0: self.x0,
                y0: my,
                x1: mx,
                y1: self.y1,
            },
            Cell {
                x0: mx,
                y0: my,
                x1: self.x1,
                y1: self.y1,
            },
        ]
    }

    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }

    /// Exact segment–box intersection (separating axes: x, y, and the segment's normal).
    fn touches(self, e: &Edge) -> bool {
        if e.ax.max(e.bx) < self.x0
            || e.ax.min(e.bx) > self.x1
            || e.ay.max(e.by) < self.y0
            || e.ay.min(e.by) > self.y1
        {
            return false;
        }
        let corners = [
            (self.x0, self.y0),
            (self.x1, self.y0),
            (self.x0, self.y1),
            (self.x1, self.y1),
        ];
        let signs: Vec<i32> = corners
            .iter()
            .map(|&(x, y)| orient(e.ax, e.ay, e.bx, e.by, x, y))
            .collect();
        !(signs.iter().all(|&s| s > 0) || signs.iter().all(|&s| s < 0))
    }
}

// ---------- the index ----------

enum Node {
    Inner([u32; 4]),
    Leaf(u32),
}

struct Leaf {
    rx: i32,
    ry: i32,
    /// Zones containing the whole leaf (no edges here). Usually one; more where source zones overlap.
    covering: Vec<u16>,
    /// Zones with edges in this leaf, and whether the reference point is inside each.
    zones: Vec<(u16, bool)>,
    edges: Vec<u32>,
}

struct Index {
    leaf_max: usize,
    nodes: Vec<Node>,
    cells: Vec<Cell>,
    leaves: Vec<Leaf>,
    edges: Vec<Edge>,
}

/// Parity of every zone in `zones` at `to`, from its parity at `from`, given the edges between.
fn advance(
    edges: &[Edge],
    list: &[u32],
    zones: &[(u16, bool)],
    from: (i32, i32),
    to: (i32, i32),
) -> Vec<(u16, bool)> {
    let mut out = zones.to_vec();
    for &i in list {
        let e = &edges[i as usize];
        if crosses(e, from.0, from.1, to.0, to.1)
            && let Some(z) = out.iter_mut().find(|(z, _)| *z == e.zone)
        {
            z.1 = !z.1;
        }
    }
    out
}

impl Index {
    fn build(edges: Vec<Edge>, zone_count: usize, leaf_max: usize) -> Self {
        let mut idx = Index {
            leaf_max,
            nodes: Vec::new(),
            cells: Vec::new(),
            leaves: Vec::new(),
            edges,
        };
        // Root: parity of every zone at the reference point, by a segment to a point outside all.
        let all: Vec<u32> = (0..idx.edges.len() as u32).collect();
        let r = WORLD.reference(&idx.edges, &all);
        let mut parity = vec![false; zone_count];
        for e in &idx.edges {
            if crosses(e, r.0, r.1, FAR_X, r.1) {
                parity[e.zone as usize] = !parity[e.zone as usize];
            }
        }
        let zones: Vec<(u16, bool)> = (0..zone_count).map(|z| (z as u16, parity[z])).collect();
        idx.node(WORLD, 0, all, zones, Vec::new(), r);
        idx
    }

    fn node(
        &mut self,
        cell: Cell,
        depth: u32,
        list: Vec<u32>,
        zones: Vec<(u16, bool)>,
        covering: Vec<u16>,
        r: (i32, i32),
    ) -> u32 {
        let id = self.nodes.len() as u32;
        self.cells.push(cell);
        if list.len() <= self.leaf_max || depth >= MAX_DEPTH {
            self.leaves.push(Leaf {
                rx: r.0,
                ry: r.1,
                covering,
                zones,
                edges: list,
            });
            self.nodes.push(Node::Leaf(self.leaves.len() as u32 - 1));
            return id;
        }
        self.nodes.push(Node::Inner([0; 4]));
        let from = r;
        let mut kids = [0u32; 4];
        for (k, child) in cell.children().into_iter().enumerate() {
            let child_list: Vec<u32> = list
                .iter()
                .copied()
                .filter(|&i| child.touches(&self.edges[i as usize]))
                .collect();
            // The child's point must avoid the parent's edges too: the segment from the parent's
            // point is counted against the parent's list.
            let to = child.reference(&self.edges, &list);
            let moved = advance(&self.edges, &list, &zones, from, to);
            let mut present: Vec<u16> = child_list
                .iter()
                .map(|&i| self.edges[i as usize].zone)
                .collect();
            present.sort_unstable();
            present.dedup();
            let mut cover = covering.clone();
            let mut child_zones = Vec::with_capacity(present.len());
            for (z, inside) in moved {
                if present.binary_search(&z).is_ok() {
                    child_zones.push((z, inside));
                } else if inside {
                    cover.push(z);
                }
            }
            cover.sort_unstable();
            kids[k] = self.node(child, depth + 1, child_list, child_zones, cover, to);
        }
        self.nodes[id as usize] = Node::Inner(kids);
        id
    }

    fn lookup(&self, x: i32, y: i32) -> u16 {
        let mut n = 0u32;
        loop {
            match &self.nodes[n as usize] {
                Node::Inner(kids) => {
                    n = *kids
                        .iter()
                        .find(|&&k| self.cells[k as usize].contains(x, y))
                        .unwrap_or(&kids[0]);
                }
                Node::Leaf(l) => {
                    // No allocation: parity as a bitmask over the leaf's zone slots.
                    let leaf = &self.leaves[*l as usize];
                    let mut bits: u128 = 0;
                    for (k, (_, inside)) in leaf.zones.iter().enumerate() {
                        bits |= u128::from(*inside) << k;
                    }
                    for &i in &leaf.edges {
                        let e = &self.edges[i as usize];
                        if crosses(e, leaf.rx, leaf.ry, x, y)
                            && let Some(k) = leaf.zones.iter().position(|(z, _)| *z == e.zone)
                        {
                            bits ^= 1 << k;
                        }
                    }
                    let edge_zone = leaf
                        .zones
                        .iter()
                        .enumerate()
                        .filter(|(k, _)| bits >> k & 1 == 1)
                        .map(|(_, (z, _))| *z)
                        .min()
                        .unwrap_or(NONE);
                    return edge_zone.min(leaf.covering.first().copied().unwrap_or(NONE));
                }
            }
        }
    }

    /// Every zone containing the point (zones overlap in a few disputed areas of the source).
    fn lookup_all(&self, x: i32, y: i32) -> Vec<u16> {
        let mut n = 0u32;
        loop {
            match &self.nodes[n as usize] {
                Node::Inner(kids) => {
                    n = *kids
                        .iter()
                        .find(|&&k| self.cells[k as usize].contains(x, y))
                        .unwrap_or(&kids[0]);
                }
                Node::Leaf(l) => {
                    let leaf = &self.leaves[*l as usize];
                    let mut zones = leaf.zones.clone();
                    for &i in &leaf.edges {
                        let e = &self.edges[i as usize];
                        if crosses(e, leaf.rx, leaf.ry, x, y)
                            && let Some(z) = zones.iter_mut().find(|(z, _)| *z == e.zone)
                        {
                            z.1 = !z.1;
                        }
                    }
                    let mut out: Vec<u16> =
                        zones.iter().filter(|(_, i)| *i).map(|(z, _)| *z).collect();
                    out.extend_from_slice(&leaf.covering);
                    out.sort_unstable();
                    out.dedup();
                    return out;
                }
            }
        }
    }
}

// ---------- the independent reference ----------

/// Even–odd ray casting to the east, written independently of `crosses`: every zone holding
/// the point, and whether the point lies exactly on an edge.
fn reference_all(edges: &[Edge], zone_bbox: &[Cell], x: i32, y: i32) -> (Vec<u16>, bool) {
    let mut parity = vec![false; zone_bbox.len()];
    let mut on_edge = false;
    for e in edges {
        let b = &zone_bbox[e.zone as usize];
        if !b.contains(x, y) {
            continue;
        }
        if !on_edge && on_segment(e, x, y) {
            on_edge = true;
        }
        if (e.ay > y) != (e.by > y) {
            let lhs = (i128::from(x) - i128::from(e.ax)) * (i128::from(e.by) - i128::from(e.ay));
            let rhs = (i128::from(e.bx) - i128::from(e.ax)) * (i128::from(y) - i128::from(e.ay));
            let east = if e.by > e.ay { lhs < rhs } else { lhs > rhs };
            if east {
                parity[e.zone as usize] = !parity[e.zone as usize];
            }
        }
    }
    let zones = parity
        .iter()
        .enumerate()
        .filter(|(_, p)| **p)
        .map(|(z, _)| z as u16)
        .collect();
    (zones, on_edge)
}

// ---------- driver ----------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (i64::from(hi) - i64::from(lo)) as u64) as i32
    }
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: spike-s2-region-index <combined-with-oceans.json> [NP.txt]");
    let t = Instant::now();
    let (names, edges, vertices) = load(&path);
    println!("# Spike S2: exact region index on timezone-boundary-builder 2026f (with oceans)\n");
    println!(
        "- zones: {}, vertices: {vertices}, edges: {}; load and quantise: {:.1} s",
        names.len(),
        edges.len(),
        t.elapsed().as_secs_f64()
    );

    let mut zone_bbox = vec![
        Cell {
            x0: i32::MAX,
            y0: i32::MAX,
            x1: i32::MIN,
            y1: i32::MIN
        };
        names.len()
    ];
    for e in &edges {
        let b = &mut zone_bbox[e.zone as usize];
        b.x0 = b.x0.min(e.ax).min(e.bx);
        b.y0 = b.y0.min(e.ay).min(e.by);
        b.x1 = b.x1.max(e.ax).max(e.bx);
        b.y1 = b.y1.max(e.ay).max(e.by);
    }

    let t = Instant::now();
    let leaf_max: usize = std::env::var("LEAF_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    let index = Index::build(edges, names.len(), leaf_max);
    let build = t.elapsed().as_secs_f64();
    let refs: usize = index.leaves.iter().map(|l| l.edges.len()).sum();
    let max_refs = index
        .leaves
        .iter()
        .map(|l| l.edges.len())
        .max()
        .unwrap_or(0);
    let empty = index.leaves.iter().filter(|l| l.edges.is_empty()).count();
    println!(
        "- leaf max: {leaf_max} edges; build: {build:.1} s; nodes: {}, leaves: {} ({} without edges), edge refs: {refs} (max {max_refs} per leaf)",
        index.nodes.len(),
        index.leaves.len(),
        empty
    );
    // Edge refs as runs of consecutive edge ids (edges of one ring are numbered in order).
    let runs: usize = index
        .leaves
        .iter()
        .map(|l| {
            let mut v = l.edges.clone();
            v.sort_unstable();
            v.windows(2).filter(|w| w[1] != w[0] + 1).count() + usize::from(!v.is_empty())
        })
        .sum();
    let edge_free = index.leaves.iter().filter(|l| l.edges.is_empty()).count();
    // Packed v2: edge-free leaves as (Morton key 8 B + zone 2 B); others as (key 8 B + run
    // offset 4 B + zone-state bits ~2 B); each run as (start u32 + length u8) = 5 B.
    let est2 = edge_free * 10 + (index.leaves.len() - edge_free) * 14 + runs * 5 + vertices * 4;
    println!(
        "- edge refs as runs: {runs} runs ({:.2} refs per run); packed v2 estimate: {:.1} MB (runs {:.1} MB)",
        refs as f64 / runs as f64,
        est2 as f64 / 1e6,
        (runs * 5) as f64 / 1e6
    );
    // Size estimate for a packed form: leaves as Morton keys (8 B) + background (2 B) + an
    // edge-ref range (4 B); edge refs as u32 (4 B); vertices delta-coded at ~4 B per vertex
    // (S1 measured 4.9 B per point pair at 1e-7° for scattered points; rings are denser).
    let est = index.leaves.len() * 14 + refs * 4 + vertices * 4;
    println!(
        "- estimated packed size: {:.1} MB (leaves {:.1} + edge refs {:.1} + vertices {:.1})\n",
        est as f64 / 1e6,
        (index.leaves.len() * 14) as f64 / 1e6,
        (refs * 4) as f64 / 1e6,
        (vertices * 4) as f64 / 1e6
    );

    // Correctness: uniform random points and adversarial near-edge points.
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let uniform: Vec<(i32, i32)> = (0..20_000)
        .map(|_| {
            (
                rng.range(-1_800_000_000, 1_800_000_000),
                rng.range(-899_000_000, 899_000_000),
            )
        })
        .collect();
    let near: Vec<(i32, i32)> = (0..20_000)
        .map(|_| {
            let e = &index.edges[(rng.next() % index.edges.len() as u64) as usize];
            let mx = ((i64::from(e.ax) + i64::from(e.bx)) / 2) as i32;
            let my = ((i64::from(e.ay) + i64::from(e.by)) / 2) as i32;
            let dx = [-1, 0, 1][(rng.next() % 3) as usize];
            let dy = [-1, 0, 1][(rng.next() % 3) as usize];
            (mx + dx, my + dy)
        })
        .collect();
    for (label, set) in [("uniform random", &uniform), ("1 unit from an edge", &near)] {
        let t = Instant::now();
        let threads = 8;
        let chunk = set.len().div_ceil(threads);
        let results: Vec<(i32, i32, Vec<u16>, Vec<u16>, bool)> = std::thread::scope(|s| {
            let handles: Vec<_> = set
                .chunks(chunk)
                .map(|part| {
                    let idx = &index;
                    let bbox = &zone_bbox;
                    s.spawn(move || {
                        part.iter()
                            .map(|&(x, y)| {
                                let a = idx.lookup_all(x, y);
                                let (b, on_edge) = reference_all(&idx.edges, bbox, x, y);
                                assert_eq!(idx.lookup(x, y), a.first().copied().unwrap_or(NONE));
                                (x, y, a, b, on_edge)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().unwrap())
                .collect()
        });
        let on_edge = results.iter().filter(|r| r.4).count();
        let overlap = results.iter().filter(|r| r.3.len() > 1).count();
        let uncovered = results.iter().filter(|r| r.3.is_empty() && !r.4).count();
        let mismatches: Vec<_> = results.iter().filter(|r| !r.4 && r.2 != r.3).collect();
        println!(
            "- exactness, {label}: {} points ({on_edge} exactly on an edge, excluded; {overlap} in more than one zone; {uncovered} in none), {} set mismatches off the edges against the ray-casting reference ({:.0} s)",
            set.len(),
            mismatches.len(),
            t.elapsed().as_secs_f64()
        );
        let n = |z: &u16| {
            names
                .get(*z as usize)
                .map_or("none", String::as_str)
                .to_string()
        };
        for (x, y, a, b, _) in mismatches.iter().take(5) {
            let a: Vec<String> = a.iter().map(n).collect();
            let b: Vec<String> = b.iter().map(n).collect();
            println!(
                "  - ({}, {}): index {a:?} vs reference {b:?}",
                *x as f64 / 1e7,
                *y as f64 / 1e7
            );
        }
        let mut pairs: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for r in results.iter().filter(|r| r.3.len() > 1) {
            *pairs
                .entry(r.3.iter().map(n).collect::<Vec<_>>().join(" + "))
                .or_default() += 1;
        }
        for (k, v) in pairs.iter().take(8) {
            println!("  - overlap {k}: {v} points");
        }
    }

    // Speed: uniform points and real places.
    let mut rng = Rng(42);
    let bench: Vec<(i32, i32)> = (0..1_000_000)
        .map(|_| {
            (
                rng.range(-1_800_000_000, 1_800_000_000),
                rng.range(-899_000_000, 899_000_000),
            )
        })
        .collect();
    let places: Vec<(i32, i32)> = std::env::args().nth(2).map_or_else(Vec::new, |p| {
        std::fs::read_to_string(p)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                Some((q(f.get(5)?.parse().ok()?), q(f.get(4)?.parse().ok()?)))
            })
            .collect()
    });
    println!();
    println!(
        "| lookup set | points | index ns/lookup | tzf-rs (simplified) ns/lookup | agreement with tzf-rs |"
    );
    println!("|---|---:|---:|---:|---:|");
    let finder = tzf_rs::DefaultFinder::new();
    for (label, set) in [
        ("uniform world", &bench),
        ("GeoNames Nepal places", &places),
    ] {
        if set.is_empty() {
            continue;
        }
        let t = Instant::now();
        let mut acc = 0u64;
        for &(x, y) in set.iter() {
            acc += u64::from(black_box(index.lookup(x, y)));
        }
        let ours = t.elapsed().as_nanos() as f64 / set.len() as f64;
        black_box(acc);
        let t = Instant::now();
        let mut agree = 0usize;
        let mut theirs_names = Vec::with_capacity(set.len());
        for &(x, y) in set.iter() {
            theirs_names.push(black_box(
                finder.get_tz_name(f64::from(x) / 1e7, f64::from(y) / 1e7),
            ));
        }
        let theirs = t.elapsed().as_nanos() as f64 / set.len() as f64;
        for (&(x, y), tz) in set.iter().zip(theirs_names) {
            let z = index.lookup(x, y);
            if names.get(z as usize).is_some_and(|n| n == tz) {
                agree += 1;
            }
        }
        println!(
            "| {label} | {} | {ours:.0} | {theirs:.0} | {:.4}% |",
            set.len(),
            100.0 * agree as f64 / set.len() as f64
        );
    }
}
