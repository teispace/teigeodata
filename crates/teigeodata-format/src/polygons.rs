//! POLYGONS (kind 8): every polygon as parts, rings and signed references to [ARCS](crate::arcs)
//! (D-088, B10).
//!
//! A polygon is a place: each one names its PLACES row. A polygon has parts; a part's first ring
//! is its outer ring and the rest are holes. A ring is a list of arc references `2·arc +
//! reversed` (the `TopoJSON` model): each arc after the first drops its first vertex, which repeats
//! the previous arc's last, and the last arc ends where the first began. Outer rings run
//! counter-clockwise and holes clockwise (RFC 7946). The section's item count is the number of
//! polygons.
//!
//! | ID | Column | Values | |
//! |---:|---|---|---|
//! | 1 | [`ROW`] | per polygon: its PLACES row, increasing within a group | required |
//! | 2 | [`PARTS`] | polygons + 1: each polygon's first part, then the part count | required |
//! | 3 | [`RINGS`] | parts + 1: each part's first ring (its outer ring) | required |
//! | 4 | [`REFS_START`] | rings + 1: each ring's first arc reference | required |
//! | 5 | [`REFS`] | per reference: `2·arc + reversed` | required |
//! | 6 | [`LABEL_X`] | per polygon: a point inside it for its label, longitude, [biased](teigeodata_core::bitpack::bias) | required |
//! | 7 | [`LABEL_Y`] | per polygon: the label's latitude, biased | required |
//! | 8 | [`AREA`] | per polygon: its area on the ellipsoid in hectares, rounded | required |
//! | 9 | [`GROUPS`] | groups + 1: each group's first polygon (admin levels, coarse to fine), then the count | optional, absent = one group |
//!
//! Rings are read at any level of detail of the arcs, with no allocation ([`Polygons::ring`]).
//! [`Polygons::contains`] is exact, with the rule of [REGION](crate::region): a point on a border
//! is read as `p + (ε, ε²)`, the same as an even–odd ray cast east that counts an edge end on the
//! point's latitude as south. Point → admin unit is REGION's job (40–50 ns); `contains` is its
//! test oracle and the fallback of a pack without REGION.
//!
//! [`Polygons::open`] checks the frame and lengths; [`Polygons::check`] checks every value
//! against the arcs in O(vertices). Reads never panic.

use core::ops::Range;

use teigeodata_core::bitpack::{Packed, unbias};
use teigeodata_core::table::Table;
use teigeodata_core::{E7, Point};

use crate::arcs::{ArcPoints, Arcs};
use crate::{Error, Kind, Section, columns};

/// Column: each polygon's PLACES row.
pub const ROW: u16 = 1;
/// Column: each polygon's first part.
pub const PARTS: u16 = 2;
/// Column: each part's first ring.
pub const RINGS: u16 = 3;
/// Column: each ring's first reference.
pub const REFS_START: u16 = 4;
/// Column: the arc references.
pub const REFS: u16 = 5;
/// Column: label longitudes.
pub const LABEL_X: u16 = 6;
/// Column: label latitudes.
pub const LABEL_Y: u16 = 7;
/// Column: areas in hectares.
pub const AREA: u16 = 8;
/// Column: each group's first polygon.
pub const GROUPS: u16 = 9;

/// An arc reference: arc `arc`, walked backwards when `reversed`.
#[must_use]
pub const fn arc_ref(arc: u32, reversed: bool) -> u32 {
    arc << 1 | reversed as u32
}

fn fail(column: u16, problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::POLYGONS.0,
        column,
        problem,
    }
}

/// The POLYGONS section of an open pack.
#[derive(Clone, Copy, Debug)]
pub struct Polygons<'a> {
    row: Packed<'a>,
    parts: Packed<'a>,
    rings: Packed<'a>,
    refs_start: Packed<'a>,
    refs: Packed<'a>,
    label_x: Packed<'a>,
    label_y: Packed<'a>,
    area: Packed<'a>,
    groups: Option<Packed<'a>>,
}

/// Values `i` and `i + 1` of an offsets column as a range; `None` if empty or decreasing.
fn span(col: Packed<'_>, i: usize) -> Option<Range<usize>> {
    let (s, e) = (col.get(i)? as usize, col.get(i + 1)? as usize);
    (s < e).then_some(s..e)
}

