//! The packed Hilbert R-tree shared by [`PLACE_INDEX`](crate::place_index) and
//! [`POLY_INDEX`](crate::poly_index) (D-081, D-088): one implementation of the layout, the
//! quantised boxes, the queries and the canonical check.
//!
//! The items are the tree's leaves, in their section's order. Level 0 holds one box per node
//! size items, each level above one box per node size boxes below, up to a single root. The
//! root is stored exactly; every other box as four 16-bit offsets inside its parent's box,
//! rounded outward, so each stored box contains the exact bounds of the items below it.
//!
//! Items are either points read from PLACES (nothing stored for them) or boxes stored as a leaf
//! level, four offsets per item inside its level-0 box, ahead of the level-0 boxes in
//! [`BOXES`].
//!
//! | ID | Column | Values |
//! |---:|---|---|
//! | 1 | [`NODE_SIZE`] | one value: children per box, [`MIN_NODE`] to [`MAX_NODE`] |
//! | 2 | [`ROOT`] | south, north, west and east edge of the root on the 1e-6° grid, [biased](teigeodata_core::bitpack::bias); empty for no items |
//! | 3 | [`BOXES`] | stored leaves (if any), then every box but the root, level 0 first: its south, north, west and east edge as offsets 0 to [`QUANTA`] across its parent |

use core::ops::{ControlFlow, Range};

use crate::places::Places;
use crate::{Error, Kind, Section, columns};
use teigeodata_core::bitpack::{Packed, unbias};

/// Column: the node size.
pub const NODE_SIZE: u16 = 1;
/// Column: the root box.
pub const ROOT: u16 = 2;
/// Column: the stored leaves and every box but the root, relative to its parent.
pub const BOXES: u16 = 3;

/// The smallest node size.
pub const MIN_NODE: usize = 4;
/// The largest node size; it bounds the fixed query buffers.
pub const MAX_NODE: usize = 32;
/// The most levels: `MIN_NODE`¹⁶ = 2³² items.
pub(crate) const MAX_LEVELS: usize = 16;
/// The steps of a child edge across its parent.
pub const QUANTA: u32 = 65_535;

pub(crate) const DEG: i32 = 1_000_000;

/// A box on the 1e-6° grid, edges included. A query box with `min_lon > max_lon` crosses the
/// antimeridian.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    /// South edge.
    pub min_lat: i32,
    /// North edge.
    pub max_lat: i32,
    /// West edge.
    pub min_lon: i32,
    /// East edge.
    pub max_lon: i32,
}

impl Rect {
    /// The empty box: [`Rect::grow`] from it gives the bounds of what it grows by.
    pub const EMPTY: Self = Self {
        min_lat: i32::MAX,
        max_lat: i32::MIN,
        min_lon: i32::MAX,
        max_lon: i32::MIN,
    };

    /// The box of one point.
    #[must_use]
    pub const fn point(lat: i32, lon: i32) -> Self {
        Self {
            min_lat: lat,
            max_lat: lat,
            min_lon: lon,
            max_lon: lon,
        }
    }

    /// The smallest box on this grid holding the point `(lat, lon)` of the 1e-7° grid: the
    /// point itself when it is on the 1e-6° grid, else the cell around it.
    #[must_use]
    pub const fn around_e7(lat: i32, lon: i32) -> Self {
        Self {
            min_lat: lat.div_euclid(10),
            max_lat: ceil10(lat),
            min_lon: lon.div_euclid(10),
            max_lon: ceil10(lon),
        }
    }

    /// Whether the boxes share a point.
    #[must_use]
    pub const fn intersects(&self, o: &Self) -> bool {
        self.min_lat <= o.max_lat
            && o.min_lat <= self.max_lat
            && self.min_lon <= o.max_lon
            && o.min_lon <= self.max_lon
    }

    /// Whether `o` lies wholly inside this box.
    #[must_use]
    pub const fn contains(&self, o: &Self) -> bool {
        self.min_lat <= o.min_lat
            && o.max_lat <= self.max_lat
            && self.min_lon <= o.min_lon
            && o.max_lon <= self.max_lon
    }

