//! Rows of a pack as a new pack's sections (D-145): the embedded baseline's slice of a country
//! pack, and the CLI's `extract`; and rows of several packs as one ([`merge`], D-153: the
//! shipped baseline).
//!
//! [`rows`] keeps rows `keep` of a pack, in their order (PLACES stays in Hilbert order), and
//! re-encodes every per-row section the engine reads: IDMAP, PLACES, `PLACE_INDEX`, ELEVATION,
//! NAMES, SEARCH (keys cut to the kept rows, the query rules and unit types kept), and
//! HIERARCHY, where each kept row's parent is its **nearest kept ancestor**, so a slice that
//! keeps its places' ancestors keeps their chain (D-143). The admin geometry of the kept rows
//! (ARCS, POLYGONS, `POLY_INDEX`, `ADMIN_REGION`) is re-encoded with only the arcs they use, at
//! level of detail `lod` of the pack's ARCS (0 = full resolution): the levels are the forge's
//! topology-safe simplification, so no new simplifier runs here. `PLACE_TZ` keeps each kept
//! row's zone when the zone's row is kept too, and none otherwise (D-151): keep the zone rows
//! of the kept places to keep their zones. `ISO_CODES` keeps each kept row's code (D-197).
//!
//! Left out, for the caller to add if it needs them: REGION (a slice answers `tz_at` from its
//! units and `PLACE_TZ`), REDIRECTS, POSTAL, DEM, BLOBS and EXTRA. The manifest is the
//! caller's too: it names the new pack, and must carry the source pack's attribution.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use teigeodata_core::{E6, E7, PlaceId, Point, hilbert};

use crate::arcs::{self, ArcColumns, Arcs};
use crate::elevation::{self, Elevations};
use crate::hierarchy::{self, Hierarchy};
use crate::idmap::{self, IdMap};
use crate::iso_codes::{self, IsoCodes};
use crate::names::{self, NameRecord, NameSet};
use crate::place_index::{self, PlaceIndex};
use crate::place_tz::{self, PlaceTz};
use crate::places::{self, PlaceColumns, Places};
use crate::poly_index::{self, PolyIndex, Rect};
use crate::polygons::{self, PolygonInput, Polygons};
use crate::region::{self, Options};
use crate::search::{self, Search, SearchSet};
use crate::{Encoding, Error, Kind, Pack};

/// One section of the new pack: kind, encoding, item count, body.
pub type Body = (Kind, Encoding, u64, Vec<u8>);

/// The node size of a rebuilt index when the pack has none to copy.
const NODE: usize = 16;

fn fail(problem: &'static str) -> Error {
    Error::Write(problem)
}

/// The finest level of `arcs` whose tolerance is at most `max_cm`: 0 (full resolution) when the
/// arcs have no levels or no level is that fine.
#[must_use]
pub fn lod_within(arcs: &Arcs<'_>, max_cm: u32) -> u8 {
    (1..=6)
        .rev()
        .find(|&l| arcs.tolerance_cm(l).is_some_and(|t| t <= max_cm))
        .unwrap_or(0)
}

/// The sections of rows `keep` (increasing, each a PLACES row of `pack`) as a pack of their own,
/// admin geometry at level `lod` with `ADMIN_REGION` built with `region` (a coarse slice wants a
/// coarser index than the pack's). See the [module](self).
///
/// # Errors
/// [`Error::Write`] if `keep` is not increasing or names a row past the end, the pack has no
/// IDMAP or PLACES, or a section does not re-encode; a reader's error for a malformed section.
pub fn rows(pack: &Pack<'_>, keep: &[u32], lod: u8, region: &Options) -> Result<Vec<Body>, Error> {
    rows_with(pack, keep, lod, region, None)
}

/// Which names kept rows keep, and how SEARCH keys a name (D-152): a slice that keeps fewer
/// names than its pack rebuilds its search keys from the names it keeps, so it finds a place
/// only by a name it can show. The rows' importance, unit types and query rules stay the
/// pack's.
pub struct NameRule<'a> {
    /// Whether name `i` (0 is the canonical, always kept) of pack row `row` stays.
    pub keep: &'a dyn Fn(u32, usize, &NameRecord<'_>) -> bool,
    /// Adds the SEARCH keys of `text`, a name of new row `row` made as its origin says, to
    /// the maps, exactly as the pack's build keyed it.
    pub keys: &'a dyn Fn(&str, names::Origin, u32, &mut search::Maps),
}

impl core::fmt::Debug for NameRule<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NameRule").finish_non_exhaustive()
    }
}

/// [`rows`], keeping only the names `names` keeps (all of them for `None`).
///
/// # Errors
/// As [`rows`].
pub fn rows_with(
    pack: &Pack<'_>,
    keep: &[u32],
    lod: u8,
    region: &Options,
    names: Option<&NameRule<'_>>,
) -> Result<Vec<Body>, Error> {
    merge(
        &[Part {
            pack,
            keep,
            lod,
            names,
            arcs: None,
        }],
        region,
    )
}

