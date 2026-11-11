//! Queries across layers: routing, the precedence rules for IDs, and the k-way merges (the
//! engine note, §3.4 and §3.5).
//!
//! The rule every query follows: a place `(layer, row)` is **visible** only if no layer above it
//! knows its ID (as a current place, a redirect or a tombstone). The winning layer for an ID
//! decides everything about it, so a stale copy below can never leak into an answer. Answers are
//! ordered by a key that depends only on the place (distance or score, then ID), so they do not
//! depend on how places are split into packs or in which order packs were added.

use core::ops::{ControlFlow, Range};

use teigeodata_core::{E6, E7, PlaceId, PlaceKind, Point};
use teigeodata_format::Kind;
#[cfg(feature = "elevation")]
use teigeodata_format::elevation::HeightSource;
use teigeodata_format::idmap::IdMap;
use teigeodata_format::place_index::{Neighbour, PlaceIndex, Rect, haversine_of};
use teigeodata_format::redirects::Fate;
#[cfg(feature = "admin-index")]
use teigeodata_format::region::Region;
#[cfg(feature = "search")]
use teigeodata_names::KeyBuf;

#[cfg(feature = "dem")]
pub use teigeodata_format::dem::Source as DemSource;

use crate::{Engine, Error, LayerId};

/// A place in an answer, read from the layer that won its ID.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    /// Its stable ID. For [`Engine::get`] of a retired ID, the survivor's.
    pub id: PlaceId,
    /// The layer it came from: its tier ([`Engine::tier`]).
    pub layer: LayerId,
    /// Its PLACES row in that layer.
    pub row: u32,
    /// Its point on the 1e-6° grid.
    pub point: Point<E6>,
    /// Its kind, `None` for a code this reader does not know.
    pub kind: Option<PlaceKind>,
    /// Its admin level (0 for a country or none).
    pub admin_level: u8,
    /// Its flag bits (`places::FLAG_FORMER`).
    pub flags: u32,
}

/// A terrain height and where it came from, from [`Engine::elevation_at`] (D-195).
#[cfg(feature = "dem")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Height {
    /// Metres above the datum.
    pub meters: f64,
    /// The DEM product it was read from; `None` for a pack that does not state it.
    pub source: Option<DemSource>,
    /// The DEM's sample spacing north to south in metres (about 30.9 for 1″); east to west it
    /// narrows with the cosine of the latitude.
    pub resolution_m: f64,
    /// The vertical datum.
    pub datum: Datum,
    /// The layer it came from.
    pub layer: LayerId,
}

/// A vertical datum.
#[cfg(feature = "dem")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Datum {
    /// Orthometric heights above the EGM2008 geoid.
    Egm2008,
}

#[cfg(feature = "dem")]
impl Datum {
    /// Its name (`EGM2008`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Egm2008 => "EGM2008",
        }
    }
}

#[cfg(feature = "dem")]
impl Height {
    fn of(meters: f64, grid: &teigeodata_format::dem::Grid, layer: LayerId) -> Self {
        Self {
            meters,
            source: DemSource::from_code(grid.source),
            resolution_m: f64::from(grid.lat_step) * teigeodata_format::dem::METRES_PER_DECI_ARCSEC,
            // The reader refuses a grid on any other datum (`Dem::check`, D-089).
            datum: Datum::Egm2008,
            layer,
        }
    }
}

/// A place near a point, from [`Engine::reverse`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Near {
    /// The place.
    pub place: Place,
    /// The great-circle distance in metres.
    pub meters: f64,
    /// The initial bearing from the place to the point asked about, in degrees clockwise from
    /// true north, `[0, 360)` (TG-19): "3.2 km at 45° from Pokhara".
    pub bearing_deg: f64,
    /// Whether the point lies inside the place's outline: `None` as `reverse` answers and for a
    /// place without an outline; [`Engine::mark_inside`] sets it (TG-19).
    pub inside: Option<bool>,
}

/// A place matching a name, from [`Engine::search`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// The place.
    pub place: Place,
    /// Its score: the best match bonus plus its importance (one formula for every pack, D-052).
    pub score: i32,
}

#[derive(Clone, Copy, Debug)]
struct NearCand {
    haversine: f64,
    id: u64,
    layer: u32,
    row: u32,
}

#[cfg(feature = "search")]
#[derive(Clone, Copy, Debug)]
pub(crate) struct HitCand {
    pub(crate) score: i32,
    pub(crate) id: u64,
    pub(crate) layer: u32,
    pub(crate) row: u32,
}

#[cfg(feature = "search")]
#[derive(Clone, Copy, Debug)]
pub(crate) struct PrefixCand {
    pub(crate) importance: u8,
    pub(crate) id: u64,
    pub(crate) layer: u32,
    pub(crate) row: u32,
}

/// Caller-owned working memory for queries. Buffers grow to the largest query seen and are
/// reused, so a warm query does not allocate (M1). One per thread; [`Default`] is empty.
#[derive(Debug, Default)]
pub struct Scratch {
    /// A decoded DEM tile, allocated on the first terrain query (19 KiB).
    #[cfg(feature = "dem")]
    dem: Option<Box<teigeodata_format::dem::DemScratch>>,
    neighbours: Vec<Neighbour>,
    near: Vec<NearCand>,
    pub(crate) segs: Vec<Range<usize>>,
    pub(crate) heads: Vec<usize>,
    #[cfg(feature = "admin-index")]
    regions: Vec<u32>,
    /// The units holding a point, for [`Engine::mark_inside`].
    #[cfg(feature = "admin-index")]
    units: Vec<Place>,
    #[cfg(feature = "search")]
    pub(crate) hits: Vec<HitCand>,
    #[cfg(feature = "search")]
    pub(crate) rows: Vec<(u32, i32)>,
    #[cfg(feature = "search")]
    pub(crate) prefix: Vec<PrefixCand>,
    /// The substring tier's best rows over every layer (D-199).
    #[cfg(feature = "substring")]
    pub(crate) substring: Vec<PrefixCand>,
    #[cfg(feature = "search")]
    pub(crate) loose: String,
    /// The rows a layer's names match the query's context with, sorted.
    #[cfg(feature = "search")]
    pub(crate) context: Vec<u32>,
    /// A query form with a native generic replaced by a Latin spelling.
    #[cfg(feature = "search")]
    pub(crate) form: String,
    /// A phonetic key and its buffers.
    #[cfg(feature = "search")]
    pub(crate) pkey: String,
    #[cfg(feature = "search")]
    pub(crate) keybuf: KeyBuf,
}

