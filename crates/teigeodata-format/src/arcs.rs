//! ARCS (kind 7): the shared borders of every polygon, each stored once (D-088, B10).
//!
//! An **arc** is a run of at least two vertices on the 1e-7° grid ([ADR-0029]) between two
//! topology nodes, or a closed ring with no node. Polygons ([POLYGONS](crate::Kind::POLYGONS))
//! list their rings as arcs, so neighbours share one copy of their border and stay coincident at
//! every level of detail. The section's item count is the number of arcs.
//!
//! Vertices are numbered across all arcs: arc `a` holds vertices `START[a]` to `START[a+1] − 1`.
//! Coordinates are zigzag deltas from the previous vertex of the same arc, 0 at an arc's first
//! vertex, whose absolute value is the arc's base; a delta of the arc's vertex count would
//! otherwise set the bit width of its whole block (the [geometry note] §3.1 measured 3.98 B per
//! vertex against 4.43 for deltas across arcs). Each 128-vertex block also stores the absolute
//! point of its first vertex (a checkpoint), so any vertex is one block decode away and an arc
//! reads backwards as cheaply as forwards.
//!
//! | ID | Column | Values | |
//! |---:|---|---|---|
//! | 1 | [`START`] | arcs + 1 values: each arc's first vertex, then the vertex count | required |
//! | 2 | [`BASE_X`] | per arc: longitude of its first vertex, [biased](teigeodata_core::bitpack::bias) | required |
//! | 3 | [`BASE_Y`] | per arc: latitude, biased | required |
//! | 4 | [`DX`] | per vertex: [zigzag](teigeodata_core::bitpack::zigzag)(Δx ÷ 10^scale), 0 at an arc's first vertex | required |
//! | 5 | [`DY`] | per vertex: the same for y | required |
//! | 6 | [`CHK_X`] | per 128-vertex block: longitude of its first vertex, biased | required |
//! | 7 | [`CHK_Y`] | per block: latitude, biased | required |
//! | 8 | [`LEVEL`] | per vertex: the coarsest level of detail that keeps it, 0 to [`PINNED`] | optional, absent = full resolution only |
//! | 9 | [`TOLERANCE`] | 6 values: the Douglas–Peucker tolerance of levels 1 to 6, centimetres, increasing | with `LEVEL` |
//! | 10 | [`SCALE`] | per arc: `k`, its deltas are stored ÷ 10^k (a source on a coarser grid) | optional, absent = all 0 |
//! | 11 | [`FROM`] | one value: the overview's level `f`, 1 to 6 | `ARCS_OVERVIEW` only, required |
//!
//! An **overview** ([`Kind::ARCS_OVERVIEW`], D-102) has ARCS's layout and holds, for every arc
//! of ARCS in the same order, only the vertices of level `f` and coarser with their levels. A
//! read at level `f` or coarser decodes those alone: a coarse read of ARCS decodes every
//! vertex, since deltas are cumulative. [`Arcs::check_overview`] checks it against ARCS;
//! [`overview`] writes it.
//!
//! Level of detail `l` keeps the vertices whose level is at least `l`: level 0 is every vertex,
//! level `l` in 1 to 6 keeps every vertex the simplifier needs to stay within the tolerance of
//! `l`, and [`PINNED`] marks the vertices kept at every level (an arc's ends, a closed arc's
//! anchor). A closed arc (first vertex = last) has at least 4 vertices, and with `LEVEL` at least
//! one pinned vertex inside it, so no ring falls below a triangle.
//!
//! [`Arcs::open`] checks the frame and every column's length in O(columns + blocks);
//! [`Arcs::check`] checks every value in O(vertices). Reads never panic and never allocate.
//!
//! [ADR-0029]: https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/adr/ADR-0029-coordinate-grid.md
//! [geometry note]: https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/geometry-topology.md

use core::ops::Range;

use teigeodata_core::bitpack::{BLOCK_LEN, Packed, unbias, unzigzag};
use teigeodata_core::table::Table;
use teigeodata_core::{E7, Point};

use crate::{Error, Kind, Section, columns};

/// Column: each arc's first vertex.
pub const START: u16 = 1;
/// Column (overview only): the level the overview starts at.
pub const FROM: u16 = 11;
/// Column: each arc's first longitude.
pub const BASE_X: u16 = 2;
/// Column: each arc's first latitude.
pub const BASE_Y: u16 = 3;
/// Column: longitude deltas.
pub const DX: u16 = 4;
/// Column: latitude deltas.
pub const DY: u16 = 5;
/// Column: each block's first longitude.
pub const CHK_X: u16 = 6;
/// Column: each block's first latitude.
pub const CHK_Y: u16 = 7;
/// Column: each vertex's level of detail.
pub const LEVEL: u16 = 8;
/// Column: the tolerance of each level.
pub const TOLERANCE: u16 = 9;
/// Column: each arc's decimal scale.
pub const SCALE: u16 = 10;

