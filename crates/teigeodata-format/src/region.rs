//! The REGION section: which regions (timezones, later admin units) hold a point, exactly
//! (ADR-0013, D-083).
//!
//! Geometry is on the 1e-7° grid ([ADR-0029]). Borders are stored once, as **arcs**: runs of
//! vertices shared by every region that the border separates. A quadtree over the world
//! rectangle ends in **leaves** of at most a few dozen edges, stored in Morton order. Each leaf
//! keeps the set of regions holding a **reference point** inside it, and the edges that touch it
//! as runs of consecutive edge numbers. A query finds its leaf through a dense **top grid** and a
//! binary search, then counts the crossings between the reference point and the query point. An
//! run crossed an odd number of times toggles its regions in or out of the set. Every predicate
//! is exact integer arithmetic.
//!
//! **Points on a border** belong to the regions just east-north-east of them: a point `p` is
//! read as `p + (ε, ε²)` for an infinitesimal ε (simulation of simplicity). A point is then never
//! on an edge, and the answer equals an even–odd ray cast to the east that counts an edge when
//! its ends lie on opposite sides of the point's latitude (an end exactly on it counts as south)
//! and it crosses strictly east of the point. Longitude 180° is read as −180° (the same
//! meridian), and latitude 90° as 1e-7° south of it.
//!
//! Region IDs are in **precedence order**: where regions overlap, the lowest ID is the single
//! answer (Q-013). The section's item count is the number of regions.
//!
//! | ID | Column | Values |
//! |---:|---|---|
//! | 1 | [`TOP`] | one value: the level `g` of the top grid, [`MIN_TOP`] to [`MAX_TOP`] |
//! | 2 | [`DIR`] | 4^g + 1 values: the number of leaves that start before each top cell, then the leaf count |
//! | 3 | [`KEY`] | per leaf, its Morton key minus its top cell's |
//! | 4 | [`LEVEL`] | per leaf, its level, up to [`LEVELS`] |
//! | 5 | [`ZONES`] | per leaf, the region set at its reference point (the whole leaf if it has no edges) |
//! | 6 | [`NUDGE`] | optional, per leaf: which candidate is its reference point (absent: 0) |
//! | 7 | [`RUNS`] | per leaf and one more: its first run |
//! | 8 | [`RUN_EDGE`] | per run: its first edge, the number of the edge's first vertex |
//! | 9 | [`RUN_LEN`] | per run: its number of edges, at least 1 |
//! | 10 | [`RUN_ZONES`] | per run: the region set its arc toggles |
//! | 11 | [`X`] | per vertex: longitude, [biased](teigeodata_core::bitpack::bias) |
//! | 12 | [`Y`] | per vertex: latitude, biased |
//! | 13 | [`SET_START`] | per region set and one more: its first entry |
//! | 14 | [`SET_ZONES`] | the region IDs of every set, each set strictly increasing |
//! | 15 | [`TOP_ZONES`] | per top cell: 1 + the region set of the leaf without edges that holds the whole cell, or 0 |
//! | 16 | [`PLACE`] | optional, per region: the PLACES row of its place (a timezone area, an admin unit), set by [`link`] |
//!
//! Set `z` is the single region `z` for every region ID `z`, so a leaf or an arc of one region
//! is read without the set tables; sets of none or of several regions follow.
//!
//! [`Region::open`] checks the structure; [`Region::check`] checks every offset, key, set range
//! and reference point. Queries never panic and never allocate. Their answers are exact on a pack
//! built by [`encode`] (feature `write`); the check does not re-derive the region sets from the
//! geometry, so a pack is only as trustworthy as its signer.
//!
//! [ADR-0029]: https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/adr/ADR-0029-coordinate-grid.md

#![allow(
    clippy::many_single_char_names,
    reason = "geometry reads as points a, b, p, r and their signs"
)]

use teigeodata_core::bitpack::{Packed, unbias};
use teigeodata_core::{E7, Point};

use crate::{Error, Kind, Section, columns};

#[cfg(feature = "write")]
mod build;
#[cfg(feature = "write")]
pub use build::{Options, encode, even_odd, link};

/// Column: the top-grid level.
pub const TOP: u16 = 1;
/// Column: the top-grid directory.
pub const DIR: u16 = 2;
/// Column: leaf keys inside their top cell.
pub const KEY: u16 = 3;
/// Column: leaf levels.
pub const LEVEL: u16 = 4;
/// Column: the region set of each leaf.
pub const ZONES: u16 = 5;
/// Column: the reference-point candidate of each leaf.
pub const NUDGE: u16 = 6;
/// Column: each leaf's first run.
pub const RUNS: u16 = 7;
/// Column: each run's first edge.
pub const RUN_EDGE: u16 = 8;
/// Column: each run's edge count.
pub const RUN_LEN: u16 = 9;
/// Column: the region set each run toggles.
pub const RUN_ZONES: u16 = 10;
/// Column: vertex longitudes.
pub const X: u16 = 11;
/// Column: vertex latitudes.
pub const Y: u16 = 12;
/// Column: the start of each region set.
pub const SET_START: u16 = 13;
/// Column: the members of every region set.
pub const SET_ZONES: u16 = 14;
/// Column: per top cell, the set of the edge-free leaf holding it whole (plus 1), else 0.
pub const TOP_ZONES: u16 = 15;
/// Column: per region, the PLACES row of its place (optional).
pub const PLACE: u16 = 16;