/// Redirect hops [`Engine::get`] follows across layers (the forge flattens chains in a pack, so
/// more than one hop means layers of different releases).
const MAX_HOPS: usize = 4;

/// Distances below this are compared as `≤` when pruning, so float rounding of a lower bound
/// never prunes a layer holding a place at the k-th distance.
const SLACK: f64 = 1.0 - 1e-12;

pub(crate) fn index(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// The k-way merge of sorted segments of `items`: emits the first `k` in `less` order.
pub(crate) fn merge<T>(
    items: &[T],
    segs: &[Range<usize>],
    heads: &mut Vec<usize>,
    k: usize,
    less: impl Fn(&T, &T) -> bool,
    mut emit: impl FnMut(&T) -> Result<(), Error>,
) -> Result<(), Error> {
    heads.clear();
    heads.extend(segs.iter().map(|r| r.start));
    for _ in 0..k {
        let mut best: Option<(usize, &T)> = None;
        for (j, (r, &h)) in segs.iter().zip(heads.iter()).enumerate() {
            if h < r.end
                && let Some(it) = items.get(h)
                && best.is_none_or(|(_, b)| less(it, b))
            {
                best = Some((j, it));
            }
        }
        let Some((j, it)) = best else { break };
        if let Some(h) = heads.get_mut(j) {
            *h += 1;
        }
        emit(it)?;
    }
    Ok(())
}

fn near_less(a: &NearCand, b: &NearCand) -> bool {
    a.haversine
        .total_cmp(&b.haversine)
        .then(a.id.cmp(&b.id))
        .is_lt()
}

#[cfg(feature = "admin-index")]
/// Every region of `region` holding `at`, into `buf` (grown when a point is in more).
fn regions_into(region: &Region<'_>, at: Point<E7>, buf: &mut Vec<u32>) -> usize {
    if buf.len() < 16 {
        buf.resize(16, 0);
    }
    loop {
        let n = region.regions_at(at, buf);
        if n <= buf.len() {
            return n;
        }
        buf.resize(n, 0);
    }
}

/// The box (1e-6°) around `at` holding every point within `radius_m` metres of it, rounded
/// outward: the latitude band widened by the radius, the longitude by the radius over the
/// cosine of the band's latitude farthest from the equator; all longitudes near a pole. Across
/// the antimeridian `min_lon > max_lon`, as the index reads it.
#[cfg(feature = "admin-index")]
fn radius_box(at: Point<E7>, radius_m: u32) -> Rect {
    // Metres per degree of latitude on the mean sphere (R = 6 371 008.8 m).
    const M_PER_DEG: f64 = 111_195.08;
    let dlat = f64::from(radius_m) / M_PER_DEG;
    let lat = f64::from(at.lat()) * 1e-7;
    let lon = f64::from(at.lon()) * 1e-7;
    let (lo, hi) = ((lat - dlat).max(-90.0), (lat + dlat).min(90.0));
    let cos = lo.abs().max(hi.abs()).to_radians().cos();
    let e6 = |d: f64, up: bool| {
        let v = if up {
            (d * 1e6).ceil() + 1.0
        } else {
            (d * 1e6).floor() - 1.0
        };
        #[allow(
            clippy::cast_possible_truncation,
            reason = "clamped to the 1e-6° range first"
        )]
        let v = v.clamp(-180e6, 180e6) as i32;
        v
    };
    let (min_lat, max_lat) = (e6(lo, false).max(-90_000_000), e6(hi, true).min(90_000_000));
    let dlon = if cos > 1e-9 { dlat / cos } else { 360.0 };
    if dlon >= 180.0 {
        return Rect {
            min_lat,
            max_lat,
            min_lon: -180_000_000,
            max_lon: 180_000_000,
        };
    }
    let wrap = |d: f64| {
        if d > 180.0 {
            d - 360.0
        } else if d < -180.0 {
            d + 360.0
        } else {
            d
        }
    };
    Rect {
        min_lat,
        max_lat,
        min_lon: e6(wrap(lon - dlon), false),
        max_lon: e6(wrap(lon + dlon), true),
    }
}

/// Whether a layer's coverage box (1e-7°) can meet the query box (1e-6°); true without a box
/// and for boxes across the antimeridian, which are not pruned.
fn meets(bbox: Option<teigeodata_format::Bbox>, rect: &Rect) -> bool {
    let Some(b) = bbox else { return true };
    if b.min.lon() > b.max.lon() || rect.min_lon > rect.max_lon {
        return true;
    }
    let mut cover = Rect::around_e7(b.min.lat(), b.min.lon());
    cover.grow(&Rect::around_e7(b.max.lat(), b.max.lon()));
    cover.intersects(rect)
}

/// A lower bound on the haversine between `at` and any point of the box: from the latitude
/// gap alone (a path between two latitudes is at least their difference long).
fn lower_bound(bbox: Option<teigeodata_format::Bbox>, at: Point<E7>) -> f64 {
    let Some(b) = bbox else { return 0.0 };
    let lat = i64::from(at.lat());
    let gap = (i64::from(b.min.lat()) - lat)
        .max(lat - i64::from(b.max.lat()))
        .max(0);
    #[allow(clippy::cast_precision_loss, reason = "|gap| < 2^31")]
    let rad = gap as f64 * core::f64::consts::PI / 180.0 / 10_000_000.0;
    let s = (rad / 2.0).sin();
    s * s * SLACK
}