    /// Grows this box to hold `o`.
    pub fn grow(&mut self, o: &Self) {
        self.min_lat = self.min_lat.min(o.min_lat);
        self.max_lat = self.max_lat.max(o.max_lat);
        self.min_lon = self.min_lon.min(o.min_lon);
        self.max_lon = self.max_lon.max(o.max_lon);
    }
}

/// ⌈v / 10⌉ for any `i32` (no overflow: |v / 10| is far inside the range).
const fn ceil10(v: i32) -> i32 {
    let d = v.div_euclid(10);
    if v.rem_euclid(10) == 0 { d } else { d + 1 }
}

/// The offsets of `[lo, hi]` inside the parent edge range `[p_lo, p_hi]`: the low edge rounded
/// down, the high edge up, so the decoded range contains `[lo, hi]`.
pub(crate) fn quantize(lo: i32, hi: i32, p_lo: i32, p_hi: i32) -> [u32; 2] {
    let span = i64::from(p_hi) - i64::from(p_lo);
    if span <= 0 {
        return [0, 0];
    }
    let q = i64::from(QUANTA);
    let low = (i64::from(lo) - i64::from(p_lo)) * q / span;
    let high = ((i64::from(hi) - i64::from(p_lo)) * q + span - 1) / span;
    let fit = |v: i64| u32::try_from(v.clamp(0, q)).unwrap_or(0);
    [fit(low), fit(high)]
}

/// The edge range of offsets `[q_lo, q_hi]` inside `[p_lo, p_hi]`, rounded outward. Defined for
/// any input (a corrupt offset gives some range, never a panic).
pub(crate) fn dequantize(q_lo: u32, q_hi: u32, p_lo: i32, p_hi: i32) -> [i32; 2] {
    let span = i64::from(p_hi) - i64::from(p_lo);
    if span <= 0 {
        return [p_lo, p_lo];
    }
    let q = i64::from(QUANTA);
    let lo = i64::from(p_lo) + i64::from(q_lo) * span / q;
    let hi = i64::from(p_lo) + (i64::from(q_hi) * span + q - 1) / q;
    let fit = |v: i64| i32::try_from(v).unwrap_or(p_hi);
    [fit(lo), fit(hi)]
}

/// A child box from its four stored offsets and its parent's box.
fn decode_box(q: &[u32], parent: &Rect) -> Rect {
    let at = |k: usize| q.get(k).copied().unwrap_or(0);
    let [min_lat, max_lat] = dequantize(at(0), at(1), parent.min_lat, parent.max_lat);
    let [min_lon, max_lon] = dequantize(at(2), at(3), parent.min_lon, parent.max_lon);
    Rect {
        min_lat,
        max_lat,
        min_lon,
        max_lon,
    }
}

/// The four stored offsets of the exact box `exact` inside its parent's decoded box.
fn encode_box(exact: &Rect, parent: &Rect) -> [u32; 4] {
    let [a, b] = quantize(exact.min_lat, exact.max_lat, parent.min_lat, parent.max_lat);
    let [c, d] = quantize(exact.min_lon, exact.max_lon, parent.min_lon, parent.max_lon);
    [a, b, c, d]
}

/// The level sizes for `items` items: `(first box, boxes)` per level, level 0 first, and the
/// number of levels. `None` if the tree would be deeper than [`MAX_LEVELS`].
pub(crate) fn levels(items: usize, node: usize) -> Option<([(usize, usize); MAX_LEVELS], usize)> {
    let mut out = [(0, 0); MAX_LEVELS];
    let (mut depth, mut count, mut first) = (0, items, 0);
    while count > 1 || (depth == 0 && count == 1) {
        count = count.div_ceil(node);
        *out.get_mut(depth)? = (first, count);
        first += count;
        depth += 1;
    }
    Some((out, depth))
}

/// Where the items' boxes come from.
#[derive(Clone, Copy, Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "PLACE_INDEX held its Places by value before; boxing would cost a read an indirection and the Copy"
)]
pub(crate) enum Leaves<'a> {
    /// PLACES points, nothing stored.
    Points(Places<'a>),
    /// A stored leaf level of this many boxes.
    Stored(usize),
}