/// The level of the vertices kept at every level of detail.
pub const PINNED: u8 = 7;
/// The number of stored tolerances (levels 1 to 6).
pub const TOLERANCES: usize = 6;
/// The largest decimal scale of an arc.
pub const MAX_SCALE: u32 = 7;

const POW10: [i32; 8] = [1, 10, 100, 1_000, 10_000, 100_000, 1_000_000, 10_000_000];

/// 10^k, for a scale `k` up to [`MAX_SCALE`] (1 above it, which [`Arcs::check`] refuses).
pub(crate) fn pow10(k: u32) -> i32 {
    POW10.get(k as usize).copied().unwrap_or(1)
}
const X_MAX: i64 = 1_800_000_000;
const Y_MAX: i64 = 900_000_000;

fn fail(column: u16, problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::ARCS.0,
        column,
        problem,
    }
}

/// The ARCS section of an open pack.
#[derive(Clone, Copy, Debug)]
pub struct Arcs<'a> {
    start: Packed<'a>,
    base_x: Packed<'a>,
    base_y: Packed<'a>,
    dx: Packed<'a>,
    dy: Packed<'a>,
    chk_x: Packed<'a>,
    chk_y: Packed<'a>,
    level: Option<Packed<'a>>,
    tolerance: Option<Packed<'a>>,
    scale: Option<Packed<'a>>,
    /// An overview's level ([`FROM`]).
    from: Option<u8>,
}

impl<'a> Arcs<'a> {
    /// Opens the section: its frame, every column's length, and that `START` begins at 0 and
    /// ends at the vertex count. O(columns + blocks).
    ///
    /// # Errors
    /// [`Error::Payload`] naming the column.
    pub fn open(section: &Section<'a>) -> Result<Self, Error> {
        let overview = section.kind() == Kind::ARCS_OVERVIEW;
        let kind = if overview {
            Kind::ARCS_OVERVIEW
        } else {
            Kind::ARCS
        };
        let table: Table<'a> = columns::open(section, kind)?;
        let arcs = usize::try_from(section.item_count())
            .ok()
            .filter(|&n| n < u32::MAX as usize)
            .ok_or_else(|| fail(0, "the arc count does not fit"))?;
        let k = kind;
        let start = columns::required(&table, k, START, Some(arcs + 1))?;
        let base_x = columns::required(&table, k, BASE_X, Some(arcs))?;
        let base_y = columns::required(&table, k, BASE_Y, Some(arcs))?;
        let dx = columns::required(&table, k, DX, None)?;
        let v = dx.len();
        let dy = columns::required(&table, k, DY, Some(v))?;
        let blocks = v.div_ceil(BLOCK_LEN);
        let chk_x = columns::required(&table, k, CHK_X, Some(blocks))?;
        let chk_y = columns::required(&table, k, CHK_Y, Some(blocks))?;
        let level = columns::optional(&table, k, LEVEL, v)?;
        let tolerance = columns::optional(&table, k, TOLERANCE, TOLERANCES)?;
        let scale = columns::optional(&table, k, SCALE, arcs)?;
        if level.is_some() != tolerance.is_some() {
            return Err(fail(LEVEL, "LEVEL and TOLERANCE come together"));
        }
        if table
            .ids()
            .any(|id| id == 0 || id > SCALE && !(overview && id == FROM))
        {
            return Err(fail(0, "an unknown column"));
        }
        let from = if overview {
            let f = columns::required(&table, k, FROM, Some(1))?
                .get(0)
                .and_then(|f| u8::try_from(f).ok())
                .filter(|f| (1..PINNED).contains(f))
                .ok_or_else(|| fail(FROM, "an overview's level is 1 to 6"))?;
            if level.is_none() {
                return Err(fail(LEVEL, "an overview has levels"));
            }
            Some(f)
        } else {
            None
        };
        if start.get(0) != Some(0) || start.get(arcs).map(|e| e as usize) != Some(v) {
            return Err(fail(
                START,
                "START must begin at 0 and end at the vertex count",
            ));
        }
        Ok(Self {
            start,
            base_x,
            base_y,
            dx,
            dy,
            chk_x,
            chk_y,
            level,
            tolerance,
            scale,
            from,
        })
    }

    /// An overview's level: it serves reads at this level of detail and coarser. `None` for
    /// ARCS.
    #[must_use]
    pub const fn overview_from(&self) -> Option<u8> {
        self.from
    }