/// One pack's share of a [`merge`].
#[derive(Clone, Copy, Debug)]
pub struct Part<'p, 'a> {
    /// The pack.
    pub pack: &'p Pack<'a>,
    /// Its rows to keep, increasing.
    pub keep: &'p [u32],
    /// The level of detail of its admin geometry (0 = full resolution).
    pub lod: u8,
    /// Its name rule; `None` keeps every name and its SEARCH keys as they are.
    pub names: Option<&'p NameRule<'p>>,
    /// A rewrite of its admin arcs at `lod` before they are encoded; `None` keeps them.
    pub arcs: Option<&'p ArcRewrite<'p>>,
}

/// A caller's rewrite of one part's admin arcs, each a run of points on the 1e-7° grid, before
/// the merge encodes them (TG-16: the forge snaps a coarse part to a decimal grid, so ARCS
/// stores each arc's deltas ÷ 10^k). The rewrite must keep every arc's ends where the arcs
/// meet (neighbours share them), at least two points per arc and four per closed arc, and no
/// crossings: the merge re-encodes what it is given, and the boxes, labels and `ADMIN_REGION`
/// follow the rewritten points.
pub struct ArcRewrite<'a> {
    /// Rewrites the part's arcs in place.
    pub run: &'a dyn Fn(&mut [Arc]),
}

/// One arc's points, as an [`ArcRewrite`] sees them.
pub type Arc = Vec<Point<E7>>;

impl core::fmt::Debug for ArcRewrite<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ArcRewrite").finish_non_exhaustive()
    }
}

/// The rows of several packs as one pack (the shipped baseline: the world part and each
/// country's slice, one layer). Rows are in Hilbert order (ties by part, then row). A row
/// whose ID a later part keeps too is left out: the later part's row replaces it, and every
/// reference to it (a parent, a zone) goes to the replacement. Admin geometry is each part's
/// at its own level of detail; polygons are grouped by admin level, coarse to fine. SEARCH
/// takes every part's keys, importance and unit types; the query rules are the union, each
/// part's unit types moved past the earlier parts'. A part without a section gives its rows
/// none of it.
///
/// # Errors
/// As [`rows`], and [`Error::Write`] if the parts name two phonetic keys or more than 255 unit
/// types between them.
pub fn merge(parts: &[Part<'_, '_>], region: &Options) -> Result<Vec<Body>, Error> {
    let mut open = Vec::with_capacity(parts.len());
    for (at, &part) in parts.iter().enumerate() {
        let ids = IdMap::open(&part.pack.section(Kind::IDMAP).ok_or(fail("no IDMAP"))?)?;
        let places = Places::open(&part.pack.section(Kind::PLACES).ok_or(fail("no PLACES"))?)?;
        let n = places.len();
        if part.keep.windows(2).any(|w| matches!(w, [a, b] if a >= b))
            || part.keep.iter().any(|&r| r as usize >= n)
        {
            return Err(fail("the kept rows are not increasing rows of the pack"));
        }
        open.push(Opened {
            at,
            part,
            ids,
            places,
            own: Vec::new(),
            own_of: vec![u32::MAX; n],
        });
    }
    // The last part holding an ID owns its row.
    let mut seen = BTreeSet::new();
    for o in open.iter_mut().rev() {
        for &r in o.part.keep {
            if seen.insert(o.ids.id(r as usize).ok_or(fail("a row without an ID"))?) {
                o.own.push(r);
            }
        }
    }
    // PLACES is in Hilbert order: number the rows by it.
    let mut order = Vec::new();
    for (i, o) in open.iter().enumerate() {
        for &r in &o.own {
            let at = o
                .places
                .point(r as usize)
                .ok_or(fail("a row without a point"))?;
            order.push((hilbert(at), i, r));
        }
    }
    order.sort_unstable();
    let mut by_id = BTreeMap::new();
    for (new, &(_, i, r)) in (0_u32..).zip(&order) {
        let o = open.get_mut(i).ok_or(fail("a part"))?;
        *o.own_of.get_mut(r as usize).ok_or(fail("a kept row"))? = new;
        by_id.insert(
            o.ids.id(r as usize).ok_or(fail("a row without an ID"))?,
            new,
        );
    }
    let m = Merged {
        open,
        by_id,
        total: u32::try_from(order.len()).map_err(|_| fail("too many rows"))?,
    };
    let mut out = m.places()?;
    out.extend(m.zones()?);
    out.extend(m.names_and_search()?);
    out.extend(m.hierarchy()?);
    out.extend(m.iso_codes()?);
    out.extend(m.geometry(region)?);
    Ok(out)
}

