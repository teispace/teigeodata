//! `PLACE_INDEX` (kind 6): a packed Hilbert R-tree over the PLACES rows (D-081).
//!
//! PLACES rows are in Hilbert order, the order a packed Hilbert R-tree sorts its items into, so
//! the rows are the tree's leaves and only the boxes above them are stored. Level 0 holds one
//! box per [node size](PlaceIndex::node_size) rows, each level above one box per node size
//! boxes below, up to a single root. The root is stored exactly; every other box as four 16-bit
//! offsets inside its parent's box, rounded outward, so each stored box contains the exact
//! bounds of the rows below it (8 bytes per box, about 0.5 byte per place at node size 16).
//!
//! A [column table](teigeodata_core::table) with these columns:
//!
//! | ID | Column | Values |
//! |---:|---|---|
//! | 1 | [`NODE_SIZE`] | one value: children per box, [`MIN_NODE`] to [`MAX_NODE`] |
//! | 2 | [`ROOT`] | south, north, west and east edge of the root on the 1e-6° grid, [biased](teigeodata_core::bitpack::bias); empty for no places |
//! | 3 | [`BOXES`] | every other box, level 0 first: its south, north, west and east edge as offsets 0 to [`QUANTA`] across its parent |
//!
//! Its item count is the number of places. [`PlaceIndex::open`] checks the structure;
//! [`PlaceIndex::check`] also checks that every offset is exactly what the writer derives from
//! the rows, so a checked index is canonical. Queries never panic, never allocate and only
//! return rows below the row count. Their answers are exact only on a checked index: a box
//! that lies wholly inside the query yields its rows without testing each one, so a corrupt box
//! can add or drop rows. Run [`PlaceIndex::check`] on packs from an untrusted source (a signed
//! pack is only as trustworthy as its signer).
//!
//! Distances are on a sphere of the mean Earth radius ([`EARTH_RADIUS_M`]), by the haversine
//! formula; with feature `spatial` (pure-Rust `libm`, the same result on every platform).

use core::ops::ControlFlow;

use crate::places::Places;
use crate::rtree::{Leaves, Tree};
use crate::{Error, Kind, Section};

pub use crate::rtree::{BOXES, MAX_NODE, MIN_NODE, NODE_SIZE, QUANTA, ROOT, Rect};

/// The mean Earth radius (IUGG), in metres.
pub const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// The `PLACE_INDEX` section of an open pack, with the PLACES rows it indexes.
#[derive(Clone, Copy, Debug)]
pub struct PlaceIndex<'a> {
    tree: Tree<'a>,
}

impl<'a> PlaceIndex<'a> {
    /// Opens the section over `places`: the frame, the node size, the root and the length of
    /// every column; O(columns + blocks).
    ///
    /// # Errors
    /// [`Error::Payload`] if the section is not a `PLACE_INDEX` column table, its item count is
    /// not the number of places, the node size is out of range, or a column is malformed or of
    /// the wrong length.
    pub fn open(section: &Section<'a>, places: Places<'a>) -> Result<Self, Error> {
        Tree::open(section, Kind::PLACE_INDEX, &Leaves::Points(places)).map(|tree| Self { tree })
    }

    /// Children per box.
    #[must_use]
    pub const fn node_size(&self) -> usize {
        self.tree.node()
    }

    /// The number of box levels above the rows.
    #[must_use]
    pub const fn depth(&self) -> usize {
        self.tree.depth
    }

    /// Calls `visit` with every row whose point is in `query`, edges included, in an
    /// unspecified order; stops early when `visit` breaks. A query with `min_lon > max_lon`
    /// crosses the antimeridian. Allocation-free: depth-first with fixed buffers; a box wholly
    /// inside the query yields its rows without testing them.
    pub fn within<F>(&self, query: Rect, visit: F) -> ControlFlow<()>
    where
        F: FnMut(usize) -> ControlFlow<()>,
    {
        self.tree.within(query, 0..self.tree.len(), visit)
    }

    /// Checks the whole tree against the rows: the root is exactly their bounds, and every
    /// stored offset is exactly what the writer derives from the exact bounds of the rows below
    /// that box and its parent's decoded box. One depth-first pass over the rows in order;
    /// O(rows), no allocation.
    ///
    /// # Errors
    /// [`Error::Payload`] naming [`ROOT`] or [`BOXES`].
    pub fn check(&self) -> Result<(), Error> {
        self.tree
            .check(|_| Err(Tree::fail(Kind::PLACE_INDEX, BOXES, "a stored leaf")))
    }
}

