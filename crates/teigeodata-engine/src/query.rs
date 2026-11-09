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
use teigeodata_format::place_index::{Neighbour, PlaceIndex};
use teigeodata_format::redirects::Fate;
use teigeodata_format::region::Region;
use teigeodata_format::search::fuzzy::{Fuzzy, MAX_CHARS};
use teigeodata_format::search::{EXACT, LOOSE, Search};

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
    /// Its elevation in metres above EGM2008, when known.
    pub elevation: Option<i16>,
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
/// The prefix tier takes the best rows by importance (D-086).
const PREFIX_K: usize = 16;
/// The prefix tier runs from this many characters.
const PREFIX_MIN: usize = 3;
/// The fuzzy tier runs from this many characters (D-086).
const FUZZY_MIN: usize = 4;

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
            elevation: places.elevation(row),
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
            let v = l.views();
            let Some(admin) = &v.admin else { continue };
            if !l.covers(at) {
                continue;
            }
            l.ensure(Kind::ADMIN_REGION)?;
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
            let v = l.views();
            let Some(zones) = &v.zones else { continue };
            if !l.covers(at) {
                continue;
            }
            l.ensure(Kind::REGION)?;
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

    /// Places matching `query`, best first (ties by ID), at most `k`, into `out`. The query is
    /// folded as the forge folds names; each layer scores its rows with the same formula
    /// (D-052): exact 60, loose 55, one edit 25 (queries of 4 to 63 characters), plus the best
    /// 16 by importance over every layer for a prefix of 3 characters or more (30), each plus
    /// the row's importance. Each layer keeps its best `k` visible rows; a k-way merge keeps
    /// the best `k` overall.
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
        let q = teigeodata_names::fold(query);
        if q.is_empty() {
            return Ok(());
        }
        let mut loose = core::mem::take(&mut s.loose);
        loose.clear();
        loose.extend(q.chars().filter(|&c| c != ' '));
        let chars = q.chars().count();
        let fuzzy = if (FUZZY_MIN..=MAX_CHARS).contains(&chars) {
            Fuzzy::new(&q, 1)
        } else {
            None
        };
        if chars >= PREFIX_MIN {
            for (i, l) in self.layers.iter().enumerate() {
                let v = l.views();
                let (Some(search), Some(ids)) = (&v.search, &v.ids) else {
                    continue;
                };
                l.ensure(Kind::SEARCH)?;
                l.ensure(Kind::IDMAP)?;
                self.prefix_best(i, search, ids, &q, &mut s.prefix)?;
            }
            s.prefix
                .sort_unstable_by(|a, b| b.importance.cmp(&a.importance).then(a.id.cmp(&b.id)));
            s.prefix.truncate(PREFIX_K);
        }
        for (i, l) in self.layers.iter().enumerate() {
            let v = l.views();
            let (Some(search), Some(ids)) = (&v.search, &v.ids) else {
                continue;
            };
            l.ensure(Kind::SEARCH)?;
            l.ensure(Kind::IDMAP)?;
            let start = s.hits.len();
            self.layer_hits(i, search, ids, (&q, &loose, fuzzy.as_ref()), k, s)?;
            s.segs.push(start..s.hits.len());
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
    /// fuzzy automaton) to `s.hits`, best first (ties by ID).
    fn layer_hits(
        &self,
        i: usize,
        search: &Search<'_>,
        ids: &IdMap<'_>,
        (q, loose, fuzzy): (&str, &str, Option<&Fuzzy>),
        k: usize,
        s: &mut Scratch,
    ) -> Result<(), Error> {
        s.rows.clear();
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
        if let Some(f) = fuzzy {
            search.walk(EXACT, f, None, usize::MAX, |o, edits| {
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
        Ok(())
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