/// An opened tree.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tree<'a> {
    kind: Kind,
    leaves: Leaves<'a>,
    len: usize,
    node: usize,
    levels: [(usize, usize); MAX_LEVELS],
    pub(crate) depth: usize,
    pub(crate) root: Rect,
    boxes: Packed<'a>,
    /// Boxes ahead of level 0 in [`BOXES`]: the stored leaves.
    base: usize,
}

impl<'a> Tree<'a> {
    pub(crate) const fn fail(kind: Kind, column: u16, problem: &'static str) -> Error {
        Error::Payload {
            kind: kind.0,
            column,
            problem,
        }
    }

    /// Opens a tree section of `kind` over `leaves`: the frame, the node size, the root and the
    /// length of every column; O(columns + blocks). The section's item count must be the
    /// number of items.
    pub(crate) fn open(
        section: &Section<'a>,
        kind: Kind,
        leaves: &Leaves<'a>,
    ) -> Result<Self, Error> {
        let leaves = *leaves;
        let fail = |c, p| Self::fail(kind, c, p);
        let table = columns::open(section, kind)?;
        let len = match leaves {
            Leaves::Points(p) => p.len(),
            Leaves::Stored(n) => n,
        };
        if usize::try_from(section.item_count()).ok() != Some(len) {
            return Err(fail(0, "the item count is not the number of items"));
        }
        let node = columns::required(&table, kind, NODE_SIZE, Some(1))?
            .get(0)
            .and_then(|v| usize::try_from(v).ok())
            .filter(|n| (MIN_NODE..=MAX_NODE).contains(n))
            .ok_or_else(|| fail(NODE_SIZE, "the node size is out of range"))?;
        let (levels, depth) =
            levels(len, node).ok_or_else(|| fail(NODE_SIZE, "too many levels"))?;
        // Every box but the root is stored: the first box of the root level is their count.
        let stored = levels.get(depth.wrapping_sub(1)).map_or(0, |l| l.0);
        let base = match leaves {
            Leaves::Points(_) => 0,
            Leaves::Stored(n) => n,
        };
        let root_col = columns::required(&table, kind, ROOT, Some(if depth == 0 { 0 } else { 4 }))?;
        let total = stored
            .checked_add(base)
            .and_then(|b| b.checked_mul(4))
            .ok_or_else(|| fail(BOXES, "too many boxes"))?;
        let boxes = columns::required(&table, kind, BOXES, Some(total))?;
        if table.ids().any(|id| id == 0 || id > BOXES) {
            return Err(fail(0, "an unknown column"));
        }
        let mut edges = [0; 4];
        root_col.run(0, &mut edges);
        let [min_lat, max_lat, min_lon, max_lon] = edges.map(unbias);
        Ok(Self {
            kind,
            leaves,
            len,
            node,
            levels,
            depth,
            root: Rect {
                min_lat,
                max_lat,
                min_lon,
                max_lon,
            },
            boxes,
            base,
        })
    }

    pub(crate) const fn node(&self) -> usize {
        self.node
    }

    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    /// The children of box `i` of `level`, whose decoded box is `parent`, into `out`: items
    /// below level 0, decoded boxes above. Returns the index of the first child and how many
    /// there are.
    pub(crate) fn children(
        &self,
        level: usize,
        i: usize,
        parent: &Rect,
        out: &mut [Rect; MAX_NODE],
    ) -> (usize, usize) {
        let first = i.saturating_mul(self.node);
        if level == 0
            && let Leaves::Points(places) = self.leaves
        {
            let mut lat = [0; MAX_NODE];
            let mut lon = [0; MAX_NODE];
            let take = self.node.min(MAX_NODE);
            let n = match (lat.get_mut(..take), lon.get_mut(..take)) {
                (Some(a), Some(o)) => places.run(first, a, o),
                _ => 0,
            };
            for (slot, (a, o)) in out.iter_mut().zip(lat.iter().zip(&lon)).take(n) {
                *slot = Rect::point(unbias(*a), unbias(*o));
            }
            return (first, n);
        }
        let (start, count) = self.below(level);
        let n = count.saturating_sub(first).min(self.node);
        let mut q = [0; 4 * MAX_NODE];
        let got = self
            .boxes
            .run(4 * (start + first), q.get_mut(..4 * n).unwrap_or_default());
        let n = got / 4;
        for (slot, quad) in out.iter_mut().zip(q.as_chunks::<4>().0).take(n) {
            *slot = decode_box(quad, parent);
        }
        (first, n)
    }

