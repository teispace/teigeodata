//! `POLY_INDEX` (kind 9): a packed Hilbert R-tree over the boxes of the
//! [POLYGONS](crate::polygons) (D-088, B10).
//!
//! The same tree as [`PLACE_INDEX`](crate::place_index), one implementation for both (the
//! columns are its 1 to 3), with the leaves stored: each polygon's box as four 16-bit offsets
//! inside its level-0 box, rounded outward, ahead of the level-0 boxes in
//! [`BOXES`]. A polygon's box is the bounds of every vertex of every ring at full resolution,
//! rounded outward from the 1e-7° grid to the index's 1e-6°. The leaves are the polygons in
//! their POLYGONS order, which groups them by admin level, coarse to fine, and orders each level
//! by PLACES row (Hilbert order); queries take a range of polygons, usually one group, and skip
//! every subtree outside it, so a country's box never slows a query for districts.
//!
//! Its item count is the number of polygons. [`PolyIndex::open`] checks the structure;
//! [`PolyIndex::check`] also recomputes every polygon's box from the arcs and checks that every
//! stored offset is exactly the writer's, so a checked index is canonical. Queries never panic,
//! never allocate and only return polygons below the count. As with `PLACE_INDEX`, answers are
//! exact only on a checked index.

use core::ops::{ControlFlow, Range};

use teigeodata_core::{E7, Point};

use crate::arcs::Arcs;
use crate::polygons::Polygons;
use crate::rtree::{Leaves, Tree};
use crate::{Error, Kind, Section};

pub use crate::rtree::{BOXES, MAX_NODE, MIN_NODE, NODE_SIZE, QUANTA, ROOT, Rect};

/// The `POLY_INDEX` section of an open pack.
#[derive(Clone, Copy, Debug)]
pub struct PolyIndex<'a> {
    tree: Tree<'a>,
}

/// The box of polygon `p` on the index's grid: the bounds of every ring's vertices at full
/// resolution, rounded outward. `None` if the polygon has no vertex. O(vertices of `p`).
#[must_use]
pub fn polygon_box(polygons: &Polygons<'_>, arcs: &Arcs<'_>, p: usize) -> Option<Rect> {
    let mut r = Rect::EMPTY;
    for ring in polygons.all_rings(p) {
        for v in polygons.ring(arcs, ring, 0) {
            r.grow(&Rect::around_e7(v.lat(), v.lon()));
        }
    }
    (r.min_lat <= r.max_lat).then_some(r)
}

impl<'a> PolyIndex<'a> {
    /// Opens the section over `polygons`; O(columns + blocks).
    ///
    /// # Errors
    /// [`Error::Payload`] if the section is not a `POLY_INDEX` column table, its item count is
    /// not the number of polygons, the node size is out of range, or a column is malformed or
    /// of the wrong length.
    pub fn open(section: &Section<'a>, polygons: &Polygons<'a>) -> Result<Self, Error> {
        Tree::open(section, Kind::POLY_INDEX, &Leaves::Stored(polygons.len()))
            .map(|tree| Self { tree })
    }

    /// The number of polygons.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.tree.len()
    }

    /// Whether there are no polygons.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.tree.len() == 0
    }

    /// Children per box.
    #[must_use]
    pub const fn node_size(&self) -> usize {
        self.tree.node()
    }

    /// The number of box levels above the polygons.
    #[must_use]
    pub const fn depth(&self) -> usize {
        self.tree.depth
    }

    /// Polygon `p`'s stored box: its exact box rounded outward by at most two quanta of each
    /// parent box, so it always holds the polygon. O(depth): one decode per level, no vertex
    /// read, no allocation. `None` past the end.
    #[must_use]
    pub fn bbox(&self, p: usize) -> Option<Rect> {
        self.tree.item_box(p)
    }

    /// Calls `visit` with every polygon in `polygons` (a range of indices, such as a
    /// [group](Polygons::group)) whose stored box meets `query`, edges included, in an
    /// unspecified order; stops early when `visit` breaks. Stored boxes are rounded outward, so
    /// this is a superset of the polygons whose exact box meets the query, never a subset. A
    /// query with `min_lon > max_lon` crosses the antimeridian. Allocation-free.
    pub fn intersecting<F>(&self, query: Rect, polygons: Range<usize>, visit: F) -> ControlFlow<()>
    where
        F: FnMut(usize) -> ControlFlow<()>,
    {
        self.tree.within(query, polygons, visit)
    }

    /// The number of tree nodes [`PolyIndex::intersecting`] reads for this query: a cost
    /// measure for benchmarks (one node is one decode of up to [node size](Self::node_size)
    /// boxes).
    #[must_use]
    pub fn nodes_read(&self, query: Rect, polygons: Range<usize>) -> usize {
        let mut nodes = 0;
        let _ =
            self.tree
                .within_counted(query, polygons, |_| ControlFlow::Continue(()), &mut nodes);
        nodes
    }

    /// Calls `visit` with every polygon in `range` that holds `at`, exactly
    /// ([`Polygons::contains`], its rule for points on a border): the index's candidates, each
    /// tested on its rings. Allocation-free; stops early when `visit` breaks.
    pub fn containing<F>(
        &self,
        polygons: &Polygons<'a>,
        arcs: &Arcs<'a>,
        at: Point<E7>,
        range: Range<usize>,
        mut visit: F,
    ) -> ControlFlow<()>
    where
        F: FnMut(usize) -> ControlFlow<()>,
    {
        // The cell of the point `contains` tests: longitude 180 is read as −180.
        let lon = if at.lon() == 1_800_000_000 {
            -1_800_000_000
        } else {
            at.lon()
        };
        let cell = Rect::around_e7(at.lat(), lon);
        self.tree.within(cell, range, |p| {
            if polygons.contains(arcs, p, at) {
                visit(p)
            } else {
                ControlFlow::Continue(())
            }
        })
    }

    /// Checks the whole tree against the polygons' boxes, recomputed from `arcs`: the root is
    /// exactly their bounds and every stored offset is exactly the writer's. O(vertices); no
    /// allocation.
    ///
    /// # Errors
    /// [`Error::Payload`] naming [`ROOT`] or [`BOXES`].
    pub fn check(&self, polygons: &Polygons<'a>, arcs: &Arcs<'a>) -> Result<(), Error> {
        if polygons.len() != self.tree.len() {
            return Err(Tree::fail(
                Kind::POLY_INDEX,
                0,
                "not the polygons it was opened on",
            ));
        }
        self.tree.check(|p| {
            polygon_box(polygons, arcs, p)
                .ok_or_else(|| Tree::fail(Kind::POLY_INDEX, BOXES, "a polygon without vertices"))
        })
    }
}

/// Encodes a `POLY_INDEX` payload over `boxes`, each polygon's [box](polygon_box) in POLYGONS
/// order, with `node` children per box (feature `write`). Its item count is the number of
/// polygons.
///
/// # Errors
/// [`Error::Write`] if the node size is out of range, a box is empty or the tree too deep.
#[cfg(feature = "write")]
pub fn encode(boxes: &[Rect], node: usize) -> Result<alloc::vec::Vec<u8>, Error> {
    crate::rtree::encode(boxes, node, true)
}
