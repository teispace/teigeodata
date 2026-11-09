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
use teigeodata_format::idmap::IdMap;
use teigeodata_format::place_index::{Neighbour, PlaceIndex, Rect, haversine_of};
use teigeodata_format::redirects::Fate;
#[cfg(feature = "admin-index")]
use teigeodata_format::region::Region;
#[cfg(feature = "search")]
use teigeodata_names::KeyBuf;

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

/// A place near a point, from [`Engine::reverse`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Near {
    /// The place.
    pub place: Place,
    /// The great-circle distance in metres.
    pub meters: f64,
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
    neighbours: Vec<Neighbour>,
    near: Vec<NearCand>,
    pub(crate) segs: Vec<Range<usize>>,
    pub(crate) heads: Vec<usize>,
    #[cfg(feature = "admin-index")]
    regions: Vec<u32>,
    #[cfg(feature = "search")]
    pub(crate) hits: Vec<HitCand>,
    #[cfg(feature = "search")]
    pub(crate) rows: Vec<(u32, i32)>,
    #[cfg(feature = "search")]
    pub(crate) prefix: Vec<PrefixCand>,
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

    /// The parent of `place` in its own pack's admin hierarchy (HIERARCHY), as the winning
    /// layer answers for the parent's ID; `None` for a root or a layer without a hierarchy.
    ///
    /// # Errors
    /// [`Error::MissingComponent`] (`admin`) if the layer has a hierarchy that is not
    /// installed; [`Error::Integrity`] if a section it reads is corrupt.
    #[cfg(feature = "admin")]
    pub fn parent(&self, place: &Place) -> Result<Option<Place>, Error> {
        let Some(i) = self.layers.iter().position(|l| l.id == place.layer) else {
            return Ok(None);
        };
        let l = self.layers.get(i).ok_or(Error::Range)?;
        l.ensure(Kind::HIERARCHY)?;
        let Some(row) = l
            .views()
            .hierarchy
            .as_ref()
            .and_then(|h| h.parent(place.row as usize))
        else {
            return Ok(None);
        };
        match self.place(i, row)? {
            Some(p) => self.winning(i, p),
            None => Ok(None),
        }
    }

    /// The ancestors of `place` in its own pack's admin hierarchy, parent first up to the root,
    /// each as the winning layer answers for its ID, into `out`.
    ///
    /// # Errors
    /// As [`parent`](Self::parent).
    #[cfg(feature = "admin")]
    pub fn ancestors(&self, place: &Place, out: &mut Vec<Place>) -> Result<(), Error> {
        out.clear();
        let Some(i) = self.layers.iter().position(|l| l.id == place.layer) else {
            return Ok(());
        };
        let l = self.layers.get(i).ok_or(Error::Range)?;
        l.ensure(Kind::HIERARCHY)?;
        let Some(h) = l.views().hierarchy.as_ref() else {
            return Ok(());
        };
        for row in h.ancestors(place.row as usize) {
            if let Some(p) = self.place(i, row)?
                && let Some(p) = self.winning(i, p)?
            {
                out.push(p);
            }
        }
        Ok(())
    }

    /// The children of `place` in its own pack's admin hierarchy, in row order, each as the
    /// winning layer answers for its ID, into `out`.
    ///
    /// # Errors
    /// As [`parent`](Self::parent).
    #[cfg(feature = "admin")]
    pub fn children(&self, place: &Place, out: &mut Vec<Place>) -> Result<(), Error> {
        out.clear();
        let Some(i) = self.layers.iter().position(|l| l.id == place.layer) else {
            return Ok(());
        };
        let l = self.layers.get(i).ok_or(Error::Range)?;
        l.ensure(Kind::HIERARCHY)?;
        let Some(h) = l.views().hierarchy.as_ref() else {
            return Ok(());
        };
        for row in h.children(place.row as usize) {
            if let Some(p) = self.place(i, row as usize)?
                && let Some(p) = self.winning(i, p)?
            {
                out.push(p);
            }
        }
        Ok(())
    }

    /// `p` from layer `i`, or the winning layer's answer for its ID when a layer above knows it.
    #[cfg(any(
        feature = "admin",
        feature = "timezone",
        feature = "timezone-index",
        feature = "admin-index"
    ))]
    fn winning(&self, i: usize, p: Place) -> Result<Option<Place>, Error> {
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
                out.push(Near { place, meters });
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

    /// The timezone area holding `at`, from the layer with the highest precedence whose
    /// timezone index holds the point; `None` where none does.
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
            let Some(zones) = &v.zones else { continue };
            let Some(r) = zones.region(at) else { continue };
            let Some(row) = zones.place(r) else {
                continue;
            };
            return match self.place(i, row as usize)? {
                Some(p) => self.winning(i, p),
                None => Ok(None),
            };
        }
        Ok(None)
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
