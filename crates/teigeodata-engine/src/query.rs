//! Queries across layers: routing, the precedence rules for IDs, and the k-way merges (the
//! engine note, §3.4 and §3.5).
//!
//! The rule every query follows: a place `(layer, row)` is **visible** only if no layer above it
//! knows its ID (as a current place, a redirect or a tombstone). The winning layer for an ID
//! decides everything about it, so a stale copy below can never leak into an answer. Answers are
//! ordered by a key that depends only on the place (distance or score, then ID), so they do not
//! depend on how places are split into packs or in which order packs were added.

use core::cell::Cell;
use core::ops::{ControlFlow, Range};

use teigeodata_core::{E6, E7, PlaceId, PlaceKind, Point};
use teigeodata_format::Kind;
use teigeodata_format::idmap::IdMap;
use teigeodata_format::place_index::{Neighbour, PlaceIndex, Rect};
use teigeodata_format::redirects::Fate;
use teigeodata_format::region::Region;
use teigeodata_format::search::fuzzy::{Fuzzy, MAX_CHARS};
use teigeodata_format::search::{EXACT, LOOSE, PHONETIC, Search};
use teigeodata_names::KeyBuf;

use crate::layer::QueryRules;
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

#[derive(Clone, Copy, Debug)]
struct HitCand {
    score: i32,
    id: u64,
    layer: u32,
    row: u32,
}

#[derive(Clone, Copy, Debug)]
struct PrefixCand {
    importance: u8,
    id: u64,
    layer: u32,
    row: u32,
}

/// Caller-owned working memory for queries. Buffers grow to the largest query seen and are
/// reused, so a warm query does not allocate (M1). One per thread; [`Default`] is empty.
#[derive(Debug, Default)]
pub struct Scratch {
    neighbours: Vec<Neighbour>,
    near: Vec<NearCand>,
    hits: Vec<HitCand>,
    rows: Vec<(u32, i32)>,
    prefix: Vec<PrefixCand>,
    segs: Vec<Range<usize>>,
    heads: Vec<usize>,
    regions: Vec<u32>,
    loose: String,
    /// The rows a layer's names match the query's context with, sorted.
    context: Vec<u32>,
    /// A query form with a native generic replaced by a Latin spelling.
    form: String,
    /// A phonetic key and its buffers.
    pkey: String,
    keybuf: KeyBuf,
}

/// Redirect hops [`Engine::get`] follows across layers (the forge flattens chains in a pack, so
/// more than one hop means layers of different releases).
const MAX_HOPS: usize = 4;

// Match bonuses, added to the importance (0 to 255): S3's, one set for every country (D-052).
const EXACT_BONUS: i32 = 60;
const LOOSE_BONUS: i32 = 55;
const FUZZY_BONUS: i32 = 35;
const FUZZY_PER_EDIT: i32 = 10;
const PREFIX_BONUS: i32 = 30;
/// Added to a row whose parent (or the row itself) the query names after a comma: a named
/// parent outranks every other match (ADR-0032).
const CONTEXT_BONUS: i32 = 1_000;
/// The most rows a context may name in one layer (a short prefix names many).
const CONTEXT_MAX: usize = 4_096;
/// Edits a context may differ by, as S3 (D-052).
const CONTEXT_EDITS: u8 = 2;
/// A phonetic match: a low-weight safety net that never decides alone (ADR-0032).
const PHONETIC_BONUS: i32 = 5;
/// A loose match once a droppable suffix is removed ("Janakpurdham").
const DROP_BONUS: i32 = LOOSE_BONUS - 5;
/// Added to a row of the unit type a query names by its abbreviation ("Janaki RM").
const UNIT_BONUS: i32 = 20;
/// A match this good (one edit or better) ends the search for any first letter.
const STRONG: i32 = FUZZY_BONUS - FUZZY_PER_EDIT;
/// The prefix tier takes the best rows by importance (D-086).
const PREFIX_K: usize = 16;
/// The prefix tier runs from this many characters.
const PREFIX_MIN: usize = 3;
/// The fuzzy tier runs from this many characters (D-086).
const FUZZY_MIN: usize = 4;