    /// Checks that this overview holds, for every arc of `full` (an ARCS), exactly its vertices
    /// of the overview's level and coarser, with their levels and the same tolerances. O(the
    /// vertices of both); run [`Arcs::check`] on each first.
    ///
    /// # Errors
    /// [`Error::Payload`] naming [`FROM`] if this is not an overview or `full` is one, or it
    /// differs from `full` at its level.
    pub fn check_overview(&self, full: &Arcs<'_>) -> Result<(), Error> {
        let (Some(from), None) = (self.from, full.from) else {
            return Err(fail(FROM, "not an overview of an ARCS"));
        };
        let differs = || fail(FROM, "the overview is not the arcs at its level");
        if self.len() != full.len()
            || (1..PINNED).any(|l| self.tolerance_cm(l) != full.tolerance_cm(l))
        {
            return Err(differs());
        }
        for a in 0..full.len() {
            let (Some(fr), Some(or)) = (full.range(a), self.range(a)) else {
                return Err(differs());
            };
            let kept = fr
                .zip(full.points(a, false, 0))
                .filter_map(|(v, p)| full.level(v).filter(|&l| l >= from).map(|l| (p, l)));
            let mine = or
                .clone()
                .zip(self.points(a, false, 0))
                .map(|(v, p)| (p, self.level(v).unwrap_or(0)));
            if !kept.eq(mine) || or.len() != self.points(a, false, 0).count() {
                return Err(differs());
            }
        }
        Ok(())
    }