impl Engine {
    /// The place at `row` of layer `i`, as an answer.
    pub(crate) fn place(&self, i: usize, row: usize) -> Result<Option<Place>, Error> {
        let Some(l) = self.layers.get(i) else {
            return Ok(None);
        };
        l.ensure(Kind::PLACES)?;
        l.ensure(Kind::IDMAP)?;
        let v = l.views();
        let (Some(ids), Some(places)) = (&v.ids, &v.places) else {
            return Ok(None);
        };
        let (Some(id), Some(point)) = (ids.id(row), places.point(row)) else {
            return Ok(None);
        };
        Ok(Some(Place {
            id,
            layer: l.id,
            row: index(row),
            point,
            kind: places.kind(row),
            admin_level: places.admin_level(row).unwrap_or(0),
            flags: places.flags(row).unwrap_or(0),
        }))
    }

    /// The place at PLACES row `row` of layer `layer`, as that layer holds it, whatever the
    /// layers above say: for walking one pack in its stored (Hilbert) order, as `tgd export`
    /// does. `None` past the last row or for a layer the engine does not hold.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt.
    pub fn at_row(&self, layer: LayerId, row: u32) -> Result<Option<Place>, Error> {
        match self.layers.iter().position(|l| l.id == layer) {
            Some(i) => self.place(i, row as usize),
            None => Ok(None),
        }
    }

    /// Whether a layer above layer `i` knows `id`: then the place in layer `i` is hidden.
    pub(crate) fn claimed_above(&self, i: usize, id: PlaceId) -> Result<bool, Error> {
        for l in self.layers.get(..i).unwrap_or_default() {
            if !l.may_hold(id) {
                continue;
            }
            l.ensure(Kind::IDMAP)?;
            l.ensure(Kind::REDIRECTS)?;
            let v = l.views();
            if v.ids.as_ref().is_some_and(|m| m.row(id).is_some())
                || v.redirects.as_ref().is_some_and(|r| r.get(id).is_some())
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The terrain height at `at` in metres above EGM2008, with where it came from (its DEM
    /// product, sample spacing and datum, D-195), from the first layer covering the point
    /// whose DEM has samples around it (D-089, D-192): bilinear between the four samples
    /// around it, each within half the DEM's height step of its source (±1 m in shipped
    /// packs). `None` where no layer has terrain (the sea, outside every DEM). A warm query
    /// does not allocate; it decodes at most one tile, and the next query in that tile goes on
    /// from where it stopped.
    ///
    /// # Errors
    /// [`Error::MissingComponent`] if a layer covering the point has a DEM that is not
    /// installed; [`Error::Integrity`] if it is corrupt.
    #[cfg(feature = "dem")]
    pub fn elevation_at(&self, at: Point<E7>, s: &mut Scratch) -> Result<Option<Height>, Error> {
        for l in &self.layers {
            if !l.covers(at) {
                continue;
            }
            l.ensure(Kind::DEM)?;
            let Some(dem) = &l.views().dem else {
                continue;
            };
            let scratch = s.dem.get_or_insert_with(Box::default);
            if let Some((meters, grid)) = dem.height(at, scratch) {
                return Ok(Some(Height::of(meters, &grid, l.id)));
            }
        }
        Ok(None)
    }

    /// The elevation of `place` in whole metres above EGM2008, from its layer's `elevation`
    /// component (D-133); `None` where it is unknown or the layer has no heights.
    ///
    /// # Errors
    /// [`Error::MissingComponent`] if the layer has heights but they are not installed;
    /// [`Error::Integrity`] if they are corrupt.
    #[cfg(feature = "elevation")]
    pub fn elevation(&self, place: &Place) -> Result<Option<i16>, Error> {
        let Some(l) = self.layer(place.layer) else {
            return Ok(None);
        };
        l.ensure(Kind::ELEVATION)?;
        Ok(l.views()
            .heights
            .as_ref()
            .and_then(|h| h.get(place.row as usize)))
    }

    /// Where the stored height of `place` came from (TG-13, D-219): surveyed or the model;
    /// `None` where unstated or the layer has no heights.
    ///
    /// # Errors
    /// As [`Engine::elevation`].
    #[cfg(feature = "elevation")]
    pub fn elevation_source(&self, place: &Place) -> Result<Option<HeightSource>, Error> {
        let Some(l) = self.layer(place.layer) else {
            return Ok(None);
        };
        l.ensure(Kind::ELEVATION)?;
        Ok(l.views()
            .heights
            .as_ref()
            .and_then(|h| h.source(place.row as usize)))
    }

    /// The canonical name of `place` (its first name record), decoded into `buf`.
    ///
    /// # Errors
    /// [`Error::Integrity`] if NAMES is corrupt; [`Error::Format`] if `buf` is too small.
    #[cfg(feature = "names")]
    pub fn name<'b>(&self, place: &Place, buf: &'b mut [u8]) -> Result<Option<&'b str>, Error> {
        let Some(l) = self.layer(place.layer) else {
            return Ok(None);
        };
        l.ensure(Kind::NAMES)?;
        let Some(names) = &l.views().names else {
            return Ok(None);
        };
        match names.canonical(place.row as usize) {
            Some(n) => Ok(Some(n.text(buf)?)),
            None => Ok(None),
        }
    }

    /// The name of `place` in `lang`, a BCP 47 language (`ne`) or language and script
    /// (`ne-Deva`, so a romanisation tagged `ne` is not taken), decoded into `buf`, with what
    /// it is. A name a person wrote comes before a machine one (D-227), then the stored order.
    /// `None` when the place has no name in `lang` or `lang` is not such a tag.
    ///
    /// # Errors
    /// [`Error::Integrity`] if NAMES is corrupt; [`Error::Format`] if `buf` is too small.
    #[cfg(feature = "names")]
    pub fn name_in<'b>(
        &self,
        place: &Place,
        lang: &str,
        buf: &'b mut [u8],
    ) -> Result<Option<(&'b str, NameInfo)>, Error> {
        let Some((want, script)) = parse_lang(lang) else {
            return Ok(None);
        };
        let Some(names) = self.names_of(place)? else {
            return Ok(None);
        };
        let fits = |n: &teigeodata_format::names::Name<'_, '_>| {
            n.lang() == Some(want) && script.is_none_or(|s| n.script() == Some(s))
        };
        let best = names
            .names(place.row as usize)
            .filter(fits)
            .min_by_key(|n| n.origin().is_machine());
        match best {
            Some(n) => {
                let info = NameInfo::of(&n);
                Ok(Some((n.text(buf)?, info)))
            }
            None => Ok(None),
        }
    }