/// The deepest quadtree level; leaves there are about 2.4 m by 1.2 m at the equator.
pub const LEVELS: u32 = 24;
/// The smallest top-grid level: below it, a key inside its top cell needs more than 32 bits.
pub const MIN_TOP: u32 = 8;
/// The largest top-grid level (4¹² = 16.8 M directory entries).
pub const MAX_TOP: u32 = 12;
/// The most regions a query collects before toggling ends; a pack built by [`encode`] has at
/// most this many in any leaf.
pub const MAX_SET: usize = 64;

const X_MIN: i64 = -1_800_000_000;
const X_SPAN: i64 = 3_600_000_000;
const Y_MIN: i64 = -900_000_000;
const Y_SPAN: i64 = 1_800_000_000;
/// The reference-point candidates tried per cell.
const NUDGES: u32 = 4096;
/// [`MAX_SET`] as a value count.
#[allow(clippy::cast_possible_truncation, reason = "64")]
const SET_CAP: u32 = MAX_SET as u32;
/// Vertices decoded at a time.
const CHUNK: usize = 128;

fn fail(column: u16, problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::REGION.0,
        column,
        problem,
    }
}

// ---------- the grid ----------

/// A quadtree cell, edges included: `x0 ≤ x ≤ x1`, `y0 ≤ y ≤ y1` on the 1e-7° grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cell {
    x0: i32,
    x1: i32,
    y0: i32,
    y1: i32,
}

/// The `i`-th of `2^level` cell edges across an axis starting at `min` of width `span`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "within the axis, so within i32"
)]
const fn edge(min: i64, span: i64, i: u64, level: u32) -> i32 {
    // i ≤ 2^24 and span < 2^32, so the product fits.
    (min + ((i * span.unsigned_abs()) >> level).cast_signed()) as i32
}

/// Spreads the low 24 bits of `v` to the even bits of the result.
const fn spread(v: u32) -> u64 {
    let mut x = (v & 0x00ff_ffff) as u64;
    x = (x | (x << 16)) & 0x0000_ffff_0000_ffff;
    x = (x | (x << 8)) & 0x00ff_00ff_00ff_00ff;
    x = (x | (x << 4)) & 0x0f0f_0f0f_0f0f_0f0f;
    x = (x | (x << 2)) & 0x3333_3333_3333_3333;
    (x | (x << 1)) & 0x5555_5555_5555_5555
}

/// The even bits of `v`, packed.
#[allow(clippy::cast_possible_truncation, reason = "24 bits remain")]
const fn compact(v: u64) -> u32 {
    let mut x = v & 0x5555_5555_5555_5555;
    x = (x | (x >> 1)) & 0x3333_3333_3333_3333;
    x = (x | (x >> 2)) & 0x0f0f_0f0f_0f0f_0f0f;
    x = (x | (x >> 4)) & 0x00ff_00ff_00ff_00ff;
    x = (x | (x >> 8)) & 0x0000_ffff_0000_ffff;
    x = (x | (x >> 16)) & 0x0000_0000_ffff_ffff;
    x as u32
}

/// The Morton key (at [`LEVELS`]) of a cell column and row.
const fn morton(col: u32, row: u32) -> u64 {
    spread(col) | (spread(row) << 1)
}

/// The cell of `level` whose Morton key (at [`LEVELS`], aligned to the level) is `key`.
const fn cell(key: u64, level: u32) -> Cell {
    let shift = LEVELS - level;
    let col = (compact(key) >> shift) as u64;
    let row = (compact(key >> 1) >> shift) as u64;
    Cell {
        x0: edge(X_MIN, X_SPAN, col, level),
        x1: edge(X_MIN, X_SPAN, col + 1, level),
        y0: edge(Y_MIN, Y_SPAN, row, level),
        y1: edge(Y_MIN, Y_SPAN, row + 1, level),
    }
}

/// The column (or row) at [`LEVELS`] holding `v`: the last `i` with `edge(i) ≤ v`, so a point on
/// a cell edge belongs to the cell above or east of it.
#[allow(clippy::cast_possible_truncation, reason = "clamped below 2^24")]
const fn index(min: i64, span: i64, v: i32) -> u32 {
    let u = (v as i64 - min).cast_unsigned();
    let n = 1_u64 << LEVELS;
    let span = span.unsigned_abs();
    let i = ((u + 1) * n - 1) / span;
    if i >= n { (n - 1) as u32 } else { i as u32 }
}