    /// The number of arcs.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.base_x.len()
    }

    /// Whether there are none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.base_x.is_empty()
    }

    /// The number of vertices of all arcs.
    #[must_use]
    pub const fn vertices(&self) -> usize {
        self.dx.len()
    }

    /// The level of detail of vertex `v` (numbered across all arcs): the coarsest level that
    /// keeps it, 0 when the section has no levels, `None` past the end.
    #[must_use]
    pub fn level(&self, v: usize) -> Option<u8> {
        if v >= self.dx.len() {
            return None;
        }
        Some(
            self.level
                .and_then(|l| l.get(v))
                .map_or(0, |l| u8::try_from(l).unwrap_or(u8::MAX)),
        )
    }

    /// Whether the arcs carry levels of detail above 0.
    #[must_use]
    pub const fn has_levels(&self) -> bool {
        self.level.is_some()
    }

    /// The tolerance of level of detail `lod` in centimetres: 0 for level 0, `None` for
    /// [`PINNED`], above it, or when the section has no levels.
    #[must_use]
    pub fn tolerance_cm(&self, lod: u8) -> Option<u32> {
        match lod {
            0 => Some(0),
            l if l < PINNED => self.tolerance?.get(usize::from(l) - 1),
            _ => None,
        }
    }

    /// The vertex numbers of arc `a`; `None` past the end or if `START` is malformed there.
    #[must_use]
    pub fn range(&self, a: usize) -> Option<Range<usize>> {
        let (s, e) = (self.start.get(a)? as usize, self.start.get(a + 1)? as usize);
        (s + 2 <= e && e <= self.vertices()).then_some(s..e)
    }

    /// The decimal scale of arc `a` (0 when the section stores none).
    #[must_use]
    pub fn scale(&self, a: usize) -> u32 {
        self.scale.and_then(|c| c.get(a)).unwrap_or(0)
    }

    /// Whether some arc has a decimal scale (the `SCALE` column is present).
    #[must_use]
    pub const fn has_scale(&self) -> bool {
        self.scale.is_some()
    }

    /// The delta columns `DX` and `DY`, for readers that sum a slice of them from a known point
    /// ([REGION](crate::region) on these arcs).
    pub(crate) const fn deltas(&self) -> (Packed<'a>, Packed<'a>) {
        (self.dx, self.dy)
    }

    /// The arc holding vertex `v`: a binary search of `START`, O(log arcs). `None` past the
    /// end.
    #[must_use]
    pub fn arc_of(&self, v: usize) -> Option<usize> {
        if v >= self.vertices() {
            return None;
        }
        // The last arc whose start is at or before `v`.
        let (mut lo, mut hi) = (0, self.len());
        while lo < hi {
            let m = lo + (hi - lo) / 2;
            if self.start.get(m).is_some_and(|s| s as usize <= v) {
                lo = m + 1;
            } else {
                hi = m;
            }
        }
        lo.checked_sub(1)
    }

    /// Vertex `v` of arc `a` as `(x, y)` on the grid: from the arc's base or its block's
    /// checkpoint, whichever is later, plus at most 127 deltas. `None` if `v` is not in `a`.
    #[must_use]
    #[allow(
        clippy::many_single_char_names,
        reason = "arc a, vertex v, range r, start s, count n, point x, y"
    )]
    pub fn vertex(&self, a: usize, v: usize) -> Option<(i32, i32)> {
        let r = self.range(a)?;
        if !r.contains(&v) {
            return None;
        }
        let block = v / BLOCK_LEN;
        let s = (block * BLOCK_LEN).max(r.start);
        let (x0, y0) = if s == r.start {
            (self.base_x.get(a)?, self.base_y.get(a)?)
        } else {
            (self.chk_x.get(block)?, self.chk_y.get(block)?)
        };
        let (mut x, mut y) = (unbias(x0), unbias(y0));
        let mul = pow10(self.scale(a));
        let (mut dx, mut dy) = ([0_u32; BLOCK_LEN], [0_u32; BLOCK_LEN]);
        let n = v - s;
        let (Some(bx), Some(by)) = (dx.get_mut(..n), dy.get_mut(..n)) else {
            return None;
        };
        if self.dx.run(s + 1, bx) != n || self.dy.run(s + 1, by) != n {
            return None;
        }
        for (ddx, ddy) in bx.iter().zip(by.iter()) {
            x = x.wrapping_add(unzigzag(*ddx).cast_signed().wrapping_mul(mul));
            y = y.wrapping_add(unzigzag(*ddy).cast_signed().wrapping_mul(mul));
        }
        Some((x, y))
    }

    /// The first point of arc `a`.
    #[must_use]
    pub fn first(&self, a: usize) -> Option<Point<E7>> {
        point(unbias(self.base_x.get(a)?), unbias(self.base_y.get(a)?))
    }

    /// The vertices of arc `a` kept at level of detail `lod`, first to last, or last to first
    /// when `reversed`. Empty past the end. The iterator holds one decoded block (about 1.2 KB).
    #[must_use]
    pub fn points(&self, a: usize, reversed: bool, lod: u8) -> ArcPoints<'a> {
        let range = self.range(a).unwrap_or(0..0);
        let s = self.scale(a);
        ArcPoints {
            arcs: *self,
            arc: a,
            first: range.start,
            todo: range,
            reversed,
            lod,
            mul: pow10(s),
            xs: [0; BLOCK_LEN],
            ys: [0; BLOCK_LEN],
            lv: [PINNED; BLOCK_LEN],
            at: 0,
            end: 0,
        }
    }

    #[cfg(feature = "alloc")]
    /// The vertices `range` of arc `a`, forwards, every level. `range.start` must be the arc's
    /// first vertex or a block's first (a checkpoint); otherwise empty.
    pub(crate) fn points_in(&self, a: usize, range: Range<usize>) -> ArcPoints<'a> {
        let arc = self.range(a).unwrap_or(0..0);
        let ok = range.start >= arc.start
            && range.end <= arc.end
            && (range.start == arc.start || range.start.is_multiple_of(BLOCK_LEN));
        ArcPoints {
            arcs: *self,
            arc: a,
            first: arc.start,
            todo: if ok { range } else { 0..0 },
            reversed: false,
            lod: 0,
            mul: pow10(self.scale(a)),
            xs: [0; BLOCK_LEN],
            ys: [0; BLOCK_LEN],
            lv: [PINNED; BLOCK_LEN],
            at: 0,
            end: 0,
        }
    }

    /// How many vertices of arc `a` level of detail `lod` keeps (0 past the end). Reads only the
    /// level column.
    #[must_use]
    pub fn count(&self, a: usize, lod: u8) -> usize {
        let Some(r) = self.range(a) else { return 0 };
        match self.level {
            Some(l) if lod > 0 => l.values(r).filter(|&v| v >= u32::from(lod)).count(),
            _ => r.len(),
        }
    }

    /// Checks every value, streaming each column once: arcs of at least 2 vertices, deltas of 0
    /// at each arc's start, every point on the grid, no repeated consecutive vertex, checkpoints
    /// equal to the decoded points, levels of at most [`PINNED`] with pinned ends, closed arcs of
    /// at least 4 vertices with a pinned vertex inside, increasing positive tolerances, scales
    /// up to [`MAX_SCALE`], and no `SCALE` that is all 0. O(vertices).
    ///
    /// # Errors
    /// [`Error::Payload`] naming the column.
    #[allow(
        clippy::many_single_char_names,
        reason = "coordinates x, y and their deltas"
    )]
    pub fn check(&self) -> Result<(), Error> {
        let (n, v) = (self.len(), self.vertices());
        if let Some(t) = self.tolerance {
            let mut last = 0;
            for x in t.values(0..TOLERANCES) {
                if x <= last {
                    return Err(fail(
                        TOLERANCE,
                        "tolerances must be positive and increasing",
                    ));
                }
                last = x;
            }
        }
        if let Some(s) = self.scale {
            if s.values(0..n).any(|k| k > MAX_SCALE) {
                return Err(fail(SCALE, "a scale above 7"));
            }
            if s.values(0..n).all(|k| k == 0) {
                return Err(fail(SCALE, "an optional column that is all 0 is stored"));
            }
        }
        let mut starts = self.start.values(0..n + 1);
        let mut next = starts.next().map_or(0, |s| s as usize);
        let (mut bx, mut by) = (self.base_x.values(0..n), self.base_y.values(0..n));
        let (mut cx, mut cy) = (
            self.chk_x.values(0..v.div_ceil(BLOCK_LEN)),
            self.chk_y.values(0..v.div_ceil(BLOCK_LEN)),
        );
        let mut levels = self.level.map(|l| l.values(0..v));
        let mut arc = Walk::default();
        let mut a = 0;
        for (i, (dx, dy)) in self.dx.values(0..v).zip(self.dy.values(0..v)).enumerate() {
            let lv = levels
                .as_mut()
                .and_then(Iterator::next)
                .unwrap_or(u32::from(PINNED));
            if lv > u32::from(PINNED) {
                return Err(fail(LEVEL, "a level above 7"));
            }
            if self.from.is_some_and(|f| lv < u32::from(f)) {
                return Err(fail(LEVEL, "an overview vertex finer than its level"));
            }
            if i == next {
                if i > 0 {
                    arc.end()?;
                }
                let end = starts.next().map_or(v, |e| e as usize);
                if end < i + 2 || end > v {
                    return Err(fail(START, "an arc of fewer than 2 vertices"));
                }
                if dx != 0 || dy != 0 {
                    return Err(fail(DX, "an arc's first delta is not 0"));
                }
                let (Some(x), Some(y)) = (bx.next(), by.next()) else {
                    return Err(fail(BASE_X, "a base is missing"));
                };
                let mul = i64::from(pow10(self.scale(a)));
                arc = Walk::start(
                    i64::from(unbias(x)),
                    i64::from(unbias(y)),
                    end - i,
                    mul,
                    lv,
                    self.level.is_some(),
                )?;
                next = end;
                a += 1;
            } else {
                arc.step(
                    i64::from(unzigzag(dx).cast_signed()),
                    i64::from(unzigzag(dy).cast_signed()),
                    lv,
                )?;
            }
            if i % BLOCK_LEN == 0 {
                let (Some(x), Some(y)) = (cx.next(), cy.next()) else {
                    return Err(fail(CHK_X, "a checkpoint is missing"));
                };
                if (i64::from(unbias(x)), i64::from(unbias(y))) != (arc.x, arc.y) {
                    return Err(fail(CHK_X, "a checkpoint differs from the decoded point"));
                }
            }
        }
        if v > 0 {
            arc.end()?;
        }
        Ok(())
    }
}