/// A part with its rows opened and numbered.
struct Opened<'p, 'a> {
    /// Its place among the parts.
    at: usize,
    part: Part<'p, 'a>,
    ids: IdMap<'a>,
    places: Places<'a>,
    /// Its kept rows no later part replaces, increasing.
    own: Vec<u32>,
    /// Each of its rows' new row, `u32::MAX` if not its own.
    own_of: Vec<u32>,
}

impl<'a> Opened<'_, 'a> {
    fn section(&self, kind: Kind) -> Option<crate::Section<'a>> {
        self.part.pack.section(kind)
    }
}

/// The parts of a merge, numbered.
struct Merged<'p, 'a> {
    open: Vec<Opened<'p, 'a>>,
    /// The new row of each kept ID.
    by_id: BTreeMap<PlaceId, u32>,
    total: u32,
}

/// A name record, owned.
type OwnedName = (String, NameMeta);
type NameMeta = (
    Option<names::Tag>,
    Option<names::Tag>,
    Option<names::NameType>,
    (Option<i32>, Option<i32>),
    u32,
);

impl<'a> Merged<'_, 'a> {
    /// The new row of row `r` of part `o`: its own, else the row that replaced its ID.
    fn resolve(&self, o: &Opened<'_, '_>, r: usize) -> Option<u32> {
        match o.own_of.get(r) {
            Some(&n) if n != u32::MAX => Some(n),
            _ => o.ids.id(r).and_then(|id| self.by_id.get(&id).copied()),
        }
    }

    fn any(&self, kind: Kind) -> bool {
        self.open.iter().any(|o| o.section(kind).is_some())
    }

    /// One value per new row: `f` of each part's own rows, `empty` where no row is.
    fn scatter<T: Clone>(
        &self,
        empty: T,
        mut f: impl FnMut(&Opened<'_, '_>, u32) -> Result<T, Error>,
    ) -> Result<Vec<T>, Error> {
        let mut out = vec![empty; self.total as usize];
        for o in &self.open {
            for &r in &o.own {
                let new = o.own_of.get(r as usize).copied().unwrap_or(u32::MAX);
                *out.get_mut(new as usize).ok_or(fail("a kept row"))? = f(o, r)?;
            }
        }
        Ok(out)
    }

    /// As [`Merged::scatter`], with each part's section `kind` opened by `open` once.
    fn scatter_in<S, T: Clone>(
        &self,
        kind: Kind,
        empty: T,
        open: impl Fn(&crate::Section<'a>) -> Result<S, Error>,
        f: impl FnMut(&S, &Opened<'_, '_>, u32) -> Result<T, Error>,
    ) -> Result<Vec<T>, Error> {
        self.scatter_with(
            |o| o.section(kind).as_ref().map(&open).transpose(),
            empty,
            f,
        )
    }

    /// As [`Merged::scatter`], with what `open` makes of each pack, once (`None` where it has
    /// nothing).
    fn scatter_with<S, T: Clone>(
        &self,
        open: impl Fn(&Opened<'_, 'a>) -> Result<Option<S>, Error>,
        empty: T,
        mut f: impl FnMut(&S, &Opened<'_, '_>, u32) -> Result<T, Error>,
    ) -> Result<Vec<T>, Error> {
        let opened: Vec<Option<S>> = self.open.iter().map(open).collect::<Result<_, _>>()?;
        let mut out = vec![empty; self.total as usize];
        for (o, s) in self.open.iter().zip(&opened) {
            let Some(s) = s else { continue };
            for &r in &o.own {
                let new = o.own_of.get(r as usize).copied().unwrap_or(u32::MAX);
                *out.get_mut(new as usize).ok_or(fail("a kept row"))? = f(s, o, r)?;
            }
        }
        Ok(out)
    }

    /// IDMAP, PLACES, `PLACE_INDEX` and ELEVATION.
    fn places(&self) -> Result<Vec<Body>, Error> {
        let k = u64::from(self.total);
        let short = || fail("a PLACES row is short");
        let id_list = self.scatter(None, |o, r| {
            o.ids
                .id(r as usize)
                .map(Some)
                .ok_or(fail("a row without an ID"))
        })?;
        let id_list: Vec<PlaceId> = id_list.into_iter().collect::<Option<_>>().ok_or(short())?;
        let rows = self.scatter(None, |o, r| {
            let (p, r) = (&o.places, r as usize);
            Ok(Some((
                p.point(r).ok_or(short())?,
                p.kind(r).ok_or(short())?,
                p.admin_level(r).ok_or(short())?,
                p.class(r).ok_or(short())?,
                p.flags(r).ok_or(short())?,
            )))
        })?;
        let rows: Vec<_> = rows.into_iter().collect::<Option<_>>().ok_or(short())?;
        let points: Vec<Point<E6>> = rows.iter().map(|x| x.0).collect();
        let kinds: Vec<_> = rows.iter().map(|x| x.1).collect();
        let levels: Vec<_> = rows.iter().map(|x| x.2).collect();
        let classes: Vec<_> = rows.iter().map(|x| x.3).collect();
        let flags: Vec<_> = rows.iter().map(|x| x.4).collect();
        let mut out: Vec<Body> = vec![
            (Kind::IDMAP, Encoding::TABLE, k, idmap::encode(&id_list)?),
            (
                Kind::PLACES,
                Encoding::TABLE,
                k,
                places::encode(&PlaceColumns {
                    points: &points,
                    kinds: &kinds,
                    admin_levels: &levels,
                    classes: &classes,
                    flags: &flags,
                })?,
            ),
        ];
        let mut node = NODE;
        for o in &self.open {
            if let Some(s) = o.section(Kind::PLACE_INDEX) {
                node = PlaceIndex::open(&s, o.places)?.node_size();
                break;
            }
        }
        out.push((
            Kind::PLACE_INDEX,
            Encoding::TABLE,
            k,
            place_index::encode(&points, node)?,
        ));
        if self.any(Kind::ELEVATION) {
            let both = self.scatter_in(
                Kind::ELEVATION,
                (None, None),
                Elevations::open,
                |h, _, r| Ok((h.get(r as usize), h.source(r as usize))),
            )?;
            let (heights, sources): (Vec<_>, Vec<_>) = both.into_iter().unzip();
            if let Some(body) = elevation::encode_with(&heights, &sources)? {
                out.push((Kind::ELEVATION, Encoding::TABLE, k, body));
            }
        }
        Ok(out)
    }

    /// `PLACE_TZ`: each row's zone when the zone's row is kept.
    fn zones(&self) -> Result<Option<Body>, Error> {
        if !self.any(Kind::PLACE_TZ) {
            return Ok(None);
        }
        let zone_of = self.scatter_in(Kind::PLACE_TZ, None, PlaceTz::open, |t, o, r| {
            Ok(t.get(r as usize).and_then(|z| self.resolve(o, z as usize)))
        })?;
        let k = u64::from(self.total);
        Ok(place_tz::encode(&zone_of)?.map(|b| (Kind::PLACE_TZ, Encoding::TABLE, k, b)))
    }

    /// `ISO_CODES`: each kept row's code, from the part that owns the row.
    fn iso_codes(&self) -> Result<Option<Body>, Error> {
        let mut codes = Vec::new();
        for o in &self.open {
            let Some(s) = o.section(Kind::ISO_CODES) else {
                continue;
            };
            let t = IsoCodes::open(&s, o.places.len())?;
            for &r in &o.own {
                if let Some(c) = t.get(r) {
                    let new = o.own_of.get(r as usize).copied().unwrap_or(u32::MAX);
                    codes.push((new, c));
                }
            }
        }
        codes.sort_unstable_by_key(|c| c.0);
        let k = codes.len() as u64;
        Ok(iso_codes::encode(&codes)?.map(|b| (Kind::ISO_CODES, Encoding::TABLE, k, b)))
    }

    /// HIERARCHY: each row's nearest kept ancestor, through rows dropped or replaced.
    fn hierarchy(&self) -> Result<Option<Body>, Error> {
        if !self.any(Kind::HIERARCHY) {
            return Ok(None);
        }
        // The nearest kept ancestor: the chain of a kept row through dropped rows.
        let parents = self.scatter_in(Kind::HIERARCHY, None, Hierarchy::open, |h, o, r| {
            Ok(h.ancestors(r as usize).find_map(|a| self.resolve(o, a)))
        })?;
        if parents.iter().all(Option::is_none) {
            return Ok(None);
        }
        Ok(Some((
            Kind::HIERARCHY,
            Encoding::TABLE,
            u64::from(self.total),
            hierarchy::encode(&parents)?,
        )))
    }

    /// NAMES and SEARCH: a part's SEARCH keys copied, or with its name rule, rebuilt from the
    /// names it keeps.
    fn names_and_search(&self) -> Result<Vec<Body>, Error> {
        let k = u64::from(self.total);
        let mut out: Vec<Body> = Vec::new();
        let mut buf = vec![0_u8; 1 << 16];
        // Every NAMES part of each pack (ADR-0040): the merge writes them as one section.
        let rows: Vec<Vec<OwnedName>> = self.scatter_with(
            |o| {
                let has = o.section(Kind::NAMES).is_some();
                has.then(|| NameSet::open(o.part.pack.parts(Kind::NAMES)))
                    .transpose()
            },
            Vec::new(),
            |n, o, r| names_of(n, o, r, &mut buf),
        )?;
        if self.any(Kind::NAMES) {
            let recs: Vec<Vec<NameRecord<'_>>> = rows
                .iter()
                .map(|r| r.iter().map(record).collect())
                .collect();
            let per_row: Vec<&[NameRecord<'_>]> = recs.iter().map(Vec::as_slice).collect();
            out.push((Kind::NAMES, Encoding::TABLE, k, names::encode(&per_row)?));
        }
        if !self.any(Kind::SEARCH) {
            return Ok(out);
        }
        let mut keys: BTreeMap<Vec<u8>, BTreeSet<u32>> = BTreeMap::new();
        let mut rules = RuleSet::default();
        // Each part's unit types move past the earlier parts'.
        let mut shift = Vec::with_capacity(self.open.len());
        for o in &self.open {
            let set = SearchSet::open(o.part.pack.parts(Kind::SEARCH))?;
            let Some(s) = set.base() else {
                shift.push(0);
                continue;
            };
            shift.push(rules.add(s.rules())?);
            match o.part.names {
                // Every part's keys (ADR-0040), into the one SEARCH the merge writes.
                None => {
                    for p in set.parts() {
                        copy_keys(p, &o.own_of, &mut keys)?;
                    }
                }
                Some(rule) => {
                    let mut maps = search::Maps::default();
                    for &r in &o.own {
                        let new = o.own_of.get(r as usize).copied().unwrap_or(u32::MAX);
                        for (t, meta) in rows.get(new as usize).into_iter().flatten() {
                            (rule.keys)(t, names::Origin::from_code(meta.4), new, &mut maps);
                        }
                    }
                    for (key, list) in search::tagged_keys(&maps) {
                        keys.entry(key).or_default().extend(list);
                    }
                }
            }
        }
        let importance =
            self.scatter_in(Kind::SEARCH, 0, Search::open, |s, _, r| Ok(s.importance(r)))?;
        let mut units = self.scatter_in(Kind::SEARCH, 0, Search::open, |s, o, r| {
            Ok(match s.unit(r) {
                0 => 0,
                u => u.saturating_add(shift.get(o.at).copied().unwrap_or(0)),
            })
        })?;
        if units.iter().all(|&u| u == 0) {
            units.clear();
        }
        let extra = search::Extra {
            rules: (!rules.list.is_empty()).then_some(rules.list.as_slice()),
            units: &units,
            part: false,
        };
        out.push((
            Kind::SEARCH,
            Encoding::TABLE,
            k,
            search::encode_with(&keys, &importance, &extra)?,
        ));
        Ok(out)
    }

    /// ARCS, POLYGONS, `POLY_INDEX` and `ADMIN_REGION` of the polygons on kept rows.
    fn geometry(&self, opts: &Options) -> Result<Vec<Body>, Error> {
        let mut shapes = Shapes::default();
        let mut node = None;
        for o in &self.open {
            let (Some(a), Some(p)) = (o.section(Kind::ARCS), o.section(Kind::POLYGONS)) else {
                continue;
            };
            let polys = Polygons::open(&p)?;
            if node.is_none() {
                node = o
                    .section(Kind::POLY_INDEX)
                    .map(|s| PolyIndex::open(&s, &polys).map(|i| i.node_size()))
                    .transpose()?;
            }
            let g = Geometry {
                arcs: &Arcs::open(&a)?,
                polys: &polys,
                count: p.item_count(),
            };
            g.collect(o, &mut shapes)?;
        }
        shapes.encode(node.unwrap_or(NODE), opts)
    }
}

/// The names of row `r` of part `o` that its rule keeps (the canonical always).
fn names_of(
    names: &NameSet<'_>,
    o: &Opened<'_, '_>,
    r: u32,
    buf: &mut [u8],
) -> Result<Vec<OwnedName>, Error> {
    let mut kept = Vec::new();
    for (i, x) in names.names(r as usize).enumerate() {
        let text = x.text(buf)?;
        let meta: NameMeta = (x.lang(), x.script(), x.kind(), x.valid(), x.rule());
        if i > 0
            && o.part
                .names
                .is_some_and(|rule| !(rule.keep)(r, i, &record_of(text, &meta)))
        {
            continue;
        }
        kept.push((String::from(text), meta));
    }
    Ok(kept)
}

/// The record of an owned name.
fn record((text, meta): &OwnedName) -> NameRecord<'_> {
    record_of(text, meta)
}