/// A row found by [`PlaceIndex::nearest`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Neighbour {
    /// The PLACES row.
    pub row: usize,
    /// The haversine of the central angle: monotone in distance, compared exactly.
    pub haversine: f64,
}

/// The haversine of the central angle of a great-circle distance of `meters`, as
/// [`Neighbour::haversine`] holds it: the cap of a radius query. Clamped to a half turn.
#[cfg(feature = "spatial")]
#[must_use]
pub fn haversine_of(meters: f64) -> f64 {
    let half = (meters.max(0.0) / (2.0 * EARTH_RADIUS_M)).min(core::f64::consts::FRAC_PI_2);
    let s = libm::sin(half);
    s * s
}

/// The great-circle distance in metres between (`lat1`, `lon1`) and (`lat2`, `lon2`), in
/// degrees, by the haversine on the mean Earth sphere; the same result on every platform.
#[cfg(feature = "spatial")]
#[must_use]
pub fn meters_between(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let sp = libm::sin((p2 - p1) / 2.0);
    let sl = libm::sin((lon2 - lon1).to_radians() / 2.0);
    let h = libm::fma(libm::cos(p1), libm::cos(p2) * sl * sl, sp * sp);
    2.0 * EARTH_RADIUS_M * libm::asin(libm::sqrt(h.clamp(0.0, 1.0)))
}

/// The initial great-circle bearing from the point (`lat1`, `lon1`) to (`lat2`, `lon2`), all in
/// degrees, as degrees clockwise from true north in `[0, 360)`: "3 km at 45° from Pokhara" is
/// the bearing from Pokhara to the point (TG-19). 0 for one point or either pole's
/// undefined direction; the same result on every platform (`libm`).
#[cfg(feature = "spatial")]
#[must_use]
pub fn bearing_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dl = (lon2 - lon1).to_radians();
    let y = libm::sin(dl) * libm::cos(p2);
    let x = libm::cos(p1) * libm::sin(p2) - libm::sin(p1) * libm::cos(p2) * libm::cos(dl);
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    let d = libm::atan2(y, x).to_degrees();
    let d = if d < 0.0 { d + 360.0 } else { d };
    // -0.0 and a value that rounds up to 360 both read as north.
    if d >= 360.0 || d == 0.0 { 0.0 } else { d }
}

#[cfg(feature = "spatial")]
impl Neighbour {
    /// The great-circle distance in metres.
    #[must_use]
    pub fn meters(&self) -> f64 {
        2.0 * EARTH_RADIUS_M * libm::asin(libm::sqrt(self.haversine.clamp(0.0, 1.0)))
    }
}

#[cfg(feature = "spatial")]
pub(crate) mod nearest {
    use super::{MAX_NODE, Neighbour, PlaceIndex, Rect};
    use crate::rtree::MAX_LEVELS;
    use teigeodata_core::{E6, Point};

    const RAD_PER_UNIT: f64 = core::f64::consts::PI / 180.0 / 1_000_000.0;
    const TURN: i64 = 360 * 1_000_000;
    /// Lower bounds are scaled by this before a box is pruned, so float rounding never prunes a
    /// box holding a point at the current k-th distance.
    const SLACK: f64 = 1.0 - 1e-12;

    fn rad(units: i64) -> f64 {
        #[allow(clippy::cast_precision_loss, reason = "|units| < 2⁵³")]
        let u = units as f64;
        u * RAD_PER_UNIT
    }

    /// The shortest longitude difference in grid units, in 0 to 180°. Grid longitudes differ by
    /// at most a turn, so no modulo (an integer division, slow on the hot path) is needed; a
    /// corrupt box edge beyond the grid gives some non-negative gap, never a panic.
    fn lon_gap(a: i32, b: i32) -> i64 {
        let d = (i64::from(a) - i64::from(b)).abs();
        if d > TURN / 2 { (TURN - d).max(0) } else { d }
    }

    /// A lower bound of `sin y` for `0 ≤ y ≤ π/2`: Taylor to y⁷, ending on a negative term.
    fn sin_lb(y: f64) -> f64 {
        // Reciprocals as constants: a multiply instead of a divide; their rounding (1e-16) is
        // far inside `SLACK`.
        let y2 = y * y;
        y * (1.0 - y2 * (1.0 / 6.0) * (1.0 - y2 * (1.0 / 20.0) * (1.0 - y2 * (1.0 / 42.0))))
    }