/// A point inside the domain as `(x, y)`, with the two edges read as described in the module
/// documentation.
fn normalise(at: Point<E7>) -> (i32, i32) {
    let x = if i64::from(at.lon()) >= X_MIN + X_SPAN {
        // Longitude 180°: the antimeridian, read from the west.
        -1_800_000_000
    } else {
        at.lon()
    };
    let y = if i64::from(at.lat()) >= Y_MIN + Y_SPAN {
        899_999_999
    } else {
        at.lat()
    };
    (x, y)
}

/// Candidate `k` for a cell's reference point: near the centre, off the round numbers borders
/// follow, strictly inside the cell. Leaves are at least 107 units on a side, so the candidate
/// is always inside.
#[allow(
    clippy::cast_possible_truncation,
    reason = "below the half width, so inside the cell"
)]
const fn reference(c: Cell, k: u32) -> (i32, i32) {
    let (w, h) = (c.x1 as i64 - c.x0 as i64, c.y1 as i64 - c.y0 as i64);
    let (hx, hy) = (max1(w / 2), max1(h / 2));
    let (cx, cy) = (c.x0 as i64 + w / 2, c.y0 as i64 + h / 2);
    let k = k as i64;
    (
        (cx + (7 + 31 * k) % hx) as i32,
        (cy + (13 + 17 * k) % hy) as i32,
    )
}

const fn max1(v: i64) -> i64 {
    if v < 1 { 1 } else { v }
}

// ---------- exact predicates ----------

/// The sign of `(b − a) × (c − a)`.
fn orient(a: (i32, i32), b: (i32, i32), c: (i32, i32)) -> i32 {
    let d = |p: i32, q: i32| i128::from(p) - i128::from(q);
    let v = d(b.0, a.0) * d(c.1, a.1) - d(b.1, a.1) * d(c.0, a.0);
    // signum of an i128 is -1, 0 or 1.
    i32::from(v > 0) - i32::from(v < 0)
}

/// The sign of `(p' − r) × (v − r)` for `p' = p + (ε, ε²)`: never 0 unless `v = r`.
fn side(r: (i32, i32), p: (i32, i32), v: (i32, i32)) -> i32 {
    let s = orient(r, p, v);
    if s != 0 {
        return s;
    }
    // (p' − r) × (v − r) = s + ε (v_y − r_y) − ε² (v_x − r_x).
    let t = v.1.cmp(&r.1) as i32;
    if t != 0 {
        return t;
    }
    -(v.0.cmp(&r.0) as i32)
}

/// The sign of `(b − a) × (p' − a)` for `p' = p + (ε, ε²)`: never 0 for an edge with `a ≠ b`.
fn side_of_edge(a: (i32, i32), b: (i32, i32), p: (i32, i32)) -> i32 {
    let s = orient(a, b, p);
    if s != 0 {
        return s;
    }
    // (b − a) × (p' − a) = s − ε (b_y − a_y) + ε² (b_x − a_x).
    let t = -(b.1.cmp(&a.1) as i32);
    if t != 0 {
        return t;
    }
    b.0.cmp(&a.0) as i32
}

/// Whether the segment from `r` (on no edge's line) to `p + (ε, ε²)` crosses edge `a`–`b`.
fn crosses(r: (i32, i32), p: (i32, i32), a: (i32, i32), b: (i32, i32)) -> bool {
    // Boxes first: two integer compares per axis reject most edges.
    let (lo_x, hi_x) = (r.0.min(p.0), r.0.max(p.0));
    let (lo_y, hi_y) = (r.1.min(p.1), r.1.max(p.1));
    if a.0.max(b.0) < lo_x || a.0.min(b.0) > hi_x || a.1.max(b.1) < lo_y || a.1.min(b.1) > hi_y {
        return false;
    }
    side(r, p, a) != side(r, p, b) && orient(a, b, r) != side_of_edge(a, b, p)
}

/// Whether edge `a`–`b` touches the closed cell, exactly (separating axes).
fn touches(c: Cell, a: (i32, i32), b: (i32, i32)) -> bool {
    if a.0.max(b.0) < c.x0 || a.0.min(b.0) > c.x1 || a.1.max(b.1) < c.y0 || a.1.min(b.1) > c.y1 {
        return false;
    }
    let s = [(c.x0, c.y0), (c.x1, c.y0), (c.x0, c.y1), (c.x1, c.y1)].map(|p| orient(a, b, p));
    !(s.iter().all(|&v| v > 0) || s.iter().all(|&v| v < 0))
}

// ---------- region sets ----------

/// A region set being toggled, in a fixed buffer.
struct Acc {
    zones: [u32; MAX_SET],
    len: usize,
}