/// The state of [`Arcs::check`] along one arc.
#[derive(Default)]
struct Walk {
    x: i64,
    y: i64,
    first: (i64, i64),
    mul: i64,
    left: usize,
    len: usize,
    last_level: u32,
    inner_pin: bool,
    levels: bool,
}

impl Walk {
    fn start(
        x: i64,
        y: i64,
        len: usize,
        mul: i64,
        level: u32,
        levels: bool,
    ) -> Result<Self, Error> {
        if x.abs() > X_MAX || y.abs() > Y_MAX {
            return Err(fail(BASE_X, "a base is off the grid"));
        }
        if level != u32::from(PINNED) {
            return Err(fail(LEVEL, "an arc's first vertex is not pinned"));
        }
        Ok(Self {
            x,
            y,
            first: (x, y),
            mul,
            left: len - 1,
            len,
            last_level: level,
            inner_pin: false,
            levels,
        })
    }

    fn step(&mut self, dx: i64, dy: i64, level: u32) -> Result<(), Error> {
        if dx == 0 && dy == 0 {
            return Err(fail(DX, "a repeated vertex"));
        }
        self.x += dx * self.mul;
        self.y += dy * self.mul;
        if self.x.abs() > X_MAX || self.y.abs() > Y_MAX {
            return Err(fail(DX, "a vertex is off the grid"));
        }
        self.left -= 1;
        if self.left > 0 && level == u32::from(PINNED) {
            self.inner_pin = true;
        }
        self.last_level = level;
        Ok(())
    }

    fn end(&self) -> Result<(), Error> {
        if self.last_level != u32::from(PINNED) {
            return Err(fail(LEVEL, "an arc's last vertex is not pinned"));
        }
        if (self.x, self.y) == self.first && (self.len < 4 || (self.levels && !self.inner_pin)) {
            return Err(fail(
                START,
                "a closed arc needs 4 vertices and a pinned one inside",
            ));
        }
        Ok(())
    }
}

fn point(x: i32, y: i32) -> Option<Point<E7>> {
    Point::from_units(y, x)
}

/// The vertices of one arc at one level of detail ([`Arcs::points`]).
#[derive(Clone, Debug)]
pub struct ArcPoints<'a> {
    arcs: Arcs<'a>,
    arc: usize,
    /// The arc's first vertex number.
    first: usize,
    /// The vertices not yet decoded.
    todo: Range<usize>,
    reversed: bool,
    lod: u8,
    mul: i32,
    xs: [i32; BLOCK_LEN],
    ys: [i32; BLOCK_LEN],
    lv: [u8; BLOCK_LEN],
    /// The decoded vertices not yet returned: `at..end` of the buffers.
    at: usize,
    end: usize,
}