/// The record of name `text` with `meta`.
fn record_of<'t>(text: &'t str, meta: &NameMeta) -> NameRecord<'t> {
    let &(lang, script, kind, valid, rule) = meta;
    NameRecord {
        text,
        lang,
        script,
        kind,
        valid_from: valid.0,
        valid_to: valid.1,
        rule,
    }
}

/// Adds SEARCH `s`'s keys to `keys`, each with its rows renumbered by `own_of` (a key left with
/// none is dropped).
fn copy_keys(
    s: &Search<'_>,
    own_of: &[u32],
    keys: &mut BTreeMap<Vec<u8>, BTreeSet<u32>>,
) -> Result<(), Error> {
    let mut buf = [0_u8; search::fst::MAX_KEY];
    for o in 0..s.keys() {
        let len = s.automaton().key(o, &mut buf).ok_or(fail("a SEARCH key"))?;
        let mut rows = BTreeSet::new();
        s.rows_of(o, |r| {
            if let Some(&n) = own_of.get(r as usize).filter(|&&n| n != u32::MAX) {
                rows.insert(n);
            }
            core::ops::ControlFlow::Continue(())
        });
        if !rows.is_empty() {
            let key = buf.get(..len).ok_or(fail("a SEARCH key"))?;
            keys.entry(key.to_vec()).or_default().extend(rows);
        }
    }
    Ok(())
}