impl Acc {
    const fn new() -> Self {
        Self {
            zones: [0; MAX_SET],
            len: 0,
        }
    }

    /// Adds `z` if absent, removes it if present. Beyond [`MAX_SET`] (a corrupt pack) a new
    /// region is dropped.
    fn toggle(&mut self, z: u32) {
        let live = self.zones.get(..self.len).unwrap_or_default();
        if let Some(at) = live.iter().position(|&v| v == z) {
            self.len -= 1;
            self.zones.swap(at, self.len);
        } else if let Some(slot) = self.zones.get_mut(self.len) {
            *slot = z;
            self.len += 1;
        }
    }

    /// Writes the set to `out` in increasing order; returns its size.
    fn write(mut self, out: &mut [u32]) -> usize {
        let live = self.zones.get_mut(..self.len).unwrap_or_default();
        live.sort_unstable();
        for (o, z) in out.iter_mut().zip(live.iter()) {
            *o = *z;
        }
        self.len
    }
}

// ---------- the reader ----------

/// Edges of one leaf kept by [`Region::check`].
const CACHED_EDGES: usize = 64;

struct EdgeCache {
    leaf: usize,
    /// `usize::MAX` when the leaf has more edges than fit (or a vertex is missing).
    len: usize,
    edges: [((i32, i32), (i32, i32)); CACHED_EDGES],
}

/// A validated REGION section, read in place.
#[derive(Clone, Copy, Debug)]
pub struct Region<'a> {
    regions: usize,
    top: u32,
    leaves: usize,
    dir: Packed<'a>,
    key: Packed<'a>,
    level: Packed<'a>,
    zones: Packed<'a>,
    nudge: Option<Packed<'a>>,
    runs: Packed<'a>,
    run_edge: Packed<'a>,
    run_len: Packed<'a>,
    run_zones: Packed<'a>,
    x: Packed<'a>,
    y: Packed<'a>,
    top_zones: Packed<'a>,
    set_start: Packed<'a>,
    set_zones: Packed<'a>,
    place: Option<Packed<'a>>,
}

