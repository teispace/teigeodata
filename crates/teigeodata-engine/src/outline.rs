//! A place's extent: the box of its outline and the outline itself at a level of detail
//! (feature `boundaries`, D-194).
//!
//! A place's outline comes from the first layer, in precedence order, whose pack has a row for
//! its ID with an outline: a city part may hold a province only as an ancestor stub, with no
//! outline, while the country pack holds the province's outline (D-191).

use core::ops::Range;

use teigeodata_core::{E7, PlaceId, Point};
use teigeodata_format::Kind;
use teigeodata_format::arcs::{Arcs, PINNED};
use teigeodata_format::place_index::Rect;
use teigeodata_format::poly_index::{PolyIndex, polygon_box};
use teigeodata_format::polygons::Polygons;

use crate::{Engine, Error, LayerId, Place};

/// A place's outline, written by [`Engine::boundary`]. Reused across calls, it allocates only
/// when an outline is larger than every one before.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outline {
    /// Every vertex of every ring, ring after ring; each ring is closed (its first point again
    /// at its end).
    pub points: Vec<Point<E7>>,
    /// The rings in stored order: each part's outer ring, then that part's holes.
    pub rings: Vec<Ring>,
    /// How far the outline may stray from the full-resolution one, in centimetres: 0 at level
    /// of detail 0.
    pub tolerance_cm: u32,
}

/// One ring of an [`Outline`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ring {
    /// The part (one piece of a multi-part outline, such as an island) it belongs to.
    pub part: u32,
    /// Whether it is a hole in its part rather than the part's outer ring.
    pub hole: bool,
    /// Its vertices in [`Outline::points`].
    pub points: Range<usize>,
}

/// The outline views of one layer.
struct Shape<'v, 'a> {
    polygons: &'v Polygons<'a>,
    arcs: &'v Arcs<'a>,
    index: Option<&'v PolyIndex<'a>>,
    polygon: usize,
}

impl Engine {
    /// The layer holding an outline of `id`, by precedence, and that outline's views.
    fn shape(&self, id: PlaceId) -> Result<Option<(usize, Shape<'_, '_>)>, Error> {
        for (i, l) in self.layers.iter().enumerate() {
            if !l.may_hold(id) {
                continue;
            }
            l.ensure(Kind::IDMAP)?;
            let v = l.views();
            let (Some(row), Some(polygons), Some(arcs)) =
                (v.ids.as_ref().and_then(|m| m.row(id)), &v.polygons, &v.arcs)
            else {
                continue;
            };
            l.ensure(Kind::POLYGONS)?;
            let Some(polygon) = u32::try_from(row).ok().and_then(|r| polygons.polygon_of(r)) else {
                continue;
            };
            let index = match &v.poly_index {
                Some(ix) => {
                    l.ensure(Kind::POLY_INDEX)?;
                    Some(ix)
                }
                None => None,
            };
            return Ok(Some((
                i,
                Shape {
                    polygons,
                    arcs,
                    index,
                    polygon,
                },
            )));
        }
        Ok(None)
    }

    /// The box of `place`'s outline on the 1e-6° grid, rounded outward so it always holds the
    /// outline: what a map needs to frame a town or a district. O(index depth) from the stored
    /// box; a pack without the box index reads the outline's vertices instead. `None` for a
    /// place without an outline (a point place: its point is its extent) or where no layer
    /// with outlines (`boundaries`) holds it.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt.
    pub fn bbox_of(&self, place: &Place) -> Result<Option<Rect>, Error> {
        let Some((i, s)) = self.shape(place.id)? else {
            return Ok(None);
        };
        if let Some(ix) = s.index {
            return Ok(ix.bbox(s.polygon));
        }
        self.layers.get(i).ok_or(Error::Range)?.ensure(Kind::ARCS)?;
        Ok(polygon_box(s.polygons, s.arcs, s.polygon))
    }

    /// `place`'s outline at level of detail `lod` into `out`, from the first layer by
    /// precedence that has one; returns that layer, or `None` (and an empty `out`) without
    /// one. Level 0 is full resolution; levels 1 to 6 are coarser, each within its tolerance
    /// ([`Outline::tolerance_cm`]); a higher level reads as 6, and a pack stored at full
    /// resolution only reads as 0. Rings keep their topology: neighbours share their borders
    /// at every level. O(vertices of the outline's arcs).
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt.
    pub fn boundary(
        &self,
        place: &Place,
        lod: u8,
        out: &mut Outline,
    ) -> Result<Option<LayerId>, Error> {
        out.points.clear();
        out.rings.clear();
        out.tolerance_cm = 0;
        let Some((i, s)) = self.shape(place.id)? else {
            return Ok(None);
        };
        let l = self.layers.get(i).ok_or(Error::Range)?;
        l.ensure(Kind::ARCS)?;
        let lod = if s.arcs.has_levels() {
            lod.min(PINNED - 1)
        } else {
            0
        };
        out.tolerance_cm = s.arcs.tolerance_cm(lod).unwrap_or(0);
        for (k, part) in s.polygons.parts(s.polygon).unwrap_or(0..0).enumerate() {
            let part_no = u32::try_from(k).map_err(|_| Error::Range)?;
            let rings = s.polygons.rings(part).unwrap_or(0..0);
            let first = rings.start;
            for ring in rings {
                let start = out.points.len();
                out.points.extend(s.polygons.ring(s.arcs, ring, lod));
                out.rings.push(Ring {
                    part: part_no,
                    hole: ring != first,
                    points: start..out.points.len(),
                });
            }
        }
        Ok(Some(l.id))
    }
}