    /// A lower bound of `cos φ` for `|φ| ≤ π/2`, at least 0: Taylor to φ⁶.
    fn cos_lb(p: f64) -> f64 {
        let p2 = p * p;
        (1.0 - p2 * 0.5 * (1.0 - p2 * (1.0 / 12.0) * (1.0 - p2 * (1.0 / 30.0)))).max(0.0)
    }

    /// The query point, with its cosine computed once.
    pub(crate) struct Query {
        lat: i32,
        lon: i32,
        cos: f64,
    }

    impl Query {
        pub(crate) fn new(at: Point<E6>) -> Self {
            Self {
                lat: at.lat(),
                lon: at.lon(),
                cos: libm::cos(rad(i64::from(at.lat()))),
            }
        }

        /// A lower bound of the haversine to any point of `b`, without trigonometry:
        /// haversin d = haversin Δφ + cos φ₁ cos φ₂ haversin Δλ, each factor bounded below over the box.
        pub(super) fn bound(&self, b: &Rect) -> f64 {
            self.bound_from(b, self.gaps(b))
        }

        /// The latitude and longitude gaps from the query to `b` in grid units (0 inside).
        pub(super) fn gaps(&self, b: &Rect) -> (i64, i64) {
            let dlat = if self.lat < b.min_lat {
                i64::from(b.min_lat) - i64::from(self.lat)
            } else if self.lat > b.max_lat {
                i64::from(self.lat) - i64::from(b.max_lat)
            } else {
                0
            };
            let dlon = if b.min_lon <= self.lon && self.lon <= b.max_lon {
                0
            } else {
                lon_gap(self.lon, b.min_lon).min(lon_gap(self.lon, b.max_lon))
            };
            (dlat, dlon)
        }

        /// [`Query::bound`] from the gaps already computed.
        fn bound_from(&self, b: &Rect, (dlat, dlon): (i64, i64)) -> f64 {
            let s1 = sin_lb(rad(dlat) * 0.5);
            let lat_term = s1 * s1;
            if dlon == 0 {
                return lat_term * SLACK;
            }
            let far = i64::from(b.min_lat.unsigned_abs().max(b.max_lat.unsigned_abs()));
            let s2 = sin_lb(rad(dlon) * 0.5);
            (lat_term + self.cos * cos_lb(rad(far)) * s2 * s2) * SLACK
        }

        /// The exact haversine to a point.
        pub(crate) fn haversine(&self, lat: i32, lon: i32) -> f64 {
            // The difference in grid units first: subtracting two rounded angles of nearby
            // points loses about 1e-12 of the result, more than the bound's slack.
            let s1 = libm::sin(rad(i64::from(lat) - i64::from(self.lat)) * 0.5);
            let s2 = libm::sin(rad(lon_gap(self.lon, lon)) * 0.5);
            let phi = rad(i64::from(lat));
            (s1 * s1 + self.cos * libm::cos(phi) * s2 * s2).clamp(0.0, 1.0)
        }
    }

    /// Inserts `n` into the sorted `out[..found]` of capacity `out.len()` (by haversine, then
    /// row); returns the new count.
    fn insert(out: &mut [Neighbour], found: usize, n: Neighbour) -> usize {
        let key = |x: &Neighbour| (x.haversine, x.row);
        let cap = out.len();
        let mut at = found.min(cap);
        while at > 0 && out.get(at - 1).is_some_and(|p| key(p) > key(&n)) {
            at -= 1;
        }
        if at >= cap {
            return found;
        }
        let last = found.min(cap - 1);
        if let Some(tail) = out.get_mut(at..=last) {
            tail.rotate_right(1);
        }
        if let Some(slot) = out.get_mut(at) {
            *slot = n;
        }
        (found + 1).min(cap)
    }