impl ArcPoints<'_> {
    /// Decodes the next piece of the arc (forwards: from `todo.start` to its block's end;
    /// backwards: from the last block's start, or the arc's start, to `todo.end`).
    #[allow(
        clippy::many_single_char_names,
        reason = "a block's range s..e, its number b, its size n"
    )]
    fn fill(&mut self) -> bool {
        let Range { start, end } = self.todo;
        if start >= end {
            return false;
        }
        let (s, e) = if self.reversed {
            (((end - 1) / BLOCK_LEN * BLOCK_LEN).max(start), end)
        } else {
            (start, ((start / BLOCK_LEN + 1) * BLOCK_LEN).min(end))
        };
        let b = s / BLOCK_LEN;
        let (off, n) = (s % BLOCK_LEN, e - s);
        let (mut dx, mut dy) = ([0_u32; BLOCK_LEN], [0_u32; BLOCK_LEN]);
        self.arcs.dx.decode_block(b, &mut dx);
        self.arcs.dy.decode_block(b, &mut dy);
        if let Some(l) = self.arcs.level {
            let mut lv = [0_u32; BLOCK_LEN];
            l.decode_block(b, &mut lv);
            for (d, v) in self
                .lv
                .iter_mut()
                .zip(lv.get(off..off + n).unwrap_or_default())
            {
                *d = u8::try_from(*v).unwrap_or(0);
            }
        }
        // The first point is absolute: the arc's base at its start, else the block checkpoint.
        let (x0, y0) = if s == self.first {
            (
                self.arcs.base_x.get(self.arc),
                self.arcs.base_y.get(self.arc),
            )
        } else {
            (self.arcs.chk_x.get(b), self.arcs.chk_y.get(b))
        };
        let (mut x, mut y) = (unbias(x0.unwrap_or(0)), unbias(y0.unwrap_or(0)));
        let deltas = dx
            .get(off..off + n)
            .unwrap_or_default()
            .iter()
            .zip(dy.get(off..off + n).unwrap_or_default());
        for (j, (ddx, ddy)) in deltas.enumerate() {
            if j > 0 {
                x = x.wrapping_add(unzigzag(*ddx).cast_signed().wrapping_mul(self.mul));
                y = y.wrapping_add(unzigzag(*ddy).cast_signed().wrapping_mul(self.mul));
            }
            if let (Some(px), Some(py)) = (self.xs.get_mut(j), self.ys.get_mut(j)) {
                (*px, *py) = (x, y);
            }
        }
        self.todo = if self.reversed { start..s } else { e..end };
        (self.at, self.end) = (0, n);
        true
    }
}

impl Iterator for ArcPoints<'_> {
    type Item = Point<E7>;

    fn next(&mut self) -> Option<Point<E7>> {
        loop {
            while self.at < self.end {
                let j = if self.reversed {
                    self.end -= 1;
                    self.end
                } else {
                    self.at += 1;
                    self.at - 1
                };
                if self.lv.get(j).copied().unwrap_or(0) >= self.lod {
                    let p = point(*self.xs.get(j)?, *self.ys.get(j)?);
                    if p.is_none() {
                        self.todo = 0..0;
                        self.end = self.at;
                    }
                    return p;
                }
            }
            if !self.fill() {
                return None;
            }
        }
    }
}

/// One chunk of an arc ([`ArcChunks`]): the arc's vertices inside one 128-vertex block, plus
/// the first vertex of the next block when the arc goes on, so consecutive chunks share an end
/// and together hold every segment of the arc once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chunk {
    /// The smallest longitude and latitude of its vertices, 1e-7°.
    pub min: (i32, i32),
    /// The largest.
    pub max: (i32, i32),
    /// Its first vertex `(lon, lat)`.
    pub first: (i32, i32),
    /// Its last vertex.
    pub last: (i32, i32),
    /// Its vertex numbers (across all arcs), the last one included: `start..end`.
    pub start: u32,
    /// One past its last vertex.
    pub end: u32,
}

/// A box and the two ends of every [`Chunk`] of every arc, in arc order: built once per ARCS
/// (O(vertices), 40 B per chunk, about one chunk per 128 vertices plus one per arc) for
/// readers that skip the arcs far from a point (D-149, `Polygons::within_chunked`). Reads
/// never allocate.
#[cfg(feature = "alloc")]
#[derive(Clone, Debug, Default)]
pub struct ArcChunks {
    /// Per arc: its first chunk, then the chunk count.
    start: alloc::vec::Vec<u32>,
    chunks: alloc::vec::Vec<Chunk>,
}

#[cfg(feature = "alloc")]
impl ArcChunks {
    /// The chunks of every arc of `arcs`, decoded at full resolution.
    #[must_use]
    pub fn build(arcs: &Arcs<'_>) -> Self {
        let mut start = alloc::vec::Vec::with_capacity(arcs.len() + 1);
        let mut chunks = alloc::vec::Vec::with_capacity(arcs.len() + arcs.vertices() / BLOCK_LEN);
        for a in 0..arcs.len() {
            start.push(u32::try_from(chunks.len()).unwrap_or(u32::MAX));
            let Some(range) = arcs.range(a) else { continue };
            let mut cur: Option<Chunk> = None;
            for (v, p) in range.clone().zip(arcs.points(a, false, 0)) {
                let xy = (p.lon(), p.lat());
                let n = u32::try_from(v).unwrap_or(u32::MAX);
                if let Some(c) = &mut cur {
                    c.min = (c.min.0.min(xy.0), c.min.1.min(xy.1));
                    c.max = (c.max.0.max(xy.0), c.max.1.max(xy.1));
                    c.last = xy;
                    c.end = n + 1;
                    if v.is_multiple_of(BLOCK_LEN) && v + 1 < range.end {
                        chunks.push(*c);
                        cur = None;
                    }
                }
                if cur.is_none() && v + 1 < range.end {
                    cur = Some(Chunk {
                        min: xy,
                        max: xy,
                        first: xy,
                        last: xy,
                        start: n,
                        end: n + 1,
                    });
                }
            }
            chunks.extend(cur);
        }
        start.push(u32::try_from(chunks.len()).unwrap_or(u32::MAX));
        Self { start, chunks }
    }