/// The union of the parts' query rules.
#[derive(Default)]
struct RuleSet<'a> {
    list: Vec<search::rules::Rule<'a>>,
    /// The unit types taken so far.
    units: u8,
}

impl<'a> RuleSet<'a> {
    /// Adds a part's rules; returns how far its unit types move.
    fn add(&mut self, rules: search::rules::Rules<'a>) -> Result<u8, Error> {
        use search::rules::Rule;
        let shift = self.units;
        for r in &rules {
            match r {
                Rule::Unit { abbreviation, unit } => {
                    let unit = unit
                        .checked_add(shift)
                        .ok_or(fail("the parts name more than 255 unit types"))?;
                    self.units = self.units.max(unit);
                    self.list.push(Rule::Unit { abbreviation, unit });
                }
                Rule::Phonetic(name) => match self.list.iter().find_map(|x| match x {
                    Rule::Phonetic(n) => Some(*n),
                    _ => None,
                }) {
                    Some(n) if n != name => return Err(fail("the parts name two phonetic keys")),
                    Some(_) => {}
                    None => self.list.push(r),
                },
                _ if self.list.contains(&r) => {}
                _ => self.list.push(r),
            }
        }
        Ok(shift)
    }
}

/// The kept polygons of every part, on their arcs.
#[derive(Default)]
struct Shapes {
    /// Each new arc's points.
    points: Vec<Vec<Point<E7>>>,
    polys: Vec<Shape>,
}