    /// The first stored box and the count of the level below box level `level`: the stored
    /// leaves below level 0.
    fn below(&self, level: usize) -> (usize, usize) {
        match level.checked_sub(1) {
            None => (0, self.base),
            Some(l) => self
                .levels
                .get(l)
                .map_or((0, 0), |&(first, count)| (self.base + first, count)),
        }
    }

    /// The stored box of item `item`, decoded from the root down its one path: O(depth), no
    /// allocation. `None` past the end, or for point leaves (nothing stored).
    pub(crate) fn item_box(&self, item: usize) -> Option<Rect> {
        if item >= self.len || matches!(self.leaves, Leaves::Points(_)) {
            return None;
        }
        let mut parent = self.root;
        let mut q = [0_u32; 4];
        // Boxes of level `l` each hold node^(l+1) items; the root level is not stored.
        for l in (0..self.depth.checked_sub(1)?).rev() {
            let mut width = self.node;
            for _ in 0..l {
                width = width.saturating_mul(self.node);
            }
            let at = self.base + self.levels.get(l)?.0 + item / width;
            if self.boxes.run(4 * at, &mut q) != 4 {
                return None;
            }
            parent = decode_box(&q, &parent);
        }
        if self.boxes.run(4 * item, &mut q) != 4 {
            return None;
        }
        Some(decode_box(&q, &parent))
    }

    /// The items below box `i` of `level`.
    pub(crate) fn span(&self, level: usize, i: usize) -> Range<usize> {
        let mut width = self.node;
        for _ in 0..level {
            width = width.saturating_mul(self.node);
        }
        let n = self.len;
        i.saturating_mul(width).min(n)..i.saturating_add(1).saturating_mul(width).min(n)
    }

    /// Calls `visit` with every item in `items` whose box meets `query`, edges included, in an
    /// unspecified order; stops early when `visit` breaks. A query with `min_lon > max_lon`
    /// crosses the antimeridian. Subtrees wholly outside `items` are skipped, so a contiguous
    /// range of items (one admin level of POLYGONS) costs only its own subtrees.
    /// Allocation-free: depth-first with fixed buffers; a box wholly inside the query yields its
    /// items without testing them.
    pub(crate) fn within<F>(&self, query: Rect, items: Range<usize>, visit: F) -> ControlFlow<()>
    where
        F: FnMut(usize) -> ControlFlow<()>,
    {
        self.within_counted(query, items, visit, &mut 0)
    }

    /// [`Tree::within`], adding the number of nodes it reads (children decoded) to `nodes`: a
    /// cost measure for benchmarks and tests.
    pub(crate) fn within_counted<F>(
        &self,
        query: Rect,
        items: Range<usize>,
        mut visit: F,
        nodes: &mut usize,
    ) -> ControlFlow<()>
    where
        F: FnMut(usize) -> ControlFlow<()>,
    {
        if query.min_lat > query.max_lat || items.start >= items.end.min(self.len) {
            return ControlFlow::Continue(());
        }
        if query.min_lon > query.max_lon {
            let east = Rect {
                max_lon: 180 * DEG,
                ..query
            };
            self.within_one(&east, &items, &mut visit, nodes)?;
            let west = Rect {
                min_lon: -180 * DEG,
                ..query
            };
            return self.within_one(&west, &items, &mut visit, nodes);
        }
        self.within_one(&query, &items, &mut visit, nodes)
    }