    /// Calls `f` with each name of `place`, its canonical name first, decoded into `buf`, and
    /// what it is: language, script, type and origin (D-227).
    ///
    /// # Errors
    /// [`Error::Integrity`] if NAMES is corrupt; [`Error::Format`] if `buf` is too small.
    #[cfg(feature = "names")]
    pub fn names(
        &self,
        place: &Place,
        buf: &mut [u8],
        mut f: impl FnMut(&str, NameInfo),
    ) -> Result<(), Error> {
        if let Some(names) = self.names_of(place)? {
            for n in names.names(place.row as usize) {
                let info = NameInfo::of(&n);
                f(n.text(buf)?, info);
            }
        }
        Ok(())
    }

    #[cfg(feature = "names")]
    fn names_of(
        &self,
        place: &Place,
    ) -> Result<Option<&teigeodata_format::names::Names<'_>>, Error> {
        let Some(l) = self.layer(place.layer) else {
            return Ok(None);
        };
        l.ensure(Kind::NAMES)?;
        Ok(l.views().names.as_ref())
    }

    /// The parent of `place` (HIERARCHY), as the winning layer answers for the parent's ID:
    /// from `place`'s own layer, else from the first layer by precedence that holds the same ID
    /// with a parent, so a pack whose copy of a place is a root defers to one that knows its
    /// parent (D-143). `None` for a root everywhere or with no hierarchy.
    ///
    /// # Errors
    /// [`Error::MissingComponent`] (`admin`) if a layer it reads has a hierarchy that is not
    /// installed; [`Error::Integrity`] if a section it reads is corrupt.
    #[cfg(feature = "admin")]
    pub fn parent(&self, place: &Place) -> Result<Option<Place>, Error> {
        let Some(own) = self.layers.iter().position(|l| l.id == place.layer) else {
            return Ok(None);
        };
        if let Some(row) = self.parent_row(own, place.row as usize)? {
            return self.resolve(own, row);
        }
        for j in (0..self.layers.len()).filter(|&j| j != own) {
            if let Some(r) = self.row_in(j, place.id)?
                && let Some(row) = self.parent_row(j, r)?
            {
                return self.resolve(j, row);
            }
        }
        Ok(None)
    }

    /// The ancestors of `place`, parent first up to the root, each as [`parent`](Self::parent)
    /// answers for the one before (so the chain may cross packs, D-143), into `out`. At most
    /// [`MAX_DEPTH`](teigeodata_format::hierarchy::MAX_DEPTH) of them, and never a place twice.
    ///
    /// # Errors
    /// As [`parent`](Self::parent).
    #[cfg(feature = "admin")]
    pub fn ancestors(&self, place: &Place, out: &mut Vec<Place>) -> Result<(), Error> {
        out.clear();
        let mut at = *place;
        while out.len() < teigeodata_format::hierarchy::MAX_DEPTH {
            match self.parent(&at)? {
                Some(p) if p.id != place.id && out.iter().all(|q| q.id != p.id) => {
                    out.push(p);
                    at = p;
                }
                _ => break,
            }
        }
        Ok(())
    }

    /// The children of `place` across every layer that holds its ID, by ID, each as the winning
    /// layer answers for it and only while that answer still has `place` as its parent (a newer
    /// pack may have moved it), into `out` (D-143).
    ///
    /// # Errors
    /// As [`parent`](Self::parent).
    #[cfg(feature = "admin")]
    pub fn children(&self, place: &Place, out: &mut Vec<Place>) -> Result<(), Error> {
        out.clear();
        for j in 0..self.layers.len() {
            let Some(r) = self.row_in(j, place.id)? else {
                continue;
            };
            let l = self.layers.get(j).ok_or(Error::Range)?;
            l.ensure(Kind::HIERARCHY)?;
            let Some(h) = l.views().hierarchy.as_ref() else {
                continue;
            };
            for row in h.children(r) {
                let Some(c) = self.place(j, row as usize)? else {
                    continue;
                };
                if !self.claimed_above(j, c.id)? {
                    out.push(c);
                } else if let Some(w) = self.get(c.id)?
                    && self.parent(&w)?.is_some_and(|q| q.id == place.id)
                {
                    out.push(w);
                }
            }
        }
        out.sort_unstable_by_key(|p| p.id);
        out.dedup_by_key(|p| p.id);
        Ok(())
    }

    /// The row of `id` itself in layer `i` (not a redirect).
    #[cfg(feature = "admin")]
    fn row_in(&self, i: usize, id: PlaceId) -> Result<Option<usize>, Error> {
        let Some(l) = self.layers.get(i).filter(|l| l.may_hold(id)) else {
            return Ok(None);
        };
        l.ensure(Kind::IDMAP)?;
        Ok(l.views().ids.as_ref().and_then(|m| m.row(id)))
    }

    /// The parent row of `row` in layer `i`'s HIERARCHY.
    #[cfg(feature = "admin")]
    fn parent_row(&self, i: usize, row: usize) -> Result<Option<usize>, Error> {
        let Some(l) = self.layers.get(i) else {
            return Ok(None);
        };
        l.ensure(Kind::HIERARCHY)?;
        Ok(l.views().hierarchy.as_ref().and_then(|h| h.parent(row)))
    }

    /// Row `row` of layer `i`, as the winning layer answers for its ID.
    #[cfg(feature = "admin")]
    fn resolve(&self, i: usize, row: usize) -> Result<Option<Place>, Error> {
        match self.place(i, row)? {
            Some(p) => self.winning(i, p),
            None => Ok(None),
        }
    }

    /// `p` from layer `i`, or the winning layer's answer for its ID when a layer above knows it.
    #[cfg(any(
        feature = "admin",
        feature = "timezone",
        feature = "timezone-index",
        feature = "admin-index",
        feature = "postal"
    ))]
    pub(crate) fn winning(&self, i: usize, p: Place) -> Result<Option<Place>, Error> {
        if self.claimed_above(i, p.id)? {
            self.get(p.id)
        } else {
            Ok(Some(p))
        }
    }

    /// The place with this ID, from the winning layer for it: its row there; for a redirect,
    /// the survivor (looked up again from the top, at most four hops); `None` for a tombstone
    /// or an ID no layer knows. Asks only layers whose ID range holds the ID.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt.
    pub fn get(&self, id: PlaceId) -> Result<Option<Place>, Error> {
        let mut want = id;
        'hop: for _ in 0..=MAX_HOPS {
            for (i, l) in self.layers.iter().enumerate() {
                if !l.may_hold(want) {
                    continue;
                }
                l.ensure(Kind::IDMAP)?;
                l.ensure(Kind::REDIRECTS)?;
                let v = l.views();
                if let Some(row) = v.ids.as_ref().and_then(|m| m.row(want)) {
                    return self.place(i, row);
                }
                match v.redirects.as_ref().and_then(|r| r.get(want)) {
                    Some(Fate::Redirect(new)) => {
                        want = new;
                        continue 'hop;
                    }
                    Some(Fate::Tombstone) => return Ok(None),
                    None => {}
                }
            }
            return Ok(None);
        }
        Ok(None)
    }

    /// The `k` places nearest to `at`, nearest first (ties by ID), into `out`. Each layer with a
    /// place index gives its nearest visible places (asked again for more while hidden places
    /// or ties at its last distance could hide one), and a k-way merge keeps the best `k`.
    /// Layers whose box is farther than the k-th distance found so far are skipped.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt.
    pub fn reverse(
        &self,
        at: Point<E6>,
        k: usize,
        s: &mut Scratch,
        out: &mut Vec<Near>,
    ) -> Result<(), Error> {
        self.nearest(at, k, f64::INFINITY, s, out)
    }

    /// The places within `meters` of `at` (great-circle, to float rounding), nearest first
    /// (ties by ID), at most `k`, into `out`: [`reverse`](Self::reverse) with a cap, which
    /// prunes every index box and layer beyond it. Empty for a negative or NaN radius.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt.
    pub fn within_radius(
        &self,
        at: Point<E6>,
        meters: f64,
        k: usize,
        s: &mut Scratch,
        out: &mut Vec<Near>,
    ) -> Result<(), Error> {
        if meters.is_nan() || meters < 0.0 {
            out.clear();
            return Ok(());
        }
        self.nearest(at, k, haversine_of(meters), s, out)
    }

    /// The `k` places nearest to `place`, nearest first (ties by ID), not counting the place
    /// itself, into `out`.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt.
    pub fn nearby(
        &self,
        place: &Place,
        k: usize,
        s: &mut Scratch,
        out: &mut Vec<Near>,
    ) -> Result<(), Error> {
        self.nearest(place.point, k.saturating_add(1), f64::INFINITY, s, out)?;
        out.retain(|n| n.place.id != place.id);
        out.truncate(k);
        Ok(())
    }

    /// `reverse` and `within_radius`: the `k` places nearest to `at` whose haversine is at
    /// most `cap`.
    fn nearest(
        &self,
        at: Point<E6>,
        k: usize,
        cap: f64,
        s: &mut Scratch,
        out: &mut Vec<Near>,
    ) -> Result<(), Error> {
        out.clear();
        s.near.clear();
        s.segs.clear();
        if k == 0 {
            return Ok(());
        }
        let at7 = at.to_e7();
        // The k-th distance so far (the cap until k are found) bounds every layer still to ask.
        let mut bound = cap;
        for (i, l) in self.layers.iter().enumerate() {
            let v = l.views();
            let (Some(index_), Some(ids)) = (&v.place_index, &v.ids) else {
                continue;
            };
            if lower_bound(l.bbox(), at7) > bound {
                continue;
            }
            l.ensure(Kind::PLACE_INDEX)?;
            l.ensure(Kind::PLACES)?;
            l.ensure(Kind::IDMAP)?;
            let start = s.near.len();
            self.layer_nearest(i, index_, ids, at, k, bound, s)?;
            s.segs.push(start..s.near.len());
            // The k-th distance over every layer so far bounds the layers still to ask.
            let mut seen = 0;
            let mut kth = f64::INFINITY;
            merge(&s.near, &s.segs, &mut s.heads, k, near_less, |c| {
                seen += 1;
                kth = c.haversine;
                Ok(())
            })?;
            if seen == k {
                bound = kth;
            }
        }
        let (near, segs, heads) = (&s.near, &s.segs, &mut s.heads);
        merge(near, segs, heads, k, near_less, |c| {
            if let Some(place) = self.place(c.layer as usize, c.row as usize)? {
                let meters = Neighbour {
                    row: c.row as usize,
                    haversine: c.haversine,
                }
                .meters();
                let bearing_deg = teigeodata_format::place_index::bearing_deg(
                    place.point.lat_degrees(),
                    place.point.lon_degrees(),
                    at.lat_degrees(),
                    at.lon_degrees(),
                );
                out.push(Near {
                    place,
                    meters,
                    bearing_deg,
                    inside: None,
                });
            }
            Ok(())
        })
    }

    /// Appends layer `i`'s visible places nearest to `at` to `s.near`, nearest first (ties by
    /// ID), at most `k` and none farther than `bound`; asks the index again for more while a
    /// hidden place or a tie at the last distance fetched could hide one.
    #[allow(
        clippy::too_many_arguments,
        clippy::many_single_char_names,
        reason = "one layer's share of `reverse`, in its names"
    )]
    fn layer_nearest(
        &self,
        i: usize,
        index_: &PlaceIndex<'_>,
        ids: &IdMap<'_>,
        at: Point<E6>,
        k: usize,
        bound: f64,
        s: &mut Scratch,
    ) -> Result<(), Error> {
        let start = s.near.len();
        // One more than k, so that a k-th place tied with an unfetched one is noticed.
        let mut want = k.saturating_add(1);
        loop {
            s.near.truncate(start);
            if s.neighbours.len() < want {
                s.neighbours.resize(want, Neighbour::default());
            }
            let buf = s.neighbours.get_mut(..want).unwrap_or_default();
            let n = index_.nearest_within(at, bound, buf);
            let fetched = buf.get(..n).unwrap_or_default();
            let last = fetched.last().map_or(f64::NEG_INFINITY, |x| x.haversine);
            s.near.extend(fetched.iter().filter_map(|nb| {
                Some(NearCand {
                    haversine: nb.haversine,
                    id: ids.id(nb.row)?.get(),
                    layer: index(i),
                    row: index(nb.row),
                })
            }));
            s.near
                .get_mut(start..)
                .unwrap_or_default()
                .sort_unstable_by(|a, b| a.haversine.total_cmp(&b.haversine).then(a.id.cmp(&b.id)));
            // Keep the visible places, nearest first, up to k and up to the k-th distance
            // of the layers before; `stop` is the distance where the walk stopped.
            let mut kept = start;
            let mut stop = None;
            for j in start..s.near.len() {
                let Some(c) = s.near.get(j).copied() else {
                    break;
                };
                if c.haversine > bound {
                    stop = Some(bound);
                    break;
                }
                let Some(id) = PlaceId::from_raw(c.id) else {
                    continue;
                };
                if self.claimed_above(i, id)? {
                    continue;
                }
                if let Some(slot) = s.near.get_mut(kept) {
                    *slot = c;
                }
                kept += 1;
                if kept - start == k {
                    stop = Some(c.haversine);
                    break;
                }
            }
            s.near.truncate(kept);
            // Complete when the layer ran out, or when every place up to where the walk
            // stopped was fetched (the last one fetched is strictly farther).
            if n < want || stop.is_some_and(|h| h < last) {
                break;
            }
            want = want.saturating_mul(2);
        }
        Ok(())
    }

    /// The places whose point is in `rect` (edges included; `min_lon > max_lon` crosses the
    /// antimeridian), in ID order, at most `limit`, into `out`. Asks the layers whose box meets
    /// `rect`; each gives its visible places with the smallest IDs, and a k-way merge by ID
    /// keeps the first `limit`.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt.
    pub fn within_bbox(
        &self,
        rect: Rect,
        limit: usize,
        s: &mut Scratch,
        out: &mut Vec<Place>,
    ) -> Result<(), Error> {
        out.clear();
        s.near.clear();
        s.segs.clear();
        if limit == 0 {
            return Ok(());
        }
        for (i, l) in self.layers.iter().enumerate() {
            let v = l.views();
            let (Some(index_), Some(ids)) = (&v.place_index, &v.ids) else {
                continue;
            };
            if !meets(l.bbox(), &rect) {
                continue;
            }
            l.ensure(Kind::PLACE_INDEX)?;
            l.ensure(Kind::PLACES)?;
            l.ensure(Kind::IDMAP)?;
            let start = s.near.len();
            let me = index(i);
            let near = &mut s.near;
            let _ = index_.within(rect, |row| {
                if let Some(id) = ids.id(row) {
                    near.push(NearCand {
                        haversine: 0.0,
                        id: id.get(),
                        layer: me,
                        row: index(row),
                    });
                }
                ControlFlow::Continue(())
            });
            s.near
                .get_mut(start..)
                .unwrap_or_default()
                .sort_unstable_by_key(|c| c.id);
            let mut kept = start;
            for j in start..s.near.len() {
                if kept - start == limit {
                    break;
                }
                let Some(c) = s.near.get(j).copied() else {
                    break;
                };
                let Some(id) = PlaceId::from_raw(c.id) else {
                    continue;
                };
                if self.claimed_above(i, id)? {
                    continue;
                }
                if let Some(slot) = s.near.get_mut(kept) {
                    *slot = c;
                }
                kept += 1;
            }
            s.near.truncate(kept);
            s.segs.push(start..kept);
        }
        let (near, segs, heads) = (&s.near, &s.segs, &mut s.heads);
        merge(
            near,
            segs,
            heads,
            limit,
            |a, b| a.id < b.id,
            |c| {
                if let Some(place) = self.place(c.layer as usize, c.row as usize)? {
                    out.push(place);
                }
                Ok(())
            },
        )
    }

    /// The admin units holding `at`, every level, coarse to fine, into `out`, from the layer
    /// with the highest precedence whose admin index holds the point (coverage beats existence,
    /// the engine note §3.4); the baseline only where no pack covers it. Returns that layer.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt.
    #[cfg(feature = "admin-index")]
    pub fn admin_at(
        &self,
        at: Point<E7>,
        s: &mut Scratch,
        out: &mut Vec<Place>,
    ) -> Result<Option<LayerId>, Error> {
        out.clear();
        for (i, l) in self.layers.iter().enumerate() {
            if !l.covers(at) {
                continue;
            }
            l.ensure(Kind::ADMIN_REGION)?;
            let v = l.views();
            let Some(admin) = &v.admin else { continue };
            if admin.on_arcs() {
                l.ensure(Kind::ARCS)?;
            }
            l.ensure(Kind::POLYGONS)?;
            let n = regions_into(admin, at, &mut s.regions);
            if n == 0 {
                continue;
            }
            for &r in s.regions.get(..n).unwrap_or_default() {
                let Some(row) = v.admin_row(admin, r) else {
                    continue;
                };
                if let Some(p) = self.place(i, row as usize)?
                    && let Some(p) = self.winning(i, p)?
                {
                    out.push(p);
                }
            }
            return Ok(Some(l.id));
        }
        Ok(None)
    }

    /// Sets [`Near::inside`] on `near` (as [`reverse`](Self::reverse) answered for `at`): for a
    /// country or an admin unit, whether it holds `at` by the stored outlines, as
    /// [`admin_at`](Self::admin_at) answers; `None` for every other place, and for all of them
    /// when no layer's outlines cover `at`. One `admin_at` for the whole answer (TG-19).
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt.
    #[cfg(feature = "admin-index")]
    pub fn mark_inside(
        &self,
        at: Point<E6>,
        s: &mut Scratch,
        near: &mut [Near],
    ) -> Result<(), Error> {
        let mut units = core::mem::take(&mut s.units);
        let covered = self.admin_at(at.to_e7(), s, &mut units);
        let covered = match covered {
            Ok(c) => c.is_some(),
            Err(e) => {
                s.units = units;
                return Err(e);
            }
        };
        for n in near.iter_mut() {
            let outlined = matches!(n.place.kind, Some(PlaceKind::Country | PlaceKind::Admin));
            n.inside = (covered && outlined).then(|| units.iter().any(|u| u.id == n.place.id));
        }
        s.units = units;
        Ok(())
    }

    /// The countries that hold `at` or whose border is within `radius_m` metres of it, into
    /// `out`, each once, as its winning layer answers it, in no set order. Every layer is asked,
    /// the baseline too: this is the locator's question (D-149), "which country packs could
    /// this point need?". The outlines are simplified, so pass a radius at least their
    /// tolerance ([`LOCATOR_RADIUS_M`] for the baseline) and no border is missed; a larger one
    /// only adds neighbours. Layers without outlines (`boundaries` not installed) are skipped.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt.
    #[cfg(feature = "admin-index")]
    pub fn countries_near(
        &self,
        at: Point<E7>,
        radius_m: u32,
        out: &mut Vec<Place>,
    ) -> Result<(), Error> {
        out.clear();
        let query = radius_box(at, radius_m);
        let r_cm = radius_m.saturating_mul(100);
        for (i, l) in self.layers.iter().enumerate() {
            let v = l.views();
            let (Some(polygons), Some(arcs), Some(index)) = (&v.polygons, &v.arcs, &v.poly_index)
            else {
                continue;
            };
            l.ensure(Kind::POLYGONS)?;
            l.ensure(Kind::POLY_INDEX)?;
            l.ensure(Kind::ARCS)?;
            let Some(chunks) = l.chunks() else { continue };
            let mut failed = None;
            let _ = index.intersecting(query, 0..polygons.len(), |p| {
                match self.country_within(i, (polygons, arcs, chunks), p, at, r_cm) {
                    Ok(Some(c)) if !out.iter().any(|o| o.id == c.id) => out.push(c),
                    Ok(_) => {}
                    Err(e) => {
                        failed = Some(e);
                        return ControlFlow::Break(());
                    }
                }
                ControlFlow::Continue(())
            });
            if let Some(e) = failed {
                return Err(e);
            }
        }
        Ok(())
    }

    /// The countries near `at` ([`countries_near`](Self::countries_near)) that no installed
    /// pack answers for, into `out`: the packs to fetch before answering at `at` in full. A
    /// country's pack address is its ID's namespace (`np`); a pack at that address answers for
    /// it, and a pack of part of it (`ae-du`) where its coverage box holds `at`. Countries
    /// without a code (`Namespace::WORLD`) have no pack and are left out.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt.
    #[cfg(feature = "admin-index")]
    pub fn missing_packs(
        &self,
        at: Point<E7>,
        radius_m: u32,
        out: &mut Vec<Place>,
    ) -> Result<(), Error> {
        self.countries_near(at, radius_m, out)?;
        out.retain(|c| {
            let Some(code) = c.id.namespace().country() else {
                return false;
            };
            !self.layers.iter().any(|l| {
                if l.options().baseline {
                    return false;
                }
                let a = l.address().as_bytes();
                match a {
                    [x, y] => [*x, *y] == code,
                    [x, y, b'-', ..] => [*x, *y] == code && l.covers(at),
                    _ => false,
                }
            })
        });
        Ok(())
    }

    /// Polygon `p` of layer `i` as its country, when it is one and holds `at` or has a border
    /// within `r_cm` of it.
    #[cfg(feature = "admin-index")]
    fn country_within(
        &self,
        i: usize,
        (polygons, arcs, chunks): (
            &teigeodata_format::polygons::Polygons<'_>,
            &teigeodata_format::arcs::Arcs<'_>,
            &teigeodata_format::arcs::ArcChunks,
        ),
        p: usize,
        at: Point<E7>,
        r_cm: u32,
    ) -> Result<Option<Place>, Error> {
        let Some(row) = polygons.row(p) else {
            return Ok(None);
        };
        let Some(place) = self.place(i, row as usize)? else {
            return Ok(None);
        };
        if place.kind != Some(PlaceKind::Country)
            || !polygons.within_chunked(arcs, chunks, p, at, r_cm)
        {
            return Ok(None);
        }
        self.winning(i, place)
    }

    /// The timezone area holding `at`, from the layer with the highest precedence that answers
    /// for the point; `None` where none does. A layer answers by its timezone index (REGION);
    /// a layer without one but with each place's zone and admin outlines (the baseline's world
    /// part, D-151) answers by the zone of its deepest admin unit holding the point, exact where
    /// a unit lies in one zone.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt.
    #[cfg(feature = "timezone-index")]
    pub fn tz_at(&self, at: Point<E7>) -> Result<Option<Place>, Error> {
        for (i, l) in self.layers.iter().enumerate() {
            if !l.covers(at) {
                continue;
            }
            l.ensure(Kind::REGION)?;
            let v = l.views();
            let row = match &v.zones {
                Some(zones) => zones.region(at).and_then(|r| zones.place(r)),
                #[cfg(all(feature = "timezone", feature = "admin-index"))]
                None => self.tz_by_admin(i, at)?,
                #[cfg(not(all(feature = "timezone", feature = "admin-index")))]
                None => None,
            };
            let Some(row) = row else {
                continue;
            };
            return match self.place(i, row as usize)? {
                Some(p) => self.winning(i, p),
                None => Ok(None),
            };
        }
        Ok(None)
    }

    /// Layer `i`'s zone row at `at` from its admin units (D-151): the `PLACE_TZ` of the
    /// deepest unit holding the point that has a zone. `None` without `PLACE_TZ` or outlines.
    #[cfg(all(
        feature = "timezone",
        feature = "admin-index",
        feature = "timezone-index"
    ))]
    fn tz_by_admin(&self, i: usize, at: Point<E7>) -> Result<Option<u32>, Error> {
        let l = self.layers.get(i).ok_or(Error::Range)?;
        let v = l.views();
        let (Some(tz), Some(admin), Some(places)) = (&v.place_tz, &v.admin, &v.places) else {
            return Ok(None);
        };
        l.ensure(Kind::PLACE_TZ)?;
        l.ensure(Kind::ADMIN_REGION)?;
        if admin.on_arcs() {
            l.ensure(Kind::ARCS)?;
        }
        l.ensure(Kind::POLYGONS)?;
        // A point lies in a few units (a country and its unit); 16 is far above that, and a
        // point in more is answered from the first 16, coarse to fine.
        let mut buf = [0_u32; 16];
        let n = admin.regions_at(at, &mut buf).min(buf.len());
        Ok(buf
            .get(..n)
            .unwrap_or_default()
            .iter()
            .filter_map(|&r| {
                let row = v.admin_row(admin, r)? as usize;
                Some((places.admin_level(row)?, tz.get(row)?))
            })
            .max_by_key(|&(level, _)| level)
            .map(|(_, zone)| zone))
    }

    /// The timezone area of `place`: the one its own layer recorded for it at build time
    /// (`PLACE_TZ`, D-134), a few bits per place, so it needs no REGION; for a layer without
    /// `PLACE_TZ` installed, [`tz_at`](Self::tz_at) its point. Both give the same area for a
    /// place of a full pack.
    ///
    /// # Errors
    /// [`Error::MissingComponent`] (`timezone`) if neither `timezone` nor `timezone-index` is
    /// installed where the pack has them; [`Error::Integrity`] if a section it reads is
    /// corrupt.
    #[cfg(any(feature = "timezone", feature = "timezone-index"))]
    pub fn tz_of(&self, place: &Place) -> Result<Option<Place>, Error> {
        let Some(i) = self.layers.iter().position(|l| l.id == place.layer) else {
            return Ok(None);
        };
        let l = self.layers.get(i).ok_or(Error::Range)?;
        #[cfg(feature = "timezone")]
        let own = l.views().place_tz.as_ref();
        #[cfg(not(feature = "timezone"))]
        let own: Option<&teigeodata_format::place_tz::PlaceTz<'_>> = None;
        let Some(tz) = own else {
            // No PLACE_TZ, or not installed: the area at the point, which agrees. If that index
            // is missing too, the component asked for is the one to name.
            #[cfg(feature = "timezone-index")]
            return match self.tz_at(place.point.to_e7()) {
                Err(Error::MissingComponent { address, .. })
                    if !l.views().installed(Kind::PLACE_TZ) =>
                {
                    Err(Error::MissingComponent {
                        address,
                        component: crate::Component::Timezone,
                    })
                }
                r => r,
            };
            #[cfg(not(feature = "timezone-index"))]
            return l.ensure(Kind::PLACE_TZ).map(|()| None);
        };
        l.ensure(Kind::PLACE_TZ)?;
        match tz.get(place.row as usize) {
            Some(row) => match self.place(i, row as usize)? {
                Some(p) => self.winning(i, p),
                None => Ok(None),
            },
            None => Ok(None),
        }
    }
}