/// A kept polygon: its row's admin level and new row, its parts' rings of new arc references
/// (`arc << 1 | reversed`), its box, the arcs it uses, its label and area.
struct Shape {
    level: u8,
    row: u32,
    refs: Vec<Vec<Vec<u32>>>,
    rect: Rect,
    arcs: Vec<u32>,
    label: Point<E7>,
    area_ha: u32,
}

/// A pack's admin geometry.
struct Geometry<'g, 'a> {
    arcs: &'g Arcs<'a>,
    polys: &'g Polygons<'a>,
    count: u64,
}

impl Geometry<'_, '_> {
    /// Adds the polygons on part `o`'s own rows to `shapes`, with the arcs they use at the
    /// part's level of detail.
    fn collect(&self, o: &Opened<'_, '_>, shapes: &mut Shapes) -> Result<(), Error> {
        let mut kept = Vec::new();
        let mut used = BTreeSet::new();
        for p in 0..usize::try_from(self.count).map_err(|_| fail("too many polygons"))? {
            let Some(row) = self.polys.row(p) else {
                continue;
            };
            let Some(&new) = o.own_of.get(row as usize).filter(|&&n| n != u32::MAX) else {
                continue;
            };
            for ring in self.polys.all_rings(p) {
                used.extend(self.polys.refs(ring).map(|(a, _)| a));
            }
            let level = o
                .places
                .admin_level(row as usize)
                .ok_or(fail("a row without a level"))?;
            kept.push((p, new, level));
        }
        let first = u32::try_from(shapes.points.len()).map_err(|_| fail("too many arcs"))?;
        let arc_new: BTreeMap<usize, u32> = used.iter().copied().zip(first..).collect();
        shapes.points.extend(
            used.iter()
                .map(|&a| self.arcs.points(a, false, o.part.lod).collect::<Vec<_>>()),
        );
        if let Some(rewrite) = o.part.arcs {
            (rewrite.run)(shapes.points.get_mut(first as usize..).unwrap_or_default());
        }
        for (p, row, level) in kept {
            let mut refs = Vec::new();
            let mut rect: Option<Rect> = None;
            let mut arcs_of = Vec::new();
            for part in self.polys.parts(p).ok_or(fail("a polygon's parts"))? {
                let mut rings = Vec::new();
                for ring in self.polys.rings(part).ok_or(fail("a part's rings"))? {
                    let mut list = Vec::new();
                    for (a, rev) in self.polys.refs(ring) {
                        let na = *arc_new.get(&a).ok_or(fail("an arc not collected"))?;
                        list.push(na << 1 | u32::from(rev));
                        arcs_of.push(na);
                        for v in shapes.points.get(na as usize).into_iter().flatten() {
                            let r = Rect::around_e7(v.lat(), v.lon());
                            match &mut rect {
                                Some(b) => b.grow(&r),
                                None => rect = Some(r),
                            }
                        }
                    }
                    rings.push(list);
                }
                refs.push(rings);
            }
            shapes.polys.push(Shape {
                level,
                row,
                refs,
                rect: rect.ok_or(fail("a polygon without vertices"))?,
                arcs: arcs_of,
                label: self
                    .polys
                    .label(p)
                    .ok_or(fail("a polygon without a label"))?,
                area_ha: self.polys.area_ha(p).unwrap_or(0),
            });
        }
        Ok(())
    }
}