    fn within_one<F>(
        &self,
        q: &Rect,
        items: &Range<usize>,
        visit: &mut F,
        nodes: &mut usize,
    ) -> ControlFlow<()>
    where
        F: FnMut(usize) -> ControlFlow<()>,
    {
        let Some(root) = self.depth.checked_sub(1) else {
            return ControlFlow::Continue(());
        };
        if !q.intersects(&self.root) {
            return ControlFlow::Continue(());
        }
        let clip = |r: Range<usize>| r.start.max(items.start)..r.end.min(items.end);
        if q.contains(&self.root) {
            for item in clip(0..self.len) {
                visit(item)?;
            }
            return ControlFlow::Continue(());
        }
        let mut stack = [Frame::EMPTY; MAX_LEVELS];
        let mut top = 0;
        if let Some(f) = stack.get_mut(0) {
            f.load(self, root, 0, &self.root);
            *nodes += 1;
        }
        while let Some(f) = stack.get_mut(top) {
            let Some((i, b)) = f.take() else {
                if top == 0 {
                    break;
                }
                top -= 1;
                continue;
            };
            let level = f.level;
            if level == 0 {
                if items.contains(&i) && q.intersects(&b) {
                    visit(i)?;
                }
                continue;
            }
            let span = self.span(level - 1, i);
            if span.end <= items.start || span.start >= items.end {
                continue;
            }
            if q.contains(&b) {
                for item in clip(span) {
                    visit(item)?;
                }
            } else if q.intersects(&b)
                && let Some(next) = stack.get_mut(top + 1)
            {
                next.load(self, level - 1, i, &b);
                *nodes += 1;
                top += 1;
            }
        }
        ControlFlow::Continue(())
    }

    /// Checks the whole tree against the items: the root is exactly their bounds, and every
    /// stored offset is exactly what the writer derives from the exact bounds below that box
    /// and its parent's decoded box. A stored leaf's exact box is `leaf(item)`; points need
    /// none. One depth-first pass over the items in order; no allocation.
    pub(crate) fn check<F>(&self, mut leaf: F) -> Result<(), Error>
    where
        F: FnMut(usize) -> Result<Rect, Error>,
    {
        let Some(root) = self.depth.checked_sub(1) else {
            return Ok(());
        };
        let exact = self.check_box(root, 0, &self.root, &mut leaf)?;
        if exact != self.root {
            return Err(Self::fail(
                self.kind,
                ROOT,
                "the root is not the bounds of the items",
            ));
        }
        Ok(())
    }

    /// The exact bounds of the items below box `i` of `level`, after checking the stored
    /// offsets of its children against them. Offsets are compared, not decoded boxes: on a
    /// narrow parent several offsets decode to the same edge, and only one is canonical.
    /// Recursion depth is at most [`MAX_LEVELS`].
    fn check_box<F>(
        &self,
        level: usize,
        i: usize,
        decoded: &Rect,
        leaf: &mut F,
    ) -> Result<Rect, Error>
    where
        F: FnMut(usize) -> Result<Rect, Error>,
    {
        let mut kids = [Rect::EMPTY; MAX_NODE];
        let (first, n) = self.children(level, i, decoded, &mut kids);
        if n == 0 {
            return Err(Self::fail(self.kind, BOXES, "a box has no children"));
        }
        let mut exact = Rect::EMPTY;
        if level == 0 && matches!(self.leaves, Leaves::Points(_)) {
            for kid in kids.iter().take(n) {
                exact.grow(kid);
            }
            return Ok(exact);
        }
        let (start, _) = self.below(level);
        let mut q = [0; 4 * MAX_NODE];
        self.boxes
            .run(4 * (start + first), q.get_mut(..4 * n).unwrap_or_default());
        for (k, (kid, quad)) in kids.iter().zip(q.as_chunks::<4>().0).take(n).enumerate() {
            let e = if level == 0 {
                leaf(first + k)?
            } else {
                self.check_box(level - 1, first + k, kid, leaf)?
            };
            if encode_box(&e, decoded) != *quad {
                return Err(Self::fail(
                    self.kind,
                    BOXES,
                    "a box is not the bounds of its children",
                ));
            }
            exact.grow(&e);
        }
        Ok(exact)
    }
}

/// One level of a depth-first walk: the children of one box.
#[derive(Clone, Copy)]
struct Frame {
    /// The level of the box whose children these are; 0: the children are items.
    level: usize,
    first: usize,
    len: usize,
    next: usize,
    kids: [Rect; MAX_NODE],
}