    /// The result so far: the sorted `out[..found]`.
    ///
    /// It also keeps two integer gates from the k-th distance `r`, recomputed only when that
    /// distance improves: a box whose latitude gap exceeds `r` (the central angle is at least
    /// the latitude difference), or whose longitude gap exceeds `asin(sin r / cos φ)` (the
    /// cross-track distance to a meridian, sin d ≥ cos φ · sin Δλ, and d grows with Δλ), cannot
    /// hold a nearer row. Both are rounded outward; children failing them are dropped with two
    /// integer compares, before any float bound.
    struct Best<'o> {
        out: &'o mut [Neighbour],
        found: usize,
        kth: f64,
        lat_gate: i64,
        lon_gate: i64,
        cos: f64,
    }

    /// Radians to grid units, rounded up and widened, so a gate is never too tight.
    fn units_up(r: f64) -> i64 {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "at most a half turn in units"
        )]
        let u = libm::ceil(r / RAD_PER_UNIT * (1.0 + 1e-9)) as i64;
        u.saturating_add(2)
    }

    impl<'o> Best<'o> {
        fn new(out: &'o mut [Neighbour], q: &Query, cap: f64) -> Self {
            let mut best = Self {
                out,
                found: 0,
                kth: f64::INFINITY,
                lat_gate: i64::MAX,
                lon_gate: i64::MAX,
                cos: q.cos,
            };
            if cap < f64::INFINITY {
                best.tighten(cap);
            }
            best
        }

        /// The k-th haversine, or the cap until `out` is full: every row offered is within it,
        /// so a radius query's cap holds without a check of its own.
        const fn kth(&self) -> f64 {
            self.kth
        }

        /// Whether a box with these gaps may hold a nearer row.
        const fn open(&self, (dlat, dlon): (i64, i64)) -> bool {
            dlat <= self.lat_gate && dlon <= self.lon_gate
        }

        fn offer(&mut self, n: Neighbour) {
            self.found = insert(self.out, self.found, n);
            if self.found < self.out.len() {
                return;
            }
            let kth = self.out.last().map_or(f64::INFINITY, |n| n.haversine);
            if kth.total_cmp(&self.kth).is_eq() {
                return;
            }
            self.tighten(kth);
        }

        /// Sets the k-th haversine and the integer gates it implies.
        fn tighten(&mut self, kth: f64) {
            self.kth = kth;
            let r = 2.0 * libm::asin(libm::sqrt(kth.clamp(0.0, 1.0)));
            self.lat_gate = units_up(r);
            // Rounded up, and no gate near 1, where asin is too steep to trust: a ratio of 1 or
            // more means the circle of radius r holds a pole and so every longitude.
            let ratio = libm::sin(r) / self.cos * (1.0 + 1e-12);
            self.lon_gate = if r >= core::f64::consts::FRAC_PI_2
                || ratio.partial_cmp(&(1.0 - 1e-6)) != Some(core::cmp::Ordering::Less)
            {
                i64::MAX
            } else {
                units_up(libm::asin(ratio))
            };
        }
    }

    /// A queued box: its bound, its level and index, and its decoded box (to decode its
    /// children).
    #[derive(Clone, Copy)]
    struct Entry {
        bound: f64,
        level: u32,
        i: usize,
        rect: Rect,
    }

    /// Boxes waiting in a best-first search: a binary min-heap on the bound, in a fixed buffer.
    struct Heap {
        len: usize,
        items: [Entry; HEAP],
    }

    /// The heap's capacity (16 KiB). A search that would overflow it walks that subtree
    /// depth-first in place instead, so results stay exact; under `cfg(test)` it is tiny, so
    /// the tests take that path.
    #[cfg(not(test))]
    const HEAP: usize = 409;
    #[cfg(test)]
    const HEAP: usize = 8;

    const EMPTY_ENTRY: Entry = Entry {
        bound: 0.0,
        level: 0,
        i: 0,
        rect: Rect::EMPTY,
    };

    impl Heap {
        fn push(&mut self, e: Entry) -> bool {
            if self.len >= HEAP {
                return false;
            }
            let mut i = self.len;
            self.len += 1;
            while i > 0 {
                let parent = (i - 1) / 2;
                match self.items.get(parent) {
                    Some(&p) if p.bound > e.bound => {
                        if let Some(slot) = self.items.get_mut(i) {
                            *slot = p;
                        }
                        i = parent;
                    }
                    _ => break,
                }
            }
            if let Some(slot) = self.items.get_mut(i) {
                *slot = e;
            }
            true
        }

        fn pop(&mut self) -> Option<Entry> {
            if self.len == 0 {
                return None;
            }
            let top = *self.items.first()?;
            self.len -= 1;
            let last = *self.items.get(self.len)?;
            let mut i = 0;
            loop {
                let key = |k: usize| self.items.get(k).map_or(f64::INFINITY, |e| e.bound);
                let (l, r) = (2 * i + 1, 2 * i + 2);
                let mut m = i;
                let mut best = last.bound;
                if l < self.len && key(l) < best {
                    (m, best) = (l, key(l));
                }
                if r < self.len && key(r) < best {
                    m = r;
                }
                if m == i {
                    break;
                }
                let child = *self.items.get(m)?;
                *self.items.get_mut(i)? = child;
                i = m;
            }
            *self.items.get_mut(i)? = last;
            Some(top)
        }
    }

    /// A child in a depth-first frame (24 bytes, so a full stack stays under 16 KiB): its
    /// bound rounded down to `f32`, its index among the box's children, and its box.
    #[derive(Clone, Copy)]
    struct Kid {
        bound: f32,
        k: u32,
        rect: Rect,
    }

    /// A lower bound as `f32`, never above the `f64` one.
    fn down(b: f64) -> f32 {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "rounded, then stepped down"
        )]
        let f = b as f32;
        if f64::from(f) > b { f.next_down() } else { f }
    }

    /// The children of one box, nearest first.
    #[derive(Clone, Copy)]
    struct Frame {
        level: usize,
        first: usize,
        len: usize,
        next: usize,
        kids: [Kid; MAX_NODE],
    }

    impl Frame {
        const EMPTY: Self = Self {
            level: 0,
            first: 0,
            len: 0,
            next: 0,
            kids: [Kid {
                bound: 0.0,
                k: 0,
                rect: Rect::EMPTY,
            }; MAX_NODE],
        };

        fn load(&mut self, ix: &PlaceIndex<'_>, q: &Query, level: usize, i: usize, parent: &Rect) {
            let mut boxes = [Rect::EMPTY; MAX_NODE];
            let (first, len) = ix.tree.children(level, i, parent, &mut boxes);
            for (k, (slot, b)) in (0_u32..).zip(self.kids.iter_mut().zip(&boxes).take(len)) {
                *slot = Kid {
                    bound: down(q.bound(b)),
                    k,
                    rect: *b,
                };
            }
            // Insertion sort: at most MAX_NODE entries.
            let kids = self.kids.get_mut(..len).unwrap_or_default();
            for j in 1..kids.len() {
                let mut m = j;
                while m > 0 && kids.get(m - 1).map(|k| k.bound) > kids.get(m).map(|k| k.bound) {
                    kids.swap(m - 1, m);
                    m -= 1;
                }
            }
            (self.level, self.first, self.len, self.next) = (level, first, len, 0);
        }
    }

    impl PlaceIndex<'_> {
        /// Fills `out` with the `out.len()` rows nearest to `at`, nearest first (ties by row),
        /// and returns how many it found (fewer only if the pack has fewer places).
        ///
        /// Best-first search (Hjaltason and Samet) on a fixed-size heap: boxes are expanded in
        /// order of a lower bound on their distance that needs no trigonometry, the rows of a
        /// level-0 box are ranked as it is expanded, and exact haversines are computed only for
        /// rows whose bound can still enter the result. Allocation-free.
        pub fn nearest(&self, at: Point<E6>, out: &mut [Neighbour]) -> usize {
            self.nearest_within(at, f64::INFINITY, out)
        }

        /// As [`nearest`](Self::nearest), keeping only rows whose haversine is at most
        /// `max_haversine` (see [`haversine_of`](super::haversine_of)): the search starts with
        /// that cap as its k-th distance, so boxes beyond it are never opened.
        pub fn nearest_within(
            &self,
            at: Point<E6>,
            max_haversine: f64,
            out: &mut [Neighbour],
        ) -> usize {
            let Some(root) = self.tree.depth.checked_sub(1) else {
                return 0;
            };
            if out.is_empty() {
                return 0;
            }
            let q = Query::new(at);
            if max_haversine.is_nan() || max_haversine < 0.0 {
                return 0;
            }
            let mut best = Best::new(out, &q, max_haversine);
            let mut heap = Heap {
                len: 0,
                items: [EMPTY_ENTRY; HEAP],
            };
            heap.push(Entry {
                bound: 0.0,
                level: u32::try_from(root).unwrap_or(0),
                i: 0,
                rect: self.tree.root,
            });
            let mut boxes = [Rect::EMPTY; MAX_NODE];
            while let Some(e) = heap.pop() {
                if e.bound > best.kth() {
                    break;
                }
                let level = e.level as usize;
                let (first, len) = self.tree.children(level, e.i, &e.rect, &mut boxes);
                for (k, b) in boxes.iter().take(len).enumerate() {
                    self.visit(&q, level, first + k, b, &mut best, &mut heap);
                }
            }
            best.found
        }

        /// One child of a box of `level`: a row is ranked; a box is queued, or searched in
        /// place if the queue is full.
        fn visit(
            &self,
            q: &Query,
            level: usize,
            i: usize,
            b: &Rect,
            best: &mut Best<'_>,
            heap: &mut Heap,
        ) {
            let gaps = q.gaps(b);
            if !best.open(gaps) {
                return;
            }
            let kth = best.kth();
            let bound = q.bound_from(b, gaps);
            if bound > kth {
                return;
            }
            if level == 0 {
                let haversine = q.haversine(b.min_lat, b.min_lon);
                if haversine <= kth {
                    best.offer(Neighbour { row: i, haversine });
                }
                return;
            }
            let entry = Entry {
                bound,
                level: u32::try_from(level - 1).unwrap_or(0),
                i,
                rect: *b,
            };
            if !heap.push(entry) {
                self.depth_first(q, level - 1, i, b, best);
            }
        }

        /// The overflow path: box `i` of `level` searched depth-first, children nearest first,
        /// on a fixed stack.
        fn depth_first(&self, q: &Query, level: usize, i: usize, rect: &Rect, best: &mut Best<'_>) {
            let mut stack = [Frame::EMPTY; MAX_LEVELS];
            let mut top = 0;
            if let Some(f) = stack.get_mut(0) {
                f.load(self, q, level, i, rect);
            }
            loop {
                let kth = best.kth();
                let Some(frame) = stack.get_mut(top) else {
                    break;
                };
                let next = frame
                    .kids
                    .get(frame.next)
                    .copied()
                    .filter(|_| frame.next < frame.len);
                let Some(kid) = next.filter(|k| f64::from(k.bound) <= kth) else {
                    // Sorted: once one child is too far, so are the rest.
                    if top == 0 {
                        break;
                    }
                    top -= 1;
                    continue;
                };
                frame.next += 1;
                let (level, i) = (frame.level, frame.first + kid.k as usize);
                if level == 0 {
                    let haversine = q.haversine(kid.rect.min_lat, kid.rect.min_lon);
                    if haversine <= kth {
                        best.offer(Neighbour { row: i, haversine });
                    }
                } else if let Some(child) = stack.get_mut(top + 1) {
                    child.load(self, q, level - 1, i, &kid.rect);
                    top += 1;
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{Best, Neighbour, PlaceIndex, Query, Rect, cos_lb, down, sin_lb};
        use std::vec::Vec;
        use teigeodata_core::{E6, Point};

        fn xorshift(s: &mut u64) -> u64 {
            *s ^= *s << 13;
            *s ^= *s >> 7;
            *s ^= *s << 17;
            *s
        }

        /// A radius: exactly the rows of `brute` (sorted by haversine, then row) within it, up
        /// to 12, whatever the cap's rank; none below the nearest; all 12 beyond the farthest.
        fn radius_is_exact(ix: &PlaceIndex<'_>, at: Point<E6>, brute: &[(f64, usize)], t: usize) {
            for cap in [
                brute.get(t % 20).map_or(0.0, |b| b.0),
                brute.first().map_or(0.0, |b| b.0 * 0.5),
                f64::from(u32::try_from(t).unwrap_or(0)) * 1e-9,
                1.0,
            ] {
                let want: Vec<(u64, usize)> = brute
                    .iter()
                    .filter(|b| b.0 <= cap)
                    .take(12)
                    .map(|b| (b.0.to_bits(), b.1))
                    .collect();
                let mut out = [Neighbour::default(); 12];
                let n = ix.nearest_within(at, cap, &mut out);
                let got: Vec<(u64, usize)> = out
                    .iter()
                    .take(n)
                    .map(|g| (g.haversine.to_bits(), g.row))
                    .collect();
                assert_eq!(got, want, "query {t} cap {cap}");
            }
        }

        /// With the test heap of 8 entries most searches overflow into the depth-first path:
        /// both paths together must still give the exact nearest rows.
        #[test]
        fn the_overflow_path_is_exact() {
            use crate::places::{self, PlaceColumns, Places};
            use crate::{Encoding, Kind, Pack, Writer};
            use teigeodata_core::{PlaceKind, hilbert};

            let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                reason = "grid units"
            )]
            let mut pts: Vec<Point<E6>> = (0..if cfg!(miri) { 300 } else { 4000 })
                .filter_map(|i| {
                    let (a, b) = (xorshift(&mut s), xorshift(&mut s));
                    let (lat, lon) = if i % 2 == 0 {
                        (
                            (a % 180_000_001) as i32 - 90_000_000,
                            (b % 360_000_001) as i32 - 180_000_000,
                        )
                    } else {
                        (
                            27_000_000 + (a % 500_000) as i32,
                            85_000_000 + (b % 500_000) as i32,
                        )
                    };
                    Point::from_units(lat, lon)
                })
                .collect();
            pts.sort_by_key(|p| (hilbert(*p), *p));
            let kinds = std::vec![PlaceKind::Settlement; pts.len()];
            let pb = places::encode(&PlaceColumns {
                points: &pts,
                kinds: &kinds,
                ..PlaceColumns::default()
            })
            .unwrap_or_default();
            let n = pts.len() as u64;
            for node in [4, 16] {
                let ib = super::super::encode(&pts, node).unwrap_or_default();
                let mut w =
                    Writer::new("np", "places", "2026.11.0").unwrap_or_else(|e| panic!("{e}"));
                w.section_bytes(Kind::PLACES, Encoding::TABLE, n, &pb)
                    .unwrap_or_else(|e| panic!("{e}"));
                w.section_bytes(Kind::PLACE_INDEX, Encoding::TABLE, n, &ib)
                    .unwrap_or_else(|e| panic!("{e}"));
                let file = w
                    .layout(false)
                    .and_then(|l| l.assemble(&[&pb, &ib]))
                    .unwrap_or_else(|e| panic!("{e}"));
                let pack = Pack::open(&file).unwrap_or_else(|e| panic!("{e}"));
                let section = |k| pack.section(k).unwrap_or_else(|| panic!("section"));
                let places = Places::open(&section(Kind::PLACES)).unwrap_or_else(|e| panic!("{e}"));
                let ix = PlaceIndex::open(&section(Kind::PLACE_INDEX), places)
                    .unwrap_or_else(|e| panic!("{e}"));
                ix.check().unwrap_or_else(|e| panic!("{e}"));
                for t in 0..if cfg!(miri) { 3 } else { 200 } {
                    let at = pts
                        .get((t * 37) % pts.len())
                        .copied()
                        .unwrap_or_else(|| panic!("a point"));
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_possible_wrap,
                        reason = "small jitter"
                    )]
                    let lon = at.lon() + (xorshift(&mut s) % 20_000) as i32 - 10_000;
                    let at =
                        Point::<E6>::from_units(at.lat(), lon.clamp(-180_000_000, 180_000_000))
                            .unwrap_or(at);
                    let q = Query::new(at);
                    let mut brute: Vec<(f64, usize)> = pts
                        .iter()
                        .enumerate()
                        .map(|(r, p)| (q.haversine(p.lat(), p.lon()), r))
                        .collect();
                    brute.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                    let mut out = [Neighbour::default(); 12];
                    assert_eq!(ix.nearest(at, &mut out), 12);
                    for (got, want) in out.iter().zip(&brute) {
                        assert_eq!(
                            (got.haversine.to_bits(), got.row),
                            (want.0.to_bits(), want.1),
                            "node {node} query {t}"
                        );
                    }
                    radius_is_exact(&ix, at, &brute, t);
                }
            }
        }

        #[test]
        fn the_series_are_lower_bounds() {
            for i in 0..=10_000 {
                let y = f64::from(i) / 10_000.0 * core::f64::consts::FRAC_PI_2;
                // Within float rounding (a few ulps, which `SLACK` covers in the bound).
                let ulps = 1.0 + 4.0 * f64::EPSILON;
                assert!(sin_lb(y) <= libm::sin(y) * ulps, "sin {y}");
                assert!(sin_lb(y) >= 0.0);
                let c = libm::cos(y) * ulps + f64::EPSILON;
                assert!(cos_lb(y) <= c && cos_lb(-y) <= c, "cos {y}");
            }
            // Tight where it matters: within 0.02% of sin at a quarter turn.
            assert!(sin_lb(core::f64::consts::FRAC_PI_2) > 0.9998);
            for b in [0.0, 1e-30, 1e-9, 0.1, 0.5, 1.0] {
                assert!(f64::from(down(b)) <= b);
            }
        }

        /// The box bound never exceeds the exact haversine to a point inside the box, for
        /// boxes anywhere, including at the poles and across the antimeridian from the query.
        #[test]
        fn the_box_bound_is_a_lower_bound() {
            let mut s: u64 = 0x2545_f491_4f6c_dd1d;
            let mut next = |m: u32| {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_possible_wrap,
                    reason = "below m, which fits an i32"
                )]
                let v = (xorshift(&mut s) % u64::from(m)) as i32;
                v
            };
            for _ in 0..if cfg!(miri) { 200 } else { 200_000 } {
                let lat = |n: &mut dyn FnMut(u32) -> i32| n(180_000_001) - 90_000_000;
                let lon = |n: &mut dyn FnMut(u32) -> i32| n(360_000_001) - 180_000_000;
                let (la, lb) = (lat(&mut next), lat(&mut next));
                let (oa, ob) = (lon(&mut next), lon(&mut next));
                let b = Rect {
                    min_lat: la.min(lb),
                    max_lat: la.max(lb),
                    min_lon: oa.min(ob),
                    max_lon: oa.max(ob),
                };
                let Some(at) = Point::<E6>::from_units(lat(&mut next), lon(&mut next)) else {
                    continue;
                };
                let q = Query::new(at);
                let bound = q.bound(&b);
                let span = |a: i32, z: i32, n: &mut dyn FnMut(u32) -> i32| {
                    a + n(u32::try_from(z - a).unwrap_or(0).saturating_add(1))
                };
                let inside = (
                    span(b.min_lat, b.max_lat, &mut next),
                    span(b.min_lon, b.max_lon, &mut next),
                );
                for (plat, plon) in [
                    (b.min_lat, b.min_lon),
                    (b.max_lat, b.max_lon),
                    (b.min_lat, b.max_lon),
                    (b.max_lat, b.min_lon),
                    (b.min_lat, at.lon().clamp(b.min_lon, b.max_lon)),
                    (at.lat().clamp(b.min_lat, b.max_lat), b.min_lon),
                    inside,
                ] {
                    let h = q.haversine(plat, plon);
                    assert!(
                        bound <= h,
                        "bound {bound} > {h} at {plat},{plon} for {b:?} from {at:?}"
                    );
                }
            }
        }

        /// The integer gates set from a k-th distance `h` admit every point at distance `h`
        /// (so every box holding one), including across a pole and near the antipode.
        #[test]
        fn the_gates_admit_the_kth_distance() {
            let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
            let mut next = |m: u32| {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_possible_wrap,
                    reason = "below m, which fits an i32"
                )]
                let v = (xorshift(&mut s) % u64::from(m)) as i32;
                v
            };
            for i in 0..if cfg!(miri) { 200 } else { 300_000 } {
                // A third of the cases near a pole, where the gates are tightest.
                // A third of those exactly on the pole: the k-th circle then just reaches the
                // pole, the gate's edge case (sin r = cos φ).
                let polar = |n: &mut dyn FnMut(u32) -> i32| {
                    let v = if n(3) == 0 {
                        90_000_000
                    } else {
                        90_000_000 - n(2_000_000)
                    };
                    if n(2) == 0 { v } else { -v }
                };
                let lat = |n: &mut dyn FnMut(u32) -> i32| n(180_000_001) - 90_000_000;
                let lon = |n: &mut dyn FnMut(u32) -> i32| n(360_000_001) - 180_000_000;
                let (qa, pa) = if i % 3 == 0 {
                    (polar(&mut next), polar(&mut next))
                } else {
                    (lat(&mut next), lat(&mut next))
                };
                let (Some(at), plon) =
                    (Point::<E6>::from_units(qa, lon(&mut next)), lon(&mut next))
                else {
                    continue;
                };
                let q = Query::new(at);
                let h = q.haversine(pa, plon);
                let mut out = [Neighbour::default(); 1];
                let mut best = Best::new(&mut out, &q, f64::INFINITY);
                best.offer(Neighbour {
                    row: 0,
                    haversine: h,
                });
                let point = Rect {
                    min_lat: pa,
                    max_lat: pa,
                    min_lon: plon,
                    max_lon: plon,
                };
                assert!(
                    best.open(q.gaps(&point)),
                    "{pa},{plon} at {h} shut out from {at:?}: gates {} {}",
                    best.lat_gate,
                    best.lon_gate
                );
            }
        }
    }
}
/// Encodes a `PLACE_INDEX` payload for `points`, the PLACES points in their row order, with
/// `node` children per box (feature `write`). Its item count is the number of points.
///
/// # Errors
/// [`Error::Write`] if the node size is out of range or the tree too deep.
#[cfg(feature = "write")]
pub fn encode(
    points: &[teigeodata_core::Point<teigeodata_core::E6>],
    node: usize,
) -> Result<alloc::vec::Vec<u8>, Error> {
    let items: alloc::vec::Vec<Rect> = points
        .iter()
        .map(|p| Rect::point(p.lat(), p.lon()))
        .collect();
    crate::rtree::encode(&items, node, false)
}