#[cfg(all(test, feature = "admin-index"))]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use super::*;

    fn at(lat: f64, lon: f64) -> Point<E7> {
        Point::from_degrees(lat, lon).unwrap()
    }

    #[test]
    fn the_radius_box_holds_the_circle() {
        // 1 km at the equator: about 0.009° each way, rounded outward.
        let b = radius_box(at(0.0, 10.0), 1_000);
        assert_eq!((b.min_lat, b.max_lat), (-8_995, 8_995));
        assert_eq!((b.min_lon, b.max_lon), (9_991_005, 10_008_995));
        // At 60° the longitude span doubles.
        let b = radius_box(at(60.0, 0.0), 1_000);
        assert!(b.max_lon >= 17_990 && b.max_lon < 18_100, "{b:?}");
        // Across the antimeridian: min_lon > max_lon.
        let b = radius_box(at(0.0, 179.995), 1_000);
        assert!(b.min_lon > b.max_lon && b.max_lon < -179_990_000, "{b:?}");
        // Near a pole: every longitude, the latitude clamped.
        let b = radius_box(at(89.995, 0.0), 1_000);
        assert_eq!(
            (b.min_lon, b.max_lon, b.max_lat),
            (-180_000_000, 180_000_000, 90_000_000)
        );
        // Zero radius: the point's cell.
        let b = radius_box(at(27.7, 85.3), 0);
        assert!(b.min_lat < 27_700_000 && b.max_lat > 27_700_000);
    }
}