impl Frame {
    const EMPTY: Self = Self {
        level: 0,
        first: 0,
        len: 0,
        next: 0,
        kids: [Rect::EMPTY; MAX_NODE],
    };

    fn load(&mut self, tree: &Tree<'_>, level: usize, i: usize, parent: &Rect) {
        let (first, len) = tree.children(level, i, parent, &mut self.kids);
        (self.level, self.first, self.len, self.next) = (level, first, len, 0);
    }

    /// The next child and its index.
    fn take(&mut self) -> Option<(usize, Rect)> {
        if self.next >= self.len {
            return None;
        }
        let b = *self.kids.get(self.next)?;
        self.next += 1;
        Some((self.first + self.next - 1, b))
    }
}

/// Encodes a tree payload over `items`, the items' exact boxes in their order, with `node`
/// children per box; `store_leaves` stores the items' boxes as a leaf level (feature `write`).
#[cfg(feature = "write")]
pub(crate) fn encode(
    items: &[Rect],
    node: usize,
    store_leaves: bool,
) -> Result<alloc::vec::Vec<u8>, Error> {
    use alloc::vec::Vec;
    use teigeodata_core::bitpack::{bias, encode_auto};

    if !(MIN_NODE..=MAX_NODE).contains(&node) {
        return Err(Error::Write("the node size is out of range"));
    }
    if items
        .iter()
        .any(|r| r.min_lat > r.max_lat || r.min_lon > r.max_lon)
    {
        return Err(Error::Write("an item has an empty box"));
    }
    let (_, depth) = levels(items.len(), node).ok_or(Error::Write("too many levels"))?;
    // The exact bounds of every level, bottom up: the items, then each box level.
    let mut exact: Vec<Vec<Rect>> = Vec::with_capacity(depth + 1);
    exact.push(items.to_vec());
    for l in 0..depth {
        let level: Vec<Rect> = exact
            .get(l)
            .map_or(&[][..], Vec::as_slice)
            .chunks(node)
            .map(|c| {
                let mut r = Rect::EMPTY;
                for b in c {
                    r.grow(b);
                }
                r
            })
            .collect();
        exact.push(level);
    }
    // Offsets top down: each box against its parent's decoded box. `exact[l + 1]` is box level
    // `l`; `exact[0]` the items, stored only with `store_leaves`.
    let bottom = usize::from(!store_leaves);
    let root = exact
        .last()
        .and_then(|l| l.first())
        .copied()
        .filter(|_| depth > 0);
    let mut offsets: Vec<Vec<[u32; 4]>> = alloc::vec![Vec::new(); depth];
    let mut decoded_above: Vec<Rect> = root.into_iter().collect();
    for e_level in (bottom..depth).rev() {
        let boxes = exact.get(e_level).map_or(&[][..], Vec::as_slice);
        let mut decoded = Vec::with_capacity(boxes.len());
        let mut quads = Vec::with_capacity(boxes.len());
        for (i, e) in boxes.iter().enumerate() {
            let parent = decoded_above.get(i / node).copied().unwrap_or(Rect::EMPTY);
            let q = encode_box(e, &parent);
            decoded.push(decode_box(&q, &parent));
            quads.push(q);
        }
        if let Some(slot) = offsets.get_mut(e_level) {
            *slot = quads;
        }
        decoded_above = decoded;
    }
    let flat: Vec<u32> = offsets.iter().flatten().flatten().copied().collect();
    let root_vals: Vec<u32> = root
        .map(|r| {
            [r.min_lat, r.max_lat, r.min_lon, r.max_lon]
                .map(bias)
                .to_vec()
        })
        .unwrap_or_default();
    let node_col = encode_auto(&[u32::try_from(node).unwrap_or(0)]);
    let (root_col, boxes_col) = (encode_auto(&root_vals), encode_auto(&flat));
    teigeodata_core::table::encode(&[
        (NODE_SIZE, &node_col[..]),
        (ROOT, &root_col),
        (BOXES, &boxes_col),
    ])
    .map_err(|_| Error::Write("the index table"))
}

#[cfg(test)]
mod tests {
    use super::{QUANTA, Rect, dequantize, levels, quantize};