impl<'a> Polygons<'a> {
    /// Opens the section: its frame, and every column's length, each offsets column from 0 to
    /// the length of the next level. O(columns + blocks).
    ///
    /// # Errors
    /// [`Error::Payload`] naming the column.
    pub fn open(section: &Section<'a>) -> Result<Self, Error> {
        let table: Table<'a> = columns::open(section, Kind::POLYGONS)?;
        let n = usize::try_from(section.item_count())
            .ok()
            .filter(|&n| n < u32::MAX as usize)
            .ok_or_else(|| fail(0, "the polygon count does not fit"))?;
        let k = Kind::POLYGONS;
        let row = columns::required(&table, k, ROW, Some(n))?;
        let parts = columns::required(&table, k, PARTS, Some(n + 1))?;
        let rings = columns::required(&table, k, RINGS, None)?;
        let refs_start = columns::required(&table, k, REFS_START, None)?;
        let refs = columns::required(&table, k, REFS, None)?;
        let label_x = columns::required(&table, k, LABEL_X, Some(n))?;
        let label_y = columns::required(&table, k, LABEL_Y, Some(n))?;
        let area = columns::required(&table, k, AREA, Some(n))?;
        let groups = match table.optional(GROUPS).map_err(|e| columns::map(k, e))? {
            Some(g) if g.len() >= 2 => Some(g),
            Some(_) => return Err(fail(GROUPS, "GROUPS needs at least 2 values")),
            None => None,
        };
        if table.ids().any(|id| id == 0 || id > GROUPS) {
            return Err(fail(0, "an unknown column"));
        }
        // Each offsets column runs from 0 to the length of the column it indexes.
        let ends = |col: Packed<'_>, len: usize| {
            col.get(0) == Some(0) && col.get(col.len() - 1).map(|e| e as usize) == Some(len)
        };
        for (id, col, len) in [
            (PARTS, parts, rings.len().saturating_sub(1)),
            (RINGS, rings, refs_start.len().saturating_sub(1)),
            (REFS_START, refs_start, refs.len()),
        ] {
            if col.is_empty() || !ends(col, len) {
                return Err(fail(
                    id,
                    "an offsets column must run from 0 to the next count",
                ));
            }
        }
        if let Some(g) = groups
            && !ends(g, n)
        {
            return Err(fail(GROUPS, "GROUPS must run from 0 to the polygon count"));
        }
        Ok(Self {
            row,
            parts,
            rings,
            refs_start,
            refs,
            label_x,
            label_y,
            area,
            groups,
        })
    }

    /// The number of polygons.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.row.len()
    }

    /// Whether there are none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.row.is_empty()
    }

    /// The number of groups (1 without a `GROUPS` column).
    #[must_use]
    pub const fn groups(&self) -> usize {
        match self.groups {
            Some(g) => g.len() - 1,
            None => 1,
        }
    }

    /// The polygons of group `g`.
    #[must_use]
    pub fn group(&self, g: usize) -> Option<Range<usize>> {
        match self.groups {
            Some(col) => {
                let (s, e) = (col.get(g)? as usize, col.get(g + 1)? as usize);
                (s <= e && e <= self.len()).then_some(s..e)
            }
            None => (g == 0).then_some(0..self.len()),
        }
    }

    /// The PLACES row of polygon `p`.
    #[must_use]
    pub fn row(&self, p: usize) -> Option<u32> {
        self.row.get(p)
    }

    /// The polygon of PLACES row `row`, if it has one: a binary search in each group.
    #[must_use]
    pub fn polygon_of(&self, row: u32) -> Option<usize> {
        (0..self.groups()).find_map(|g| {
            let r = self.group(g)?;
            let (mut lo, mut hi) = (r.start, r.end);
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                match self.row.get(mid)?.cmp(&row) {
                    core::cmp::Ordering::Less => lo = mid + 1,
                    core::cmp::Ordering::Greater => hi = mid,
                    core::cmp::Ordering::Equal => return Some(mid),
                }
            }
            None
        })
    }

    /// The parts of polygon `p`.
    #[must_use]
    pub fn parts(&self, p: usize) -> Option<Range<usize>> {
        span(self.parts, p)
    }

    /// The rings of part `part`; the first is its outer ring.
    #[must_use]
    pub fn rings(&self, part: usize) -> Option<Range<usize>> {
        span(self.rings, part)
    }

    /// The arc references of ring `ring`, as `(arc, reversed)`.
    pub fn refs(&self, ring: usize) -> impl Iterator<Item = (usize, bool)> + 'a {
        let r = span(self.refs_start, ring).unwrap_or(0..0);
        self.refs.values(r).map(|v| ((v >> 1) as usize, v & 1 == 1))
    }

    /// The label point of polygon `p`, inside it.
    #[must_use]
    pub fn label(&self, p: usize) -> Option<Point<E7>> {
        Point::from_units(unbias(self.label_y.get(p)?), unbias(self.label_x.get(p)?))
    }

    /// The area of polygon `p` in hectares.
    #[must_use]
    pub fn area_ha(&self, p: usize) -> Option<u32> {
        self.area.get(p)
    }

    /// The vertices of ring `ring` at level of detail `lod`, closed (the first point again at the
    /// end), in the ring's stored orientation. Empty past the end.
    #[must_use]
    pub fn ring(&self, arcs: &Arcs<'a>, ring: usize, lod: u8) -> RingPoints<'a> {
        let refs = span(self.refs_start, ring).unwrap_or(0..0);
        RingPoints {
            arcs: *arcs,
            refs: self.refs,
            todo: refs,
            lod,
            current: None,
            skip: false,
        }
    }

    /// Whether polygon `p` holds `at`, exactly (see the module docs for points on a border).
    /// O(vertices of `p`); no allocation.
    #[must_use]
    pub fn contains(&self, arcs: &Arcs<'a>, p: usize, at: Point<E7>) -> bool {
        let x = if at.lon() == 1_800_000_000 {
            -1_800_000_000
        } else {
            at.lon()
        };
        let q = (x, at.lat().min(899_999_999));
        let mut inside = false;
        for ring in self.all_rings(p) {
            let mut prev: Option<(i32, i32)> = None;
            for v in self.ring(arcs, ring, 0) {
                let b = (v.lon(), v.lat());
                if let Some(a) = prev
                    && crosses(q, a, b)
                {
                    inside = !inside;
                }
                prev = Some(b);
            }
        }
        inside
    }

    /// Every ring of polygon `p`.
    fn all_rings(&self, p: usize) -> Range<usize> {
        let parts = self.parts(p).unwrap_or(0..0);
        let first = self.rings(parts.start).map_or(0, |r| r.start);
        let last = parts
            .end
            .checked_sub(1)
            .and_then(|l| self.rings(l))
            .map_or(first, |r| r.end);
        first..last.max(first)
    }

    /// Checks every value against `arcs` and a PLACES count of `places`: offsets increasing
    /// (each polygon has a part, each part a ring, each ring a reference); rows below `places`,
    /// increasing within a group and unique; references to existing arcs that join end to start
    /// and close; rings of at least 4 vertices, outer rings counter-clockwise and holes
    /// clockwise; labels inside their polygon. O(vertices + polygons · log polygons).
    ///
    /// # Errors
    /// [`Error::Payload`] naming the column.
    pub fn check(&self, arcs: &Arcs<'a>, places: usize) -> Result<(), Error> {
        for (id, col) in [
            (PARTS, self.parts),
            (RINGS, self.rings),
            (REFS_START, self.refs_start),
        ] {
            let mut last = None;
            for v in col.values(0..col.len()) {
                if last.is_some_and(|l| v <= l) {
                    return Err(fail(id, "offsets must increase"));
                }
                last = Some(v);
            }
        }
        self.check_rows(places)?;
        for v in self.refs.values(0..self.refs.len()) {
            if (v >> 1) as usize >= arcs.len() {
                return Err(fail(REFS, "a reference to a missing arc"));
            }
        }
        for p in 0..self.len() {
            let parts = self
                .parts(p)
                .ok_or_else(|| fail(PARTS, "a polygon without parts"))?;
            for part in parts {
                let rings = self
                    .rings(part)
                    .ok_or_else(|| fail(RINGS, "a part without rings"))?;
                for ring in rings.clone() {
                    let area2 = self.check_ring(arcs, ring)?;
                    if (ring == rings.start) != (area2 > 0) {
                        return Err(fail(
                            REFS,
                            "outer rings run counter-clockwise, holes clockwise",
                        ));
                    }
                }
            }
            let label = self
                .label(p)
                .ok_or_else(|| fail(LABEL_X, "a label is off the grid"))?;
            if !self.contains(arcs, p, label) {
                return Err(fail(LABEL_X, "a label outside its polygon"));
            }
        }
        Ok(())
    }

    fn check_rows(&self, places: usize) -> Result<(), Error> {
        if let Some(g) = self.groups {
            let mut last = 0;
            for v in g.values(1..g.len()) {
                if v < last {
                    return Err(fail(GROUPS, "groups must not decrease"));
                }
                last = v;
            }
        }
        for g in 0..self.groups() {
            let r = self
                .group(g)
                .ok_or_else(|| fail(GROUPS, "a group out of range"))?;
            let mut last: Option<u32> = None;
            for p in r {
                let row = self.row.get(p).unwrap_or(u32::MAX);
                if row as usize >= places {
                    return Err(fail(ROW, "a row past the PLACES count"));
                }
                if last.is_some_and(|l| row <= l) {
                    return Err(fail(ROW, "rows must increase within a group"));
                }
                last = Some(row);
                if g > 0 && self.polygon_of(row) != Some(p) {
                    return Err(fail(ROW, "a row with two polygons"));
                }
            }
        }
        Ok(())
    }

    /// Checks that ring `ring` joins and closes, has at least 4 vertices, and returns twice its
    /// signed area (positive counter-clockwise).
    fn check_ring(&self, arcs: &Arcs<'a>, ring: usize) -> Result<i128, Error> {
        let mut first: Option<Point<E7>> = None;
        let mut end: Option<Point<E7>> = None;
        for (arc, reversed) in self.refs(ring) {
            let mut pts = arcs.points(arc, reversed, 0);
            let start = pts.next();
            let stop = pts.last();
            if end.is_some() && start != end {
                return Err(fail(REFS, "consecutive arcs of a ring do not join"));
            }
            first = first.or(start);
            end = stop;
        }
        if first.is_none() || first != end {
            return Err(fail(REFS, "a ring does not close"));
        }
        let (mut n, mut area2, mut prev) = (0_usize, 0_i128, None::<(i128, i128)>);
        let o = first.map_or((0, 0), |p| (i128::from(p.lon()), i128::from(p.lat())));
        for v in self.ring(arcs, ring, 0) {
            let b = (i128::from(v.lon()) - o.0, i128::from(v.lat()) - o.1);
            if let Some(a) = prev {
                area2 += a.0 * b.1 - b.0 * a.1;
            }
            prev = Some(b);
            n += 1;
        }
        if n < 4 || area2 == 0 {
            return Err(fail(REFS, "a ring of fewer than 4 vertices or no area"));
        }
        Ok(area2)
    }
}