/// A query as each layer reads it: the folded name, its loose key, the fuzzy automaton with
/// its anchor, and a unit-type abbreviation it ended with.
#[derive(Clone, Copy)]
struct Query<'q> {
    q: &'q str,
    loose: &'q str,
    fuzzy: Option<(&'q Fuzzy, Option<char>)>,
    abbr: Option<&'q str>,
    /// The layer's query rules.
    rules: &'q QueryRules<'q>,
}

/// Distances below this are compared as `≤` when pruning, so float rounding of a lower bound
/// never prunes a layer holding a place at the k-th distance.
const SLACK: f64 = 1.0 - 1e-12;

fn index(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// The k-way merge of sorted segments of `items`: emits the first `k` in `less` order.
fn merge<T>(
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

fn hit_less(a: &HitCand, b: &HitCand) -> bool {
    b.score.cmp(&a.score).then(a.id.cmp(&b.id)).is_lt()
}

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
    fn place(&self, i: usize, row: usize) -> Result<Option<Place>, Error> {
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
    fn claimed_above(&self, i: usize, id: PlaceId) -> Result<bool, Error> {
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

    /// `p` from layer `i`, or the winning layer's answer for its ID when a layer above knows it.
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
        out.clear();
        s.near.clear();
        s.segs.clear();
        if k == 0 {
            return Ok(());
        }
        let at7 = at.to_e7();
        let mut bound = f64::INFINITY;
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
            let n = index_.nearest(at, buf);
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
                let Some(row) = v.region_row(admin, r, true) else {
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
    pub fn tz_at(&self, at: Point<E7>) -> Result<Option<Place>, Error> {
        for (i, l) in self.layers.iter().enumerate() {
            if !l.covers(at) {
                continue;
            }
            l.ensure(Kind::REGION)?;
            let v = l.views();
            let Some(zones) = &v.zones else { continue };
            let Some(r) = zones.region(at) else { continue };
            let Some(row) = v.region_row(zones, r, false) else {
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
    pub fn tz_of(&self, place: &Place) -> Result<Option<Place>, Error> {
        let Some(i) = self.layers.iter().position(|l| l.id == place.layer) else {
            return Ok(None);
        };
        let l = self.layers.get(i).ok_or(Error::Range)?;
        let Some(tz) = &l.views().place_tz else {
            // No PLACE_TZ, or not installed: the area at the point, which agrees. If that index
            // is missing too, the component asked for is the one to name.
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

    /// Places matching `query`, best first (ties by ID), at most `k`, into `out`. The query is
    /// folded as the forge folds names; each layer scores its rows with the same formula
    /// (D-052): exact 60, loose 55, one edit 25 (queries of 4 to 63 characters, keys with the
    /// query's first letter; any first letter only when no layer has a visible match of one
    /// edit or better), plus the best 16 by importance over every layer for a prefix of 3
    /// characters or more (30), each plus the row's importance. After a comma the query names a
    /// context ("Name, Parent"): a row whose ancestor (or itself) has a name matching it,
    /// exactly, loosely, within 2 edits or as a prefix, gains 1,000, so a named parent outranks
    /// every other match (ADR-0032, D-119). Each layer keeps its best `k` visible rows; a k-way
    /// merge keeps the best `k` overall.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt.
    pub fn search(
        &self,
        query: &str,
        k: usize,
        s: &mut Scratch,
        out: &mut Vec<Hit>,
    ) -> Result<(), Error> {
        out.clear();
        s.hits.clear();
        s.segs.clear();
        s.prefix.clear();
        if k == 0 {
            return Ok(());
        }
        // "Name, Parent": the parent is a context, matched against each row's ancestors.
        let (name, context) = match query.split_once(',') {
            Some((n, c)) => (n, teigeodata_names::fold(c)),
            None => (query, String::new()),
        };
        let folded = teigeodata_names::fold(name);
        // A trailing unit-type abbreviation that a layer's rules name ("Janaki RM") is not part
        // of the name: rows of that type gain UNIT_BONUS instead (D-120).
        let (q, abbr) = match folded.rsplit_once(' ') {
            Some((stem, last)) if !stem.is_empty() && self.names_unit(last) => (stem, Some(last)),
            _ => (folded.as_str(), None),
        };
        if q.is_empty() {
            return Ok(());
        }
        let mut loose = core::mem::take(&mut s.loose);
        loose.clear();
        loose.extend(q.chars().filter(|&c| c != ' '));
        let chars = q.chars().count();
        let fuzzy = if (FUZZY_MIN..=MAX_CHARS).contains(&chars) {
            Fuzzy::new(q, 1)
        } else {
            None
        };
        if chars >= PREFIX_MIN {
            for (i, l) in self.layers.iter().enumerate() {
                l.ensure(Kind::SEARCH)?;
                l.ensure(Kind::IDMAP)?;
                let v = l.views();
                let (Some(search), Some(ids)) = (&v.search, &v.ids) else {
                    continue;
                };
                self.prefix_best(i, search, ids, q, &mut s.prefix)?;
            }
            s.prefix
                .sort_unstable_by(|a, b| b.importance.cmp(&a.importance).then(a.id.cmp(&b.id)));
            s.prefix.truncate(PREFIX_K);
        }
        // Fuzzy keys start with the query's first letter; only when no layer has a strong match
        // (exact, loose, within one edit or a prefix) does a second pass take any first letter
        // (ADR-0032): first-letter typos are rare, and the anchored walk is several times faster.
        let mut anchor = q.chars().next();
        loop {
            let mut strong = false;
            for (i, l) in self.layers.iter().enumerate() {
                l.ensure(Kind::SEARCH)?;
                l.ensure(Kind::IDMAP)?;
                let v = l.views();
                let (Some(search), Some(ids)) = (&v.search, &v.ids) else {
                    continue;
                };
                context_rows(search, &context, &mut s.context);
                if !s.context.is_empty() {
                    l.ensure(Kind::HIERARCHY)?;
                }
                let start = s.hits.len();
                let walk = fuzzy.as_ref().map(|f| (f, anchor));
                let query = Query {
                    q,
                    loose: &loose,
                    fuzzy: walk,
                    abbr,
                    rules: &v.rules,
                };
                strong |= self.layer_hits(i, search, ids, query, k, s)?;
                s.segs.push(start..s.hits.len());
            }
            if strong || fuzzy.is_none() || anchor.is_none() {
                break;
            }
            anchor = None;
            s.hits.clear();
            s.segs.clear();
        }
        // The loose key's buffer goes back to the scratch, to be reused.
        s.loose = loose;
        let (hits, segs, heads) = (&s.hits, &s.segs, &mut s.heads);
        merge(hits, segs, heads, k, hit_less, |c| {
            if let Some(place) = self.place(c.layer as usize, c.row as usize)? {
                out.push(Hit {
                    place,
                    score: c.score,
                });
            }
            Ok(())
        })
    }

    /// Appends layer `i`'s best `k` visible rows for the folded query `q` (its loose key and its
    /// fuzzy automaton, anchored on a first letter or not) to `s.hits`, best first (ties by
    /// ID). Returns whether any row matched strongly: [`STRONG`] or better before importance.
    fn layer_hits(
        &self,
        i: usize,
        search: &Search<'_>,
        ids: &IdMap<'_>,
        Query {
            q,
            loose,
            fuzzy,
            abbr,
            rules,
        }: Query<'_>,
        k: usize,
        s: &mut Scratch,
    ) -> Result<bool, Error> {
        s.rows.clear();
        rule_hits(
            search,
            rules,
            q,
            &mut s.rows,
            (&mut s.form, &mut s.pkey, &mut s.keybuf),
        );
        let rows = &mut s.rows;
        let add = |ordinal: Option<u32>, bonus: i32, rows: &mut Vec<(u32, i32)>| {
            if let Some(o) = ordinal {
                search.rows_of(o, |r| {
                    rows.push((r, bonus));
                    ControlFlow::Continue(())
                });
            }
        };
        add(search.get(EXACT, q), EXACT_BONUS, rows);
        // A loose lookup reads the loose list and its exact twin (D-086).
        add(search.get(LOOSE, loose), LOOSE_BONUS, rows);
        add(search.get(EXACT, loose), LOOSE_BONUS, rows);
        if let Some((f, anchor)) = fuzzy {
            search.walk(EXACT, f, anchor, usize::MAX, |o, edits| {
                let bonus = FUZZY_BONUS - FUZZY_PER_EDIT * i32::from(edits);
                add(Some(o), bonus, rows);
                ControlFlow::Continue(())
            });
        }
        let me = index(i);
        rows.extend(
            s.prefix
                .iter()
                .filter(|p| p.layer == me)
                .map(|p| (p.row, PREFIX_BONUS)),
        );
        rows.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        rows.dedup_by_key(|r| r.0);
        let unit = abbr.and_then(|a| rules.unit(a));
        if let Some(u) = unit {
            for r in rows.iter_mut() {
                if search.unit(r.0) == u {
                    r.1 += UNIT_BONUS;
                }
            }
        }
        // Strong only through a visible row, so a hidden row decides nothing (D-114); the unit
        // bonus does not count.
        let mut strong = false;
        for &(row, mut bonus) in rows.iter() {
            if unit.is_some_and(|u| search.unit(row) == u) {
                bonus -= UNIT_BONUS;
            }
            if bonus < STRONG {
                continue;
            }
            if let Some(id) = ids.id(row as usize)
                && !self.claimed_above(i, id)?
            {
                strong = true;
                break;
            }
        }
        self.context_bonus(i, &s.context, rows);
        // Score every row, then read IDs (and check visibility) only for the groups of equal
        // score that can still enter this layer's best k.
        for r in rows.iter_mut() {
            r.1 += i32::from(search.importance(r.0));
        }
        rows.sort_unstable_by_key(|r| core::cmp::Reverse(r.1));
        let start = s.hits.len();
        for group in rows.chunk_by(|a, b| a.1 == b.1) {
            if s.hits.len() - start >= k {
                break;
            }
            for &(row, score) in group {
                let Some(id) = ids.id(row as usize) else {
                    continue;
                };
                if self.claimed_above(i, id)? {
                    continue;
                }
                s.hits.push(HitCand {
                    score,
                    id: id.get(),
                    layer: me,
                    row,
                });
            }
        }
        let seg = s.hits.get_mut(start..).unwrap_or_default();
        seg.sort_unstable_by(|a, b| b.score.cmp(&a.score).then(a.id.cmp(&b.id)));
        s.hits.truncate(start + k.min(seg_len(start, s.hits.len())));
        Ok(strong)
    }

    /// Appends layer `i`'s best [`PREFIX_K`] visible rows under a key starting with `q`, by
    /// importance then ID. Skips keys and blocks of keys that cannot enter (`Search::best_in`).
    fn prefix_best(
        &self,
        i: usize,
        search: &Search<'_>,
        ids: &IdMap<'_>,
        q: &str,
        prefix: &mut Vec<PrefixCand>,
    ) -> Result<(), Error> {
        let start = prefix.len();
        // Once full, keys whose best row ties the worst kept one are still read: a tie is
        // broken by ID, which the key order does not follow.
        let floor = Cell::new(-1_i32);
        let mut failed = None;
        let me = index(i);
        search.best_in(
            search.range(EXACT, q),
            || floor.get(),
            |o| {
                search.rows_of(o, |r| {
                    let importance = search.importance(r);
                    if i32::from(importance) <= floor.get() {
                        return ControlFlow::Break(());
                    }
                    let top = prefix.get(start..).unwrap_or_default();
                    if top.iter().any(|p| p.row == r) {
                        return ControlFlow::Continue(());
                    }
                    let Some(id) = ids.id(r as usize) else {
                        return ControlFlow::Continue(());
                    };
                    let rank = |p: &PrefixCand| (p.importance, core::cmp::Reverse(p.id));
                    let cand = PrefixCand {
                        importance,
                        id: id.get(),
                        layer: me,
                        row: r,
                    };
                    let full = top.len() == PREFIX_K;
                    let worst = top
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, p)| rank(p))
                        .map(|(j, p)| (j, rank(p)));
                    if full && worst.is_some_and(|(_, w)| rank(&cand) <= w) {
                        return ControlFlow::Continue(());
                    }
                    match self.claimed_above(i, id) {
                        Ok(true) => return ControlFlow::Continue(()),
                        Ok(false) => {}
                        Err(e) => {
                            failed = Some(e);
                            return ControlFlow::Break(());
                        }
                    }
                    if full && let Some((j, _)) = worst {
                        prefix.swap_remove(start + j);
                    }
                    prefix.push(cand);
                    if prefix.len() - start == PREFIX_K {
                        let low = prefix
                            .get(start..)
                            .unwrap_or_default()
                            .iter()
                            .map(|p| p.importance)
                            .min()
                            .unwrap_or(0);
                        floor.set(i32::from(low) - 1);
                    }
                    ControlFlow::Continue(())
                });
                if failed.is_some() {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
        );
        failed.map_or(Ok(()), Err)
    }
}

const fn seg_len(start: usize, end: usize) -> usize {
    end.saturating_sub(start)
}

impl Engine {
    /// Adds [`CONTEXT_BONUS`] to each of layer `i`'s `rows` whose ancestor (or itself) is one
    /// of the `context` rows.
    fn context_bonus(&self, i: usize, context: &[u32], rows: &mut [(u32, i32)]) {
        let hierarchy = self
            .layers
            .get(i)
            .and_then(|l| l.views().hierarchy.as_ref());
        if let (false, Some(h)) = (context.is_empty(), hierarchy) {
            let named =
                |r: usize| u32::try_from(r).is_ok_and(|r| context.binary_search(&r).is_ok());
            for r in rows.iter_mut() {
                let row = r.0 as usize;
                if named(row) || h.ancestors(row).any(named) {
                    r.1 += CONTEXT_BONUS;
                }
            }
        }
    }

    /// Whether some layer's rules name `abbreviation` as a unit type's.
    fn names_unit(&self, abbreviation: &str) -> bool {
        self.layers
            .iter()
            .any(|l| l.views().rules.unit(abbreviation).is_some())
    }
}

/// The matches the layer's query rules add (D-120): the phonetic key of the query and of each
/// form with its native generic replaced by a Latin spelling (also looked up loosely), and the
/// query without a droppable suffix. The bonuses as in S3 (D-052).
fn rule_hits(
    search: &Search<'_>,
    rules: &QueryRules<'_>,
    q: &str,
    rows: &mut Vec<(u32, i32)>,
    (form, pkey, keybuf): (&mut String, &mut String, &mut KeyBuf),
) {
    let add = |ordinal: Option<u32>, bonus: i32, rows: &mut Vec<(u32, i32)>| {
        if let Some(o) = ordinal {
            search.rows_of(o, |r| {
                rows.push((r, bonus));
                ControlFlow::Continue(())
            });
        }
    };
    let phonetic = rules.phonetic;
    let mut buf = [0_u8; 256];
    let mut lookup = |f: &str, loosely: bool, rows: &mut Vec<(u32, i32)>| {
        if loosely {
            let l = despace(f, &mut buf);
            add(search.get(LOOSE, l), LOOSE_BONUS, rows);
            add(search.get(EXACT, l), LOOSE_BONUS, rows);
        }
        if let Some(p) = phonetic {
            p.key_into(f, keybuf, pkey);
            add(search.get(PHONETIC, pkey), PHONETIC_BONUS, rows);
        }
    };
    lookup(q, false, rows);
    // The longest native generic the query ends with (the natives are longest first): each of
    // its Latin spellings in its place.
    let mut natives = rules
        .natives
        .iter()
        .skip_while(|n| !q.ends_with(n.0))
        .peekable();
    if let Some(&&(n, _)) = natives.peek()
        && let Some(stem) = q.strip_suffix(n)
    {
        for &(_, spelling) in natives.take_while(|m| m.0 == n) {
            form.clear();
            form.push_str(stem);
            form.push_str(spelling);
            lookup(form, true, rows);
        }
    }
    for d in &rules.drops {
        if let Some(stem) = q.strip_suffix(d)
            && !stem.trim().is_empty()
        {
            let mut b = [0_u8; 256];
            let l = despace(stem, &mut b);
            add(search.get(LOOSE, l), DROP_BONUS, rows);
            add(search.get(EXACT, l), DROP_BONUS, rows);
        }
    }
}

/// The rows of `search` whose names match `context` (folded): exactly, loosely, within
/// [`CONTEXT_EDITS`] edits with the same first letter, or as a prefix of 3 characters or more; sorted, at most
/// [`CONTEXT_MAX`]. Empty for an empty context.
fn context_rows(search: &Search<'_>, context: &str, out: &mut Vec<u32>) {
    out.clear();
    let context = context.trim();
    if context.is_empty() {
        return;
    }
    let take = |o: u32, out: &mut Vec<u32>| {
        search.rows_of(o, |r| {
            if out.len() >= CONTEXT_MAX {
                return ControlFlow::Break(());
            }
            out.push(r);
            ControlFlow::Continue(())
        });
    };
    let mut buf = [0_u8; 256];
    let loose = despace(context, &mut buf);
    for o in [
        search.get(EXACT, context),
        search.get(LOOSE, loose),
        search.get(EXACT, loose),
    ]
    .into_iter()
    .flatten()
    {
        take(o, out);
    }
    let chars = context.chars().count();
    if (FUZZY_MIN..=MAX_CHARS).contains(&chars)
        && let Some(f) = Fuzzy::new(context, CONTEXT_EDITS)
    {
        // Anchored on the first letter, as the name's first pass: a parent is typed with care.
        search.walk(EXACT, &f, context.chars().next(), usize::MAX, |o, _| {
            take(o, out);
            if out.len() >= CONTEXT_MAX {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });
    }
    if chars >= PREFIX_MIN {
        for o in search.range(EXACT, context) {
            if out.len() >= CONTEXT_MAX {
                break;
            }
            take(o, out);
        }
    }
    out.sort_unstable();
    out.dedup();
}

/// `s` without spaces, in `buf` (the loose key's fold, D-086); `s` itself if it does not fit.
fn despace<'b>(s: &'b str, buf: &'b mut [u8; 256]) -> &'b str {
    let mut n = 0;
    for b in s.bytes().filter(|&b| b != b' ') {
        let Some(slot) = buf.get_mut(n) else {
            return s;
        };
        *slot = b;
        n += 1;
    }
    core::str::from_utf8(buf.get(..n).unwrap_or_default()).unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use teigeodata_format::search::{self, EXACT, LOOSE, Search};
    use teigeodata_format::{Kind, Section};

    use teigeodata_format::search::PHONETIC;
    use teigeodata_format::search::rules::Rule;
    use teigeodata_names::{KeyBuf, PhoneticKey};

    use super::{
        CONTEXT_MAX, DROP_BONUS, LOOSE_BONUS, PHONETIC_BONUS, QueryRules, context_rows, despace,
        rule_hits,
    };

    fn body(names: &[(u8, &str, u32)], rows: usize) -> Vec<u8> {
        let mut keys: BTreeMap<Vec<u8>, BTreeSet<u32>> = BTreeMap::new();
        for &(tag, text, row) in names {
            let mut k = vec![tag];
            k.extend_from_slice(text.as_bytes());
            keys.entry(k).or_default().insert(row);
        }
        search::encode(&keys, &vec![1; rows]).unwrap()
    }

    #[test]
    fn a_context_names_rows_exactly_loosely_by_edits_and_prefix() {
        let b = body(
            &[
                (EXACT, "kathmandu", 0),
                (EXACT, "lalitpur", 1),
                (LOOSE, "dolakha", 2), // "dola kha" folds loosely
                (EXACT, "kailali", 3),
                (EXACT, "jhapa", 4),
            ],
            5,
        );
        let s = Search::open(&Section::unpacked(Kind::SEARCH, 5, &b)).unwrap();
        let mut out = Vec::new();
        let mut rows = |c: &str| {
            context_rows(&s, c, &mut out);
            out.clone()
        };
        assert_eq!(rows("kathmandu"), [0]);
        assert_eq!(rows("kathmanud"), [0]); // one transposition
        assert_eq!(rows("katmando"), [0]); // two edits
        assert_eq!(rows("athmandu"), Vec::<u32>::new()); // first letter differs
        assert_eq!(rows("dola kha"), [2]);
        assert_eq!(rows("lal"), [1]); // a prefix of 3
        assert_eq!(rows("la"), Vec::<u32>::new());
        assert_eq!(rows("  "), Vec::<u32>::new());
    }

    #[test]
    fn the_rules_add_native_drop_and_phonetic_matches() {
        let key = PhoneticKey::from_name("indic-v0").unwrap().key("janakpur");
        let mut keys: BTreeMap<Vec<u8>, BTreeSet<u32>> = BTreeMap::new();
        let mut put = |tag: u8, text: &str, row: u32| {
            let mut k = vec![tag];
            k.extend_from_slice(text.as_bytes());
            keys.entry(k).or_default().insert(row);
        };
        put(EXACT, "janakpur", 0);
        put(EXACT, "rampurzz", 1);
        put(EXACT, "rampurayy", 2);
        put(EXACT, "rampurww", 3);
        put(PHONETIC, &key, 4);
        // The shorter native first: the longest the query ends with still wins, and each of its
        // spellings is tried.
        let rules = [
            Rule::Phonetic("indic-v0"),
            Rule::Unit {
                abbreviation: "rm",
                unit: 3,
            },
            Rule::Drop("dham"),
            Rule::Native {
                native: "b",
                spelling: "yy",
            },
            Rule::Native {
                native: "ab",
                spelling: "zz",
            },
            Rule::Native {
                native: "ab",
                spelling: "ww",
            },
        ];
        let extra = search::Extra {
            rules: Some(&rules),
            units: &[0, 3, 0, 0, 0],
        };
        let b = search::encode_with(&keys, &[1; 5], &extra).unwrap();
        let s = Search::open(&Section::unpacked(Kind::SEARCH, 5, &b)).unwrap();
        let r = QueryRules::new(&s);
        assert_eq!(r.unit("rm"), Some(3));
        assert_eq!(r.unit("m"), None);
        assert_eq!(s.unit(1), 3);
        let (mut form, mut pkey, mut buf) = (String::new(), String::new(), KeyBuf::default());
        let mut hits = |q: &str| {
            let mut rows = Vec::new();
            rule_hits(&s, &r, q, &mut rows, (&mut form, &mut pkey, &mut buf));
            rows.sort_unstable();
            rows
        };
        assert_eq!(hits("rampurab"), [(1, LOOSE_BONUS), (3, LOOSE_BONUS)]);
        assert_eq!(hits("janakpurdham"), [(0, DROP_BONUS)]);
        assert_eq!(hits("dham"), []);
        assert_eq!(hits("janakpur"), [(4, PHONETIC_BONUS)]);
        // A layer without RULES has none.
        let empty = body(&[], 5);
        let plain = Search::open(&Section::unpacked(Kind::SEARCH, 5, &empty)).unwrap();
        let none = QueryRules::new(&plain);
        assert!(none.phonetic.is_none() && none.natives.is_empty() && none.unit("rm").is_none());
    }

    #[test]
    fn a_context_is_bounded() {
        // Many keys under one prefix, and one key with more rows than the bound.
        let n = u32::try_from(CONTEXT_MAX).unwrap() + 50;
        let names: Vec<(u8, String, u32)> = (0..n)
            .map(|i| (EXACT, format!("kar{i:05}"), i))
            .chain((0..n).map(|i| (EXACT, "kar".to_string(), i)))
            .collect();
        let refs: Vec<(u8, &str, u32)> =
            names.iter().map(|(t, n, r)| (*t, n.as_str(), *r)).collect();
        let b = body(&refs, n as usize);
        let s = Search::open(&Section::unpacked(Kind::SEARCH, u64::from(n), &b)).unwrap();
        let mut out = Vec::new();
        context_rows(&s, "kar", &mut out);
        assert_eq!(out.len(), CONTEXT_MAX);
        let mut buf = [0_u8; 256];
        assert_eq!(despace("a b  c", &mut buf), "abc");
        let long = "x".repeat(300);
        assert_eq!(despace(&long, &mut buf), long);
    }
}