impl<'a> Region<'a> {
    /// Opens a REGION section, checking its structure (O(columns + blocks), no per-row work).
    ///
    /// # Errors
    /// [`Error::Payload`] if a column is missing, unknown, of the wrong length, or the top level
    /// is out of range.
    pub fn open(section: &Section<'a>) -> Result<Self, Error> {
        let table = columns::open(section, Kind::REGION)?;
        let req = |id, rows| columns::required(&table, Kind::REGION, id, rows);
        let top = req(TOP, Some(1))?
            .get(0)
            .filter(|g| (MIN_TOP..=MAX_TOP).contains(g))
            .ok_or_else(|| fail(TOP, "the top level is out of range"))?;
        let cells = 1_usize << (2 * top);
        let dir = req(DIR, Some(cells + 1))?;
        let leaves = dir
            .get(cells)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| fail(DIR, "the directory has no leaf count"))?;
        let key = req(KEY, Some(leaves))?;
        let level = req(LEVEL, Some(leaves))?;
        let zones = req(ZONES, Some(leaves))?;
        let nudge = columns::optional(&table, Kind::REGION, NUDGE, leaves)?;
        let runs = req(RUNS, Some(leaves + 1))?;
        let run_count = runs
            .get(leaves)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| fail(RUNS, "the runs have no count"))?;
        let run_edge = req(RUN_EDGE, Some(run_count))?;
        let run_len = req(RUN_LEN, Some(run_count))?;
        let run_zones = req(RUN_ZONES, Some(run_count))?;
        let x = req(X, None)?;
        let y = req(Y, Some(x.len()))?;
        let top_zones = req(TOP_ZONES, Some(cells))?;
        let set_start = req(SET_START, None)?;
        let entries = set_start
            .len()
            .checked_sub(1)
            .and_then(|last| set_start.get(last))
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| fail(SET_START, "the sets have no count"))?;
        let set_zones = req(SET_ZONES, Some(entries))?;
        if table.ids().any(|id| id == 0 || id > PLACE) {
            return Err(fail(0, "an unknown column"));
        }
        let regions =
            usize::try_from(section.item_count()).map_err(|_| fail(0, "too many regions"))?;
        let place = columns::optional(&table, Kind::REGION, PLACE, regions)?;
        Ok(Self {
            regions,
            top,
            leaves,
            dir,
            key,
            level,
            zones,
            nudge,
            runs,
            run_edge,
            run_len,
            run_zones,
            x,
            y,
            top_zones,
            set_start,
            set_zones,
            place,
        })
    }

    /// The number of regions.
    #[must_use]
    pub const fn regions(&self) -> usize {
        self.regions
    }

    /// The PLACES row of region `region`'s place, when the section links them ([`PLACE`]).
    #[must_use]
    pub fn place(&self, region: u32) -> Option<u32> {
        self.place.as_ref()?.get(region as usize)
    }

    /// The number of leaves.
    #[must_use]
    pub const fn leaves(&self) -> usize {
        self.leaves
    }

    /// The number of edge runs.
    #[must_use]
    pub const fn runs(&self) -> usize {
        self.run_edge.len()
    }

    /// The number of arc vertices.
    #[must_use]
    pub const fn vertices(&self) -> usize {
        self.x.len()
    }

    /// The top-grid level.
    #[must_use]
    pub const fn top_level(&self) -> u32 {
        self.top
    }

    /// The region with the lowest ID (the highest precedence) holding `at`, if any.
    #[must_use]
    pub fn region(&self, at: Point<E7>) -> Option<u32> {
        let mut out = [0];
        (self.regions_at(at, &mut out) > 0).then_some(out[0])
    }

    /// Writes the regions holding `at` to `out` in increasing ID (precedence) order and returns
    /// how many there are, which may exceed `out.len()`.
    pub fn regions_at(&self, at: Point<E7>, out: &mut [u32]) -> usize {
        let p = normalise(at);
        let key = morton(index(X_MIN, X_SPAN, p.0), index(Y_MIN, Y_SPAN, p.1));
        let mut acc = Acc::new();
        // A top cell inside one leaf without edges: one read.
        let cell_id = usize::try_from(key >> (2 * (LEVELS - self.top))).unwrap_or(0);
        if let Some(z) = self.top_zones.get(cell_id).and_then(|v| v.checked_sub(1)) {
            self.add_set(z, &mut acc);
            return acc.write(out);
        }
        let leaf = self.leaf(key);
        self.add_set(self.zones.get(leaf).unwrap_or(u32::MAX), &mut acc);
        let (first, last) = (self.runs.get(leaf), self.runs.get(leaf + 1));
        let (Some(first), Some(last)) = (first, last) else {
            return acc.write(out);
        };
        if first < last {
            let level = self.level.get(leaf).unwrap_or(LEVELS).min(LEVELS);
            let mask = !((1_u64 << (2 * (LEVELS - level))) - 1);
            let k = self.nudge.and_then(|n| n.get(leaf)).unwrap_or(0);
            let r = reference(cell(key & mask, level), k);
            let mut buf = ([0_u32; CHUNK], [0_u32; CHUNK]);
            for run in first..last {
                if self.run_crossings(run as usize, r, p, &mut buf) & 1 == 1 {
                    self.add_set(
                        self.run_zones.get(run as usize).unwrap_or(u32::MAX),
                        &mut acc,
                    );
                }
            }
        }
        acc.write(out)
    }

    /// The leaf holding Morton key `key`: the last leaf starting at or before it.
    fn leaf(&self, key: u64) -> usize {
        let shift = 2 * (LEVELS - self.top);
        let c = usize::try_from(key >> shift).unwrap_or(0);
        let (lo, hi) = (self.dir.get(c), self.dir.get(c + 1));
        let (Some(lo), Some(hi)) = (lo, hi) else {
            return 0;
        };
        // Low `shift` bits: below 2^32 since the top level is at least MIN_TOP.
        let rel = u32::try_from(key & ((1_u64 << shift) - 1)).unwrap_or(u32::MAX);
        // The first owned leaf starting after `rel`, by binary search.
        let (mut a, mut b) = (lo as usize, (hi as usize).min(self.leaves));
        while a < b {
            let m = a + (b - a) / 2;
            if self.key.get(m).is_some_and(|k| k <= rel) {
                a = m + 1;
            } else {
                b = m;
            }
        }
        a.saturating_sub(1)
    }

    /// Toggles every member of set `id` into `acc`.
    fn add_set(&self, id: u32, acc: &mut Acc) {
        if (id as usize) < self.regions {
            acc.toggle(id);
            return;
        }
        let id = id as usize;
        let (Some(s), Some(e)) = (self.set_start.get(id), self.set_start.get(id + 1)) else {
            return;
        };
        for j in s..e.min(s.saturating_add(SET_CAP)) {
            if let Some(z) = self.set_zones.get(j as usize) {
                acc.toggle(z);
            }
        }
    }

    /// How many edges of `run` the segment from `r` to `p` crosses.
    fn run_crossings(
        &self,
        run: usize,
        r: (i32, i32),
        p: (i32, i32),
        (xs, ys): &mut ([u32; CHUNK], [u32; CHUNK]),
    ) -> u32 {
        let (Some(first), Some(len)) = (self.run_edge.get(run), self.run_len.get(run)) else {
            return 0;
        };
        let (mut at, end) = (first as usize, first as usize + len as usize);
        let mut count = 0;
        // Each chunk shares its first vertex with the previous chunk's last.
        while at < end {
            let want = (end - at + 1).min(CHUNK);
            let (Some(xw), Some(yw)) = (xs.get_mut(..want), ys.get_mut(..want)) else {
                break;
            };
            let n = self.x.run(at, xw).min(self.y.run(at, yw));
            if n < 2 {
                break;
            }
            let mut prev: Option<(i32, i32)> = None;
            for (&x, &y) in xw.iter().zip(yw.iter()).take(n) {
                let v = (unbias(x), unbias(y));
                if let Some(a) = prev
                    && a != v
                    && crosses(r, p, a, v)
                {
                    count += 1;
                }
                prev = Some(v);
            }
            at += n - 1;
        }
        count
    }

    /// Checks everything [`Region::open`] does not, in one pass over each column: the directory
    /// and the keys partition the world in Morton order; runs, sets and region IDs are in
    /// range; sets strictly increase; no reference point lies on the line of one of its leaf's
    /// edges. It does not re-derive the region sets from the geometry.
    ///
    /// # Errors
    /// [`Error::Payload`] naming the first column found inconsistent.
    pub fn check(&self) -> Result<(), Error> {
        self.check_sets()?;
        self.check_leaves()?;
        self.check_runs()
    }

    fn check_sets(&self) -> Result<(), Error> {
        let sets = self.set_start.len() - 1;
        for s in 0..sets {
            let (a, b) = (self.set_start.get(s), self.set_start.get(s + 1));
            let (Some(a), Some(b)) = (a, b) else {
                return Err(fail(SET_START, "a set start is missing"));
            };
            if b < a || (b - a) as usize > MAX_SET {
                return Err(fail(
                    SET_START,
                    "the set starts decrease or a set is too large",
                ));
            }
            if s < self.regions
                && (b - a != 1 || self.set_zones.get(a as usize) != u32::try_from(s).ok())
            {
                return Err(fail(SET_ZONES, "set z is not the single region z"));
            }
            let mut last: Option<u32> = None;
            for j in a..b {
                let z = self.set_zones.get(j as usize).unwrap_or(u32::MAX);
                if z as usize >= self.regions || last.is_some_and(|l| l >= z) {
                    return Err(fail(SET_ZONES, "a set is not increasing region IDs"));
                }
                last = Some(z);
            }
        }
        if self.set_start.get(0) != Some(0) && sets > 0 {
            return Err(fail(SET_START, "the first set does not start at 0"));
        }
        Ok(())
    }

    fn check_leaves(&self) -> Result<(), Error> {
        let sets = self.set_start.len() - 1;
        let shift = 2 * (LEVELS - self.top);
        let cells = 1_usize << (2 * self.top);
        let mut expect: u64 = 0;
        let mut leaf = 0;
        // Neighbouring top cells share their holding leaf: its edges are decoded once.
        let mut cache = EdgeCache {
            leaf: usize::MAX,
            len: 0,
            edges: [((0, 0), (0, 0)); CACHED_EDGES],
        };
        for c in 0..cells {
            let (Some(lo), Some(hi)) = (self.dir.get(c), self.dir.get(c + 1)) else {
                return Err(fail(DIR, "a directory entry is missing"));
            };
            if lo as usize != leaf || hi < lo {
                return Err(fail(
                    DIR,
                    "the directory does not count the leaves in order",
                ));
            }
            let base = (c as u64) << shift;
            // A fast answer needs a holding leaf at least the cell's size whose edges all
            // miss the cell (its set is not re-derived, like every set).
            if self.top_zones.get(c).is_some_and(|v| v != 0) {
                let holder = if lo < hi && self.key.get(lo as usize) == Some(0) {
                    Some(lo as usize)
                } else {
                    (lo as usize).checked_sub(1)
                };
                let ok = holder.is_some_and(|h| {
                    self.level.get(h).is_some_and(|l| l <= self.top)
                        && self.edges_miss(h, cell(base, self.top), &mut cache)
                });
                if !ok || self.top_zones.get(c).is_none_or(|v| v as usize > sets) {
                    return Err(fail(TOP_ZONES, "a top cell's fast answer has edges in it"));
                }
            }
            for i in lo as usize..hi as usize {
                let rel = self
                    .key
                    .get(i)
                    .ok_or_else(|| fail(KEY, "a key is missing"))?;
                let level = self.level.get(i).unwrap_or(u32::MAX);
                if level > LEVELS {
                    return Err(fail(LEVEL, "a level is too deep"));
                }
                let k = base + u64::from(rel);
                let size = 1_u64 << (2 * (LEVELS - level));
                if u64::from(rel) >> shift != 0 || k != expect || !k.is_multiple_of(size) {
                    return Err(fail(
                        KEY,
                        "the leaves do not tile the world in Morton order",
                    ));
                }
                if self.zones.get(i).is_none_or(|z| z as usize >= sets) {
                    return Err(fail(ZONES, "a leaf's set is out of range"));
                }
                expect = k + size;
            }
            leaf = hi as usize;
        }
        // Leaves coarser than the top grid start in an earlier top cell, so the directory
        // counts every leaf; the last must end the world.
        if leaf != self.leaves || expect != 1_u64 << (2 * LEVELS) {
            return Err(fail(KEY, "the leaves do not cover the world"));
        }
        Ok(())
    }

    /// [`Region::runs_miss`] through `cache`, which holds leaf `h`'s edges when they fit.
    fn edges_miss(&self, h: usize, c: Cell, cache: &mut EdgeCache) -> bool {
        if cache.leaf != h {
            cache.leaf = h;
            cache.len = usize::MAX;
            let (Some(a), Some(b)) = (self.runs.get(h), self.runs.get(h + 1)) else {
                return false;
            };
            let at = |i: usize| Some((unbias(self.x.get(i)?), unbias(self.y.get(i)?)));
            let mut n = 0;
            'fill: for run in a..b {
                let (Some(e), Some(len)) = (
                    self.run_edge.get(run as usize),
                    self.run_len.get(run as usize),
                ) else {
                    return false;
                };
                for i in e as usize..e as usize + len as usize {
                    let (Some(p), Some(q), Some(slot)) = (at(i), at(i + 1), cache.edges.get_mut(n))
                    else {
                        n = usize::MAX;
                        break 'fill;
                    };
                    *slot = (p, q);
                    n += 1;
                }
            }
            cache.len = n;
        }
        match cache.edges.get(..cache.len) {
            Some(edges) => edges.iter().all(|&(p, q)| !touches(c, p, q)),
            None => self.runs_miss(h, c),
        }
    }

    /// Whether every edge of leaf `h` misses the closed cell `c`.
    fn runs_miss(&self, h: usize, c: Cell) -> bool {
        let (Some(a), Some(b)) = (self.runs.get(h), self.runs.get(h + 1)) else {
            return false;
        };
        let at = |i: usize| Some((unbias(self.x.get(i)?), unbias(self.y.get(i)?)));
        (a..b).all(|run| {
            let (Some(e), Some(n)) = (
                self.run_edge.get(run as usize),
                self.run_len.get(run as usize),
            ) else {
                return false;
            };
            (e as usize..e as usize + n as usize).all(|i| match (at(i), at(i + 1)) {
                (Some(p), Some(q)) => !touches(c, p, q),
                _ => false,
            })
        })
    }

    fn check_runs(&self) -> Result<(), Error> {
        let sets = self.set_start.len() - 1;
        let vertices = self.x.len();
        let mut key: u64 = 0;
        for leaf in 0..self.leaves {
            let level = self.level.get(leaf).unwrap_or(LEVELS).min(LEVELS);
            let size = 1_u64 << (2 * (LEVELS - level));
            let (Some(a), Some(b)) = (self.runs.get(leaf), self.runs.get(leaf + 1)) else {
                return Err(fail(RUNS, "a run start is missing"));
            };
            if b < a {
                return Err(fail(RUNS, "the run starts do not increase"));
            }
            let k = self.nudge.and_then(|n| n.get(leaf)).unwrap_or(0);
            if a == b && k != 0 {
                return Err(fail(NUDGE, "a leaf without edges has a reference point"));
            }
            if k >= NUDGES {
                return Err(fail(NUDGE, "a reference point is past the candidates"));
            }
            let c = cell(key, level);
            let r = reference(c, k);
            // Runs in a leaf increase and do not touch: the writer's runs are maximal.
            let mut after = 0;
            for run in a as usize..b as usize {
                let (Some(e), Some(n)) = (self.run_edge.get(run), self.run_len.get(run)) else {
                    return Err(fail(RUN_EDGE, "a run is missing"));
                };
                if n == 0 || e as usize + n as usize >= vertices {
                    return Err(fail(RUN_LEN, "a run is empty or ends past the vertices"));
                }
                if run > a as usize && e <= after {
                    return Err(fail(RUN_EDGE, "a leaf's runs overlap or are out of order"));
                }
                after = e + n;
                if self.run_zones.get(run).is_none_or(|z| z as usize >= sets) {
                    return Err(fail(RUN_ZONES, "a run's set is out of range"));
                }
                self.check_edges(c, r, e as usize, n as usize)?;
            }
            key += size;
        }
        Ok(())
    }

    /// Every edge of the run touches the leaf's cell and has the reference point off its line.
    fn check_edges(&self, c: Cell, r: (i32, i32), first: usize, len: usize) -> Result<(), Error> {
        let at = |i: usize| -> Option<(i32, i32)> {
            Some((unbias(self.x.get(i)?), unbias(self.y.get(i)?)))
        };
        for i in first..first + len {
            let (Some(a), Some(b)) = (at(i), at(i + 1)) else {
                return Err(fail(X, "a vertex is missing"));
            };
            if a == b || !touches(c, a, b) {
                return Err(fail(RUN_EDGE, "a run has an edge outside its leaf"));
            }
            if orient(a, b, r) == 0 {
                return Err(fail(NUDGE, "a reference point is on the line of an edge"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Cell, LEVELS, X_MIN, X_SPAN, Y_MIN, Y_SPAN, cell, compact, crosses, edge, index, morton,
        reference, spread,
    };

    #[test]
    fn morton_round_trips() {
        for v in [0, 1, 2, 3, 0x00ab_cdef, 0x00ff_ffff, 0x0055_5555] {
            assert_eq!(compact(spread(v)), v);
            assert_eq!(spread(v) & !0x5555_5555_5555_5555, 0);
        }
        assert_eq!(morton(1, 0), 1);
        assert_eq!(morton(0, 1), 2);
        assert_eq!(morton(0x00ff_ffff, 0x00ff_ffff), (1 << 48) - 1);
    }

    #[test]
    fn cells_nest_and_index_finds_them() {
        // Children of a cell tile it exactly.
        for level in [0, 1, 5, 17, 23] {
            for key in [0_u64, 1 << 20, (1 << 48) - (1 << (2 * (LEVELS - level)))] {
                let key = key & !((1_u64 << (2 * (LEVELS - level))) - 1);
                let c = cell(key, level);
                let step = 1_u64 << (2 * (LEVELS - level - 1));
                let k: [Cell; 4] = core::array::from_fn(|q| cell(key + q as u64 * step, level + 1));
                assert_eq!(
                    (k[0].x0, k[0].y0, k[3].x1, k[3].y1),
                    (c.x0, c.y0, c.x1, c.y1)
                );
                assert_eq!((k[0].x1, k[0].y1), (k[1].x0, k[2].y0));
            }
        }
        // `index` is the last cell edge at or before the value.
        for v in [
            -1_800_000_000,
            -1_799_999_999,
            -1,
            0,
            1,
            123_456_789,
            1_799_999_999,
        ] {
            let i = index(X_MIN, X_SPAN, v);
            assert!(edge(X_MIN, X_SPAN, u64::from(i), LEVELS) <= v);
            assert!(edge(X_MIN, X_SPAN, u64::from(i) + 1, LEVELS) > v);
        }
        for v in [-900_000_000, -1, 0, 899_999_999] {
            let i = index(Y_MIN, Y_SPAN, v);
            assert!(edge(Y_MIN, Y_SPAN, u64::from(i), LEVELS) <= v);
            assert!(edge(Y_MIN, Y_SPAN, u64::from(i) + 1, LEVELS) > v);
        }
        // `index` at the cell edges themselves, pinned.
        assert_eq!(index(X_MIN, X_SPAN, -1_800_000_000), 0);
        assert_eq!(index(X_MIN, X_SPAN, 1_800_000_000), (1 << LEVELS) - 1);
        assert_eq!(index(Y_MIN, Y_SPAN, 0), 1 << (LEVELS - 1));
        // The reference candidates are part of the format: pinned values.
        let root = cell(0, 0);
        assert_eq!(reference(root, 0), (7, 13));
        assert_eq!(reference(root, 3), (100, 64));
        let small = cell(0, LEVELS);
        assert_eq!(reference(small, 0), (-1_799_999_886, -899_999_934));
        assert_eq!(reference(small, 5), (-1_799_999_838, -899_999_902));
        // Every reference candidate of the smallest cell is strictly inside it.
        let c = cell(0, LEVELS);
        for k in 0..4096 {
            let (x, y) = reference(c, k);
            assert!(c.x0 < x && x < c.x1 && c.y0 < y && y < c.y1, "{k}");
        }
    }

    #[test]
    fn crossings_follow_the_perturbation() {
        let r = (0, 0);
        // A vertical edge through the query point: p + ε is east of it, so a segment from the
        // west crosses it.
        assert!(crosses((-10, 1), (0, 0), (0, -5), (0, 5)));
        assert!(!crosses((10, 1), (0, 0), (0, -5), (0, 5)));
        // A horizontal edge through p: p + ε² is north of it.
        assert!(crosses((1, -10), (0, 0), (-5, 0), (5, 0)));
        assert!(!crosses((1, 10), (0, 0), (-5, 0), (5, 0)));
        // A vertex exactly on the segment counts once for the two edges meeting there.
        let (a, v, b) = ((-5, 5), (0, 5), (5, 5));
        let n = u32::from(crosses(r, (0, 10), a, v)) + u32::from(crosses(r, (0, 10), v, b));
        assert_eq!(n, 1);
        // ... and not at all for a spike touching it.
        let (a, b) = ((-5, 10), (-5, 0));
        let n =
            u32::from(crosses(r, (0, 10), a, (0, 5))) + u32::from(crosses(r, (0, 10), (0, 5), b));
        assert_eq!(n % 2, 0);
    }
}