    /// The chunks of arc `a` (empty past the end).
    #[must_use]
    pub fn of(&self, a: usize) -> &[Chunk] {
        match (self.start.get(a), self.start.get(a + 1)) {
            (Some(&s), Some(&e)) => self.chunks.get(s as usize..e as usize).unwrap_or_default(),
            _ => &[],
        }
    }

    /// The bytes it holds on the heap.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.start.capacity() * 4 + self.chunks.capacity() * size_of::<Chunk>()
    }
}

/// The arcs of an ARCS section to write (feature `write`).
#[cfg(feature = "write")]
#[derive(Clone, Copy, Debug, Default)]
pub struct ArcColumns<'c> {
    /// Each arc's vertices, at least 2, no vertex repeated in a row; a closed arc has at least
    /// 4.
    pub arcs: &'c [&'c [Point<E7>]],
    /// Each arc's per-vertex levels (0 to [`PINNED`], ends pinned, a closed arc pinned inside),
    /// or empty for full resolution only.
    pub levels: &'c [&'c [u8]],
    /// With `levels`: the tolerances of levels 1 to 6 in centimetres, increasing.
    pub tolerances_cm: &'c [u32],
}

/// The columns of [`encode`] as they are built.
#[cfg(feature = "write")]
#[derive(Default)]
struct Builder {
    start: alloc::vec::Vec<u32>,
    base_x: alloc::vec::Vec<u32>,
    base_y: alloc::vec::Vec<u32>,
    scale: alloc::vec::Vec<u32>,
    dx: alloc::vec::Vec<u32>,
    dy: alloc::vec::Vec<u32>,
    chk_x: alloc::vec::Vec<u32>,
    chk_y: alloc::vec::Vec<u32>,
    level: alloc::vec::Vec<u32>,
}

#[cfg(feature = "write")]
impl Builder {
    /// Adds one arc, after checking it.
    fn arc(&mut self, arc: &[Point<E7>], levels: Option<&[u8]>) -> Result<(), Error> {
        use teigeodata_core::bitpack::{bias, zigzag};
        let [first, .., last] = arc else {
            return Err(Error::Write("an arc of fewer than 2 vertices"));
        };
        let closed = first == last;
        if closed && arc.len() < 4 {
            return Err(Error::Write("a closed arc of fewer than 4 vertices"));
        }
        if let Some(lv) = levels {
            let inner = lv.get(1..lv.len().saturating_sub(1)).unwrap_or_default();
            if lv.len() != arc.len()
                || lv.iter().any(|&l| l > PINNED)
                || lv.first() != Some(&PINNED)
                || lv.last() != Some(&PINNED)
                || closed && !inner.contains(&PINNED)
            {
                return Err(Error::Write(
                    "levels: one per vertex, at most 7, ends pinned, a closed arc pinned inside",
                ));
            }
            self.level.extend(lv.iter().map(|&l| u32::from(l)));
        }
        let deltas = || {
            arc.windows(2).filter_map(|w| match w {
                [a, b] => Some((
                    i64::from(b.lon()) - i64::from(a.lon()),
                    i64::from(b.lat()) - i64::from(a.lat()),
                )),
                _ => None,
            })
        };
        for d in deltas() {
            if d == (0, 0) {
                return Err(Error::Write("a repeated vertex"));
            }
            if d.0.abs() > i64::from(i32::MAX) {
                return Err(Error::Write("a segment spans more than 2^31 - 1 units"));
            }
        }
        let k = (0..=MAX_SCALE)
            .rev()
            .find(|&k| {
                let p = i64::from(pow10(k));
                deltas().all(|d| d.0 % p == 0 && d.1 % p == 0)
            })
            .unwrap_or(0);
        let p = i64::from(pow10(k));
        let at = u32::try_from(self.dx.len()).map_err(|_| Error::Write("too many vertices"))?;
        self.start.push(at);
        self.base_x.push(bias(first.lon()));
        self.base_y.push(bias(first.lat()));
        self.scale.push(k);
        for (pt, d) in arc.iter().zip(core::iter::once((0, 0)).chain(deltas())) {
            if self.dx.len().is_multiple_of(BLOCK_LEN) {
                self.chk_x.push(bias(pt.lon()));
                self.chk_y.push(bias(pt.lat()));
            }
            // |Δx| ≤ 2³¹ − 1 (checked) and |Δy| ≤ 1.8 × 10⁹: both fit an i32.
            #[allow(
                clippy::cast_possible_truncation,
                reason = "checked to fit 32 bits above"
            )]
            let (x, y) = ((d.0 / p) as i32, (d.1 / p) as i32);
            self.dx.push(zigzag(x.cast_unsigned()));
            self.dy.push(zigzag(y.cast_unsigned()));
        }
        Ok(())
    }
}