    #[test]
    fn a_point_on_the_finer_grid_rounds_outward() {
        assert_eq!(
            Rect::around_e7(270, -270),
            Rect {
                min_lat: 27,
                max_lat: 27,
                min_lon: -27,
                max_lon: -27
            }
        );
        assert_eq!(
            Rect::around_e7(271, -271),
            Rect {
                min_lat: 27,
                max_lat: 28,
                min_lon: -28,
                max_lon: -27
            }
        );
        let edge = 1_800_000_000;
        assert_eq!(Rect::around_e7(-edge + 1, edge - 1).min_lat, -180_000_000);
    }

    #[test]
    fn level_sizes() {
        let sizes = |n, m| {
            let (l, d) = levels(n, m).unwrap_or_default();
            l.get(..d)
                .unwrap_or_default()
                .iter()
                .map(|x| x.1)
                .collect::<std::vec::Vec<_>>()
        };
        assert_eq!(sizes(0, 16), std::vec::Vec::<usize>::new());
        assert_eq!(sizes(1, 16), [1]);
        assert_eq!(sizes(16, 16), [1]);
        assert_eq!(sizes(17, 16), [2, 1]);
        assert_eq!(sizes(257, 16), [17, 2, 1]);
        assert_eq!(
            sizes(1 << 20, 4),
            [
                1 << 18,
                1 << 16,
                1 << 14,
                1 << 12,
                1 << 10,
                256,
                64,
                16,
                4,
                1
            ]
        );
        assert!(levels(usize::MAX, 4).is_none());
    }

    /// A decoded range always contains the range it was quantized from and stays inside the
    /// parent, for parents of every width (0, 1, below and far above `QUANTA` units).
    #[test]
    fn quantization_rounds_outward() {
        let mut s: u64 = 0x1234_5678_9abc_def1;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for _ in 0..if cfg!(miri) { 500 } else { 500_000 } {
            let widths = [0_i64, 1, 7, 65_534, 65_535, 65_536, 1_000_000, 360_000_000];
            let w = widths[usize::try_from(next() % 8).unwrap_or(0)];
            let p_lo =
                i32::try_from(i64::from(i32::try_from(next() % 1000).unwrap_or(0)) - 180_000_000)
                    .unwrap_or(0);
            let p_hi = i32::try_from(i64::from(p_lo) + w).unwrap_or(p_lo);
            let span = u64::try_from(w).unwrap_or(0) + 1;
            let a = p_lo + i32::try_from(next() % span).unwrap_or(0);
            let b = p_lo + i32::try_from(next() % span).unwrap_or(0);
            let (lo, hi) = (a.min(b), a.max(b));
            let [q0, q1] = quantize(lo, hi, p_lo, p_hi);
            assert!(q0 <= q1 && q1 <= QUANTA);
            // The format pins the rounding: the floor and the ceiling of the exact offset.
            if w > 0 {
                let off = |v: i32| u64::try_from(i64::from(v) - i64::from(p_lo)).unwrap_or(0);
                let (quanta, wu) = (u64::from(QUANTA), u64::try_from(w).unwrap_or(1));
                assert_eq!(
                    u64::from(q0),
                    off(lo) * quanta / wu,
                    "low edge of {lo} in {p_lo}..{p_hi}"
                );
                assert_eq!(
                    u64::from(q1),
                    (off(hi) * quanta).div_ceil(wu),
                    "high edge of {hi} in {p_lo}..{p_hi}"
                );
            }
            let [d0, d1] = dequantize(q0, q1, p_lo, p_hi);
            assert!(
                p_lo <= d0 && d0 <= lo && hi <= d1 && d1 <= p_hi,
                "{lo}..{hi} in {p_lo}..{p_hi}: {d0}..{d1}"
            );
            // And tight: each edge moves out by at most two quanta (one rounding each way).
            let slack = 2 * (w / i64::from(QUANTA) + 1);
            assert!(
                i64::from(lo) - i64::from(d0) <= slack && i64::from(d1) - i64::from(hi) <= slack,
                "{lo}..{hi} in {p_lo}..{p_hi}: {d0}..{d1} is loose"
            );
        }
    }
}