impl Shapes {
    /// ARCS, POLYGONS (grouped by admin level, rows increasing in each), `POLY_INDEX` and
    /// `ADMIN_REGION`.
    fn encode(mut self, node: usize, opts: &Options) -> Result<Vec<Body>, Error> {
        if self.polys.is_empty() {
            return Ok(Vec::new());
        }
        self.polys.sort_by_key(|s| (s.level, s.row));
        let ring_refs: Vec<Vec<Vec<&[u32]>>> = self
            .polys
            .iter()
            .map(|p| {
                p.refs
                    .iter()
                    .map(|r| r.iter().map(Vec::as_slice).collect())
                    .collect()
            })
            .collect();
        let part_refs: Vec<Vec<&[&[u32]]>> = ring_refs
            .iter()
            .map(|p| p.iter().map(Vec::as_slice).collect())
            .collect();
        let mut input: Vec<PolygonInput<'_>> = self
            .polys
            .iter()
            .zip(&part_refs)
            .map(|(s, parts)| PolygonInput {
                row: s.row,
                parts,
                label: s.label,
                area_ha: s.area_ha,
            })
            .collect();
        let mut groups = Vec::new();
        for (i, s) in self.polys.iter().enumerate() {
            if i == 0 || self.polys.get(i - 1).map(|q| q.level) != Some(s.level) {
                groups.push(u32::try_from(i).map_err(|_| fail("too many polygons"))?);
            }
        }
        let arc_slices: Vec<&[Point<E7>]> = self.points.iter().map(Vec::as_slice).collect();
        let arcs_body = arcs::encode(&ArcColumns {
            arcs: &arc_slices,
            levels: &[],
            tolerances_cm: &[],
        })?;
        let lon_lat: Vec<Vec<[i32; 2]>> = self
            .points
            .iter()
            .map(|a| a.iter().map(|v| [v.lon(), v.lat()]).collect())
            .collect();
        // The region's run scales are the new arcs' own (a source on a coarse decimal grid has
        // them), so read them back from the encoded ARCS.
        let arc_count = self.points.len() as u64;
        let new_arcs = Arcs::open(&crate::Section::unpacked(Kind::ARCS, arc_count, &arcs_body))?;
        let scales: Vec<u32> = if new_arcs.has_scale() {
            (0..new_arcs.len()).map(|a| new_arcs.scale(a)).collect()
        } else {
            Vec::new()
        };
        let arcs_per_polygon: Vec<Vec<u32>> = self.polys.iter().map(|s| s.arcs.clone()).collect();
        let admin = region::encode_on_arcs(&lon_lat, &scales, &arcs_per_polygon, opts)?;
        let polygons_body = labels_inside(&mut input, &groups, &new_arcs)?;
        let boxes: Vec<Rect> = self.polys.iter().map(|s| s.rect).collect();
        let n = self.polys.len() as u64;
        Ok(vec![
            (Kind::ARCS, Encoding::TABLE, arc_count, arcs_body),
            (Kind::POLYGONS, Encoding::TABLE, n, polygons_body),
            (
                Kind::POLY_INDEX,
                Encoding::TABLE,
                n,
                poly_index::encode(&boxes, node)?,
            ),
            (Kind::ADMIN_REGION, Encoding::TABLE, n, admin),
        ])
    }
}