/// Whether edge `a`–`b` toggles the even–odd count of `q`: its ends lie on opposite sides of
/// `q`'s latitude (an end on it counts as south) and it passes strictly east of `q`.
fn crosses(q: (i32, i32), a: (i32, i32), b: (i32, i32)) -> bool {
    if (a.1 > q.1) == (b.1 > q.1) {
        return false;
    }
    let lhs = (i128::from(q.0) - i128::from(a.0)) * (i128::from(b.1) - i128::from(a.1));
    let rhs = (i128::from(b.0) - i128::from(a.0)) * (i128::from(q.1) - i128::from(a.1));
    if b.1 > a.1 { lhs < rhs } else { lhs > rhs }
}

/// The vertices of one ring ([`Polygons::ring`]).
#[derive(Clone, Debug)]
pub struct RingPoints<'a> {
    arcs: Arcs<'a>,
    refs: Packed<'a>,
    todo: Range<usize>,
    lod: u8,
    current: Option<ArcPoints<'a>>,
    /// Whether the current arc's first vertex is the previous arc's last.
    skip: bool,
}

impl Iterator for RingPoints<'_> {
    type Item = Point<E7>;

    fn next(&mut self) -> Option<Point<E7>> {
        loop {
            if let Some(cur) = &mut self.current
                && let Some(p) = cur.next()
            {
                if self.skip {
                    self.skip = false;
                    continue;
                }
                return Some(p);
            }
            let r = self.todo.start;
            if r >= self.todo.end {
                self.current = None;
                return None;
            }
            self.todo.start += 1;
            let v = self.refs.get(r)?;
            self.skip = self.current.is_some();
            self.current = Some(self.arcs.points((v >> 1) as usize, v & 1 == 1, self.lod));
        }
    }
}