/// Encodes an ARCS payload ([`Encoding::TABLE`](crate::Encoding::TABLE)); its item count is
/// the number of arcs. Each arc's scale is the largest power of ten, up to 10^7, dividing all
/// its deltas.
///
/// # Errors
/// [`Error::Write`] if an arc breaks a rule of [`ArcColumns`], a segment spans more than
/// 2³¹ − 1 grid units, or there are 2³² vertices or more.
#[cfg(feature = "write")]
pub fn encode(cols: &ArcColumns<'_>) -> Result<alloc::vec::Vec<u8>, Error> {
    encode_with(cols, None)
}

/// Encodes the `ARCS_OVERVIEW` payload of `full` (an ARCS with levels) at level `from` (1 to 6):
/// every arc with only its vertices of that level and coarser (D-102). Its item count is
/// `full.len()`; the arcs' ends are pinned, so every arc keeps at least its two ends.
///
/// # Errors
/// [`Error::Write`] if `full` has no levels or is an overview, `from` is out of range, or an
/// arc of `full` does not decode.
#[cfg(feature = "write")]
pub fn overview(full: &Arcs<'_>, from: u8) -> Result<alloc::vec::Vec<u8>, Error> {
    use alloc::vec::Vec;
    if !full.has_levels() || full.from.is_some() || !(1..PINNED).contains(&from) {
        return Err(Error::Write(
            "an overview needs an ARCS with levels and a level of 1 to 6",
        ));
    }
    let (mut pts, mut lvs): (Vec<Vec<Point<E7>>>, Vec<Vec<u8>>) = (Vec::new(), Vec::new());
    for a in 0..full.len() {
        let range = full.range(a).ok_or(Error::Write("an arc out of range"))?;
        let (mut p, mut l) = (Vec::new(), Vec::new());
        for (v, q) in range.zip(full.points(a, false, 0)) {
            let lv = full.level(v).ok_or(Error::Write("an arc out of range"))?;
            if lv >= from {
                p.push(q);
                l.push(lv);
            }
        }
        pts.push(p);
        lvs.push(l);
    }
    let tolerances: Vec<u32> = (1..PINNED)
        .map(|l| full.tolerance_cm(l).unwrap_or(0))
        .collect();
    let pr: Vec<&[Point<E7>]> = pts.iter().map(Vec::as_slice).collect();
    let lr: Vec<&[u8]> = lvs.iter().map(Vec::as_slice).collect();
    encode_with(
        &ArcColumns {
            arcs: &pr,
            levels: &lr,
            tolerances_cm: &tolerances,
        },
        Some(from),
    )
}

#[cfg(feature = "write")]
fn encode_with(cols: &ArcColumns<'_>, from: Option<u8>) -> Result<alloc::vec::Vec<u8>, Error> {
    use alloc::vec::Vec;
    use teigeodata_core::bitpack::{Coding, encode as pack, encode_direct};

    let with_levels = !cols.levels.is_empty();
    if with_levels
        && (cols.levels.len() != cols.arcs.len() || cols.tolerances_cm.len() != TOLERANCES)
        || !with_levels && !cols.tolerances_cm.is_empty()
    {
        return Err(Error::Write(
            "levels need one list per arc and 6 tolerances",
        ));
    }
    let increasing = cols
        .tolerances_cm
        .iter()
        .try_fold(0, |last, &t| (t > last).then_some(t));
    if increasing.is_none() {
        return Err(Error::Write("tolerances must be positive and increasing"));
    }
    let mut b = Builder::default();
    for (i, arc) in cols.arcs.iter().enumerate() {
        b.arc(arc, cols.levels.get(i).copied())?;
    }
    let end = u32::try_from(b.dx.len()).map_err(|_| Error::Write("too many vertices"))?;
    b.start.push(end);
    let mut encoded: Vec<(u16, Vec<u8>)> = alloc::vec![
        (START, encode_direct(&b.start)),
        (BASE_X, encode_direct(&b.base_x)),
        (BASE_Y, encode_direct(&b.base_y)),
        (DX, pack(&b.dx, Coding::For)),
        (DY, pack(&b.dy, Coding::For)),
        (CHK_X, encode_direct(&b.chk_x)),
        (CHK_Y, encode_direct(&b.chk_y)),
    ];
    if with_levels {
        encoded.push((LEVEL, pack(&b.level, Coding::For)));
        encoded.push((TOLERANCE, pack(cols.tolerances_cm, Coding::For)));
    }
    if b.scale.iter().any(|&k| k != 0) {
        encoded.push((SCALE, pack(&b.scale, Coding::For)));
    }
    if let Some(f) = from {
        encoded.push((FROM, encode_direct(&[u32::from(f)])));
    }
    let refs: Vec<(u16, &[u8])> = encoded.iter().map(|(id, b)| (*id, &b[..])).collect();
    teigeodata_core::table::encode(&refs).map_err(|_| Error::Write("the arc table"))
}