/// POLYGONS of `input`, after moving each label the coarser outline no longer holds (a small
/// island's) to a point inside it.
fn labels_inside(
    input: &mut [PolygonInput<'_>],
    groups: &[u32],
    arcs: &Arcs<'_>,
) -> Result<Vec<u8>, Error> {
    let first = polygons::encode(input, groups)?;
    let check = Polygons::open(&crate::Section::unpacked(
        Kind::POLYGONS,
        input.len() as u64,
        &first,
    ))?;
    let mut moved = 0;
    for (i, poly) in input.iter_mut().enumerate() {
        if check.contains(arcs, i, poly.label) {
            continue;
        }
        poly.label = check
            .parts(i)
            .into_iter()
            .flatten()
            .find_map(|part| {
                let rings: Vec<Vec<Point<E7>>> = check
                    .rings(part)?
                    .map(|r| check.ring(arcs, r, 0).collect())
                    .collect();
                interior(&rings).filter(|&at| check.contains(arcs, i, at))
            })
            .ok_or(fail("no point inside a coarse polygon"))?;
        moved += 1;
    }
    if moved == 0 {
        Ok(first)
    } else {
        polygons::encode(input, groups)
    }
}

/// A point inside a part (its outer ring first, then holes): on a horizontal line through the
/// outer ring's box at 1/2, 1/4, 3/4, … of its height, the middle of the widest span between
/// crossings. `None` if no line finds one.
fn interior(rings: &[Vec<Point<E7>>]) -> Option<Point<E7>> {
    let outer = rings.first()?;
    let (lo, hi) = outer.iter().fold((i32::MAX, i32::MIN), |(lo, hi), v| {
        (lo.min(v.lat()), hi.max(v.lat()))
    });
    let h = i64::from(hi) - i64::from(lo);
    let mut xs: Vec<f64> = Vec::new();
    for k in [4_i64, 2, 6, 1, 3, 5, 7] {
        let y = i32::try_from(i64::from(lo) + h * k / 8).ok()?;
        xs.clear();
        for ring in rings {
            for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
                if (a.lat() > y) != (b.lat() > y) {
                    let t = f64::from(y - a.lat()) / f64::from(b.lat() - a.lat());
                    xs.push(f64::from(a.lon()) + t * f64::from(b.lon() - a.lon()));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        let best = xs
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[a, b]| (b - a, a.midpoint(b)))
            .max_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((width, mid)) = best
            && width >= 2.0
        {
            #[allow(clippy::cast_possible_truncation, reason = "between two longitudes")]
            return Point::<E7>::from_units(y, mid as i32);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use super::{E7, Point, RuleSet, interior};
    use crate::search::rules::{self, Rule, Rules};
    use alloc::vec::Vec;

    #[test]
    fn the_parts_rules_join_and_their_unit_types_move() {
        let a = rules::encode(&[
            Rule::Phonetic("indic-v0"),
            Rule::Unit {
                abbreviation: "rm",
                unit: 3,
            },
            Rule::Drop("dham"),
        ])
        .unwrap();
        let b = rules::encode(&[
            Rule::Unit {
                abbreviation: "tsl",
                unit: 2,
            },
            Rule::Drop("dham"),
            Rule::Phonetic("indic-v0"),
        ])
        .unwrap();
        let mut set = RuleSet::default();
        assert_eq!(set.add(Rules::new(&a)).unwrap(), 0);
        assert_eq!(
            set.add(Rules::new(&b)).unwrap(),
            3,
            "b's types move past a's"
        );
        assert_eq!(
            set.list,
            [
                Rule::Phonetic("indic-v0"),
                Rule::Unit {
                    abbreviation: "rm",
                    unit: 3
                },
                Rule::Drop("dham"),
                Rule::Unit {
                    abbreviation: "tsl",
                    unit: 5
                },
            ]
        );
        // Two phonetic keys cannot share one map.
        let c = rules::encode(&[Rule::Phonetic("other-v0")]).unwrap();
        assert!(set.add(Rules::new(&c)).is_err());
        // More than 255 unit types between the parts.
        let d = rules::encode(&[Rule::Unit {
            abbreviation: "x",
            unit: 255,
        }])
        .unwrap();
        assert!(RuleSet::default().add(Rules::new(&d)).is_ok());
        let mut full = RuleSet::default();
        full.add(Rules::new(&a)).unwrap();
        assert!(full.add(Rules::new(&d)).is_err());
    }

    fn ring(v: &[(i32, i32)]) -> Vec<Point<E7>> {
        v.iter()
            .map(|&(x, y)| Point::<E7>::from_units(y, x).unwrap())
            .collect()
    }

    #[test]
    fn an_interior_point_avoids_the_hole() {
        // A 100 × 100 square with a hole over its middle band, x 10 to 90.
        let outer = ring(&[(0, 0), (100, 0), (100, 100), (0, 100)]);
        let hole = ring(&[(10, 40), (10, 60), (90, 60), (90, 40)]);
        let p = interior(&[outer.clone(), hole]).unwrap();
        // Half-way up is in the hole's band: the widest span there is outside the hole.
        assert!(
            p.lon() < 10 || p.lon() > 90 || !(40..=60).contains(&p.lat()),
            "{p:?}"
        );
        assert!((0..=100).contains(&p.lon()) && (0..=100).contains(&p.lat()));
        // Without the hole: the middle.
        let q = interior(&[outer]).unwrap();
        assert_eq!((q.lon(), q.lat()), (50, 50));
        // A degenerate ring has none.
        assert!(interior(&[ring(&[(0, 0), (5, 0), (9, 0)])]).is_none());
    }
}
