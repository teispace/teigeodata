//! A place's extent: the box of its outline and the outline itself at a level of detail
//! (feature `boundaries`, D-194).
//!
//! A place's outline comes from the first layer, in precedence order, whose pack has a row for
//! its ID with an outline: a city part may hold a province only as an ancestor stub, with no
//! outline, while the country pack holds the province's outline (D-191).

use core::ops::Range;

use teigeodata_core::{E6, E7, PlaceId, Point};
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

    /// How many vertices [`Engine::boundary`] reads for `place` at level of detail `lod`, or
    /// `None` without an outline: counted from the arcs' levels, no coordinate decoded, so a
    /// caller picks the level that fits a bound before reading it.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt.
    pub fn boundary_vertices(&self, place: &Place, lod: u8) -> Result<Option<usize>, Error> {
        let Some((i, s)) = self.shape(place.id)? else {
            return Ok(None);
        };
        self.layers.get(i).ok_or(Error::Range)?.ensure(Kind::ARCS)?;
        let lod = if s.arcs.has_levels() {
            lod.min(PINNED - 1)
        } else {
            0
        };
        let mut n = 0_usize;
        for part in s.polygons.parts(s.polygon).unwrap_or(0..0) {
            for ring in s.polygons.rings(part).unwrap_or(0..0) {
                // Consecutive arcs share their joint, which the ring reads once.
                let (mut vertices, mut arcs) = (0_usize, 0_usize);
                for (a, _) in s.polygons.refs(ring) {
                    vertices += s.arcs.count(a, lod);
                    arcs += 1;
                }
                n += vertices.saturating_sub(arcs.saturating_sub(1));
            }
        }
        Ok(Some(n))
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

impl Outline {
    /// Writes the outline as an RFC 7946 geometry: a `Polygon`, or a `MultiPolygon` when it
    /// has more than one part, each outer ring counterclockwise and each hole clockwise (the
    /// right-hand rule, §3.1.6), coordinates longitude first with the seven decimals the pack
    /// stores. The servers' answers and every binding's `boundary` are these bytes. O(vertices).
    pub fn write_geojson(&self, out: &mut Vec<u8>) {
        let shape = self;
        let parts = shape
            .rings
            .iter()
            .map(|r| r.part)
            .max()
            .map_or(0, |p| p + 1);
        let multi = parts > 1;
        out.extend_from_slice(if multi {
            b"{\"type\":\"MultiPolygon\",\"coordinates\":["
        } else {
            b"{\"type\":\"Polygon\",\"coordinates\":["
        });
        let mut part = None;
        for (i, ring) in shape.rings.iter().enumerate() {
            let pts = shape.points.get(ring.points.clone()).unwrap_or_default();
            if multi && part != Some(ring.part) {
                if part.is_some() {
                    out.extend_from_slice(b"],");
                }
                out.push(b'[');
                part = Some(ring.part);
            } else if i > 0 {
                out.push(b',');
            }
            // Twice the signed area (shoelace, degrees): positive for counterclockwise.
            let area: f64 = pts
                .windows(2)
                .map(|w| match w {
                    [a, b] => a.lon_degrees() * b.lat_degrees() - b.lon_degrees() * a.lat_degrees(),
                    _ => 0.0,
                })
                .sum();
            let reverse = (area > 0.0) == ring.hole;
            out.push(b'[');
            let mut write_point = |j: usize, p: &Point<E7>| {
                if j > 0 {
                    out.push(b',');
                }
                out.push(b'[');
                e7(p.lon(), out);
                out.push(b',');
                e7(p.lat(), out);
                out.push(b']');
            };
            if reverse {
                for (j, p) in pts.iter().rev().enumerate() {
                    write_point(j, p);
                }
            } else {
                for (j, p) in pts.iter().enumerate() {
                    write_point(j, p);
                }
            }
            out.push(b']');
        }
        if multi && part.is_some() {
            out.push(b']');
        }
        out.extend_from_slice(b"]}");
    }
}

/// Writes a place's `point` as an RFC 7946 `Point` geometry, longitude first, with seven
/// decimals as an outline's: the extent of a place with no outline.
pub fn write_point_geojson(point: Point<E6>, out: &mut Vec<u8>) {
    out.extend_from_slice(b"{\"type\":\"Point\",\"coordinates\":[");
    // A 1e-6° unit is ten 1e-7° units; ±1.8e9 still fits an `i32`.
    e7(point.lon().saturating_mul(10), out);
    out.push(b',');
    e7(point.lat().saturating_mul(10), out);
    out.extend_from_slice(b"]}");
}

/// Writes `units` of the 1e-7° grid as degrees with 7 decimals, exactly: what `{:.7}` writes
/// for `units / 1e7`, from the integer alone (an outline writes tens of thousands of them).
fn e7(units: i32, out: &mut Vec<u8>) {
    if units < 0 {
        out.push(b'-');
    }
    let abs = units.unsigned_abs();
    let (whole, fraction) = (abs / 10_000_000, abs % 10_000_000);
    let mut digits = [0_u8; 10];
    let mut start = digits.len();
    let mut rest = whole;
    loop {
        start -= 1;
        if let Some(d) = digits.get_mut(start) {
            // `rest % 10` is a digit.
            *d = b'0' + (rest % 10) as u8;
        }
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    out.extend_from_slice(digits.get(start..).unwrap_or_default());
    out.push(b'.');
    let mut decimals = [b'0'; 7];
    let mut left = fraction;
    for d in decimals.iter_mut().rev() {
        *d = b'0' + (left % 10) as u8;
        left /= 10;
    }
    out.extend_from_slice(&decimals);
}

#[cfg(test)]
mod tests {
    use teigeodata_core::{E7, Point};

    /// `e7` writes what `{:.7}` of the degrees wrote, for every kind of value.
    #[test]
    fn e7_is_the_degrees_to_seven_decimals() {
        let mut values = vec![
            0,
            1,
            -1,
            9_999_999,
            -9_999_999,
            10_000_000,
            -10_000_000,
            1_800_000_000,
            -1_800_000_000,
            900_000_000,
            -900_000_000,
            123_456_789,
            -5,
        ];
        // A fixed LCG over the whole longitude range.
        let mut x = 0x2545_f491_u64;
        for _ in 0..200_000 {
            x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let v = i64::try_from(x >> 33).unwrap_or(0) % 3_600_000_001 - 1_800_000_000;
            values.push(i32::try_from(v).unwrap_or(0));
        }
        let mut out = Vec::new();
        for v in values {
            let p = Point::<E7>::from_units(0, v).unwrap_or_else(|| panic!("{v} on the grid"));
            out.clear();
            super::e7(v, &mut out);
            assert_eq!(
                String::from_utf8_lossy(&out),
                format!("{:.7}", p.lon_degrees()),
                "{v}"
            );
        }
    }
}