/// What a name is (D-227): its language, script, type and how it was made.
#[cfg(feature = "names")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NameInfo {
    /// Its language; `None` if undetermined.
    pub lang: Option<teigeodata_format::names::Tag>,
    /// Its script; `None` if unknown.
    pub script: Option<teigeodata_format::names::Tag>,
    /// Its type; `None` if unspecified.
    pub kind: Option<teigeodata_format::names::NameType>,
    /// How it was made; [`Origin::is_machine`](teigeodata_format::names::Origin::is_machine)
    /// says whether a program wrote it.
    pub origin: teigeodata_format::names::Origin,
}

#[cfg(feature = "names")]
impl NameInfo {
    fn of(n: &teigeodata_format::names::Name<'_, '_>) -> Self {
        Self {
            lang: n.lang(),
            script: n.script(),
            kind: n.kind(),
            origin: n.origin(),
        }
    }
}

/// `ne` or `ne-Deva` as a language tag and an optional script tag.
#[cfg(feature = "names")]
fn parse_lang(
    code: &str,
) -> Option<(
    teigeodata_format::names::Tag,
    Option<teigeodata_format::names::Tag>,
)> {
    use teigeodata_format::names::Tag;
    let mut it = code.split('-');
    let lang = Tag::new(it.next()?)?;
    let script = match it.next() {
        None => None,
        Some(s) if s.len() == 4 => Some(Tag::new(s)?),
        Some(_) => return None,
    };
    it.next().is_none().then_some((lang, script))
}