/// One polygon to write (feature `write`).
#[cfg(feature = "write")]
#[derive(Clone, Copy, Debug)]
pub struct PolygonInput<'c> {
    /// Its PLACES row.
    pub row: u32,
    /// Its parts, each a list of rings (the outer ring first), each a list of [`arc_ref`]s.
    pub parts: &'c [&'c [&'c [u32]]],
    /// A point inside it.
    pub label: Point<E7>,
    /// Its area in hectares.
    pub area_ha: u32,
}

/// Encodes a POLYGONS payload ([`Encoding::TABLE`](crate::Encoding::TABLE)); its item count is
/// the number of polygons. `groups` is empty (one group) or each group's first polygon, from 0,
/// increasing; rows must increase within a group. Ring geometry is checked by
/// [`Polygons::check`] against the arcs, not here.
///
/// # Errors
/// [`Error::Write`] if a polygon has no part, a part no ring, a ring no reference, rows do not
/// increase within a group, or the groups are malformed.
#[cfg(feature = "write")]
pub fn encode(polygons: &[PolygonInput<'_>], groups: &[u32]) -> Result<alloc::vec::Vec<u8>, Error> {
    use alloc::vec::Vec;
    use teigeodata_core::bitpack::{Coding, bias, encode as pack, encode_direct};

    let n = u32::try_from(polygons.len()).map_err(|_| Error::Write("too many polygons"))?;
    if !groups.is_empty()
        && (groups.first() != Some(&0)
            || !groups.is_sorted()
            || groups.last().is_some_and(|&l| l > n))
    {
        return Err(Error::Write(
            "groups start at 0, do not decrease and stay below the count",
        ));
    }
    let count = |v: usize| u32::try_from(v).map_err(|_| Error::Write("too many items"));
    let (mut row, mut parts, mut rings, mut refs_start, mut refs) = (
        Vec::new(),
        alloc::vec![0],
        alloc::vec![0],
        alloc::vec![0],
        Vec::new(),
    );
    let (mut lx, mut ly, mut area) = (Vec::new(), Vec::new(), Vec::new());
    for (i, p) in polygons.iter().enumerate() {
        let first_of_group = groups.binary_search(&count(i)?).is_ok();
        if !first_of_group && row.last().is_some_and(|&l| p.row <= l) {
            return Err(Error::Write("rows must increase within a group"));
        }
        if p.parts.is_empty()
            || p.parts
                .iter()
                .any(|part| part.is_empty() || part.iter().any(|ring| ring.is_empty()))
        {
            return Err(Error::Write(
                "a polygon needs parts, a part rings, a ring references",
            ));
        }
        for part in p.parts {
            for ring in *part {
                refs.extend_from_slice(ring);
                refs_start.push(count(refs.len())?);
            }
            rings.push(count(refs_start.len() - 1)?);
        }
        parts.push(count(rings.len() - 1)?);
        row.push(p.row);
        lx.push(bias(p.label.lon()));
        ly.push(bias(p.label.lat()));
        area.push(p.area_ha);
    }
    let mut cols: Vec<(u16, Vec<u8>)> = alloc::vec![
        (ROW, encode_direct(&row)),
        (PARTS, encode_direct(&parts)),
        (RINGS, encode_direct(&rings)),
        (REFS_START, encode_direct(&refs_start)),
        (REFS, pack(&refs, Coding::For)),
        (LABEL_X, encode_direct(&lx)),
        (LABEL_Y, encode_direct(&ly)),
        (AREA, pack(&area, Coding::For)),
    ];
    if !groups.is_empty() {
        let mut g = groups.to_vec();
        g.push(n);
        cols.push((GROUPS, encode_direct(&g)));
    }
    let refs: Vec<(u16, &[u8])> = cols.iter().map(|(id, b)| (*id, &b[..])).collect();
    teigeodata_core::table::encode(&refs).map_err(|_| Error::Write("the polygon table"))
}
