//! Name search across layers (feature `search`): the scoring of the engine note §3.5, the
//! query rules (D-120, D-121) and the "Name, Parent" context (D-119). A build without the
//! feature carries none of it, nor `teigeodata-names` (D-133, D-136).

use core::cell::Cell;
use core::ops::ControlFlow;

use teigeodata_core::{E6, Point};
use teigeodata_format::Kind;
use teigeodata_format::idmap::IdMap;
use teigeodata_format::places::Places;
use teigeodata_format::search::fuzzy::{Fuzzy, MAX_CHARS};
use teigeodata_format::search::{EXACT, LOOSE, MACHINE, PHONETIC, Search};
#[cfg(feature = "substring")]
use teigeodata_format::search::{SUBSTRING, SUBSTRING_MIN};
use teigeodata_names::KeyBuf;

use crate::layer::QueryRules;
use crate::query::{Hit, HitCand, PrefixCand, Scratch, index, merge};
use crate::{Engine, Error};

// Match bonuses, added to the importance (0 to 255): S3's, one set for every country (D-052).
const EXACT_BONUS: i32 = 60;
const LOOSE_BONUS: i32 = 55;
const FUZZY_BONUS: i32 = 35;
const FUZZY_PER_EDIT: i32 = 10;
const PREFIX_BONUS: i32 = 30;
/// Added to a row whose parent (or the row itself) the query names after a comma: a named
/// parent outranks every other match (ADR-0032).
#[cfg(feature = "admin")]
const CONTEXT_BONUS: i32 = 1_000;
/// The most rows a context may name in one layer (a short prefix names many).
const CONTEXT_MAX: usize = 4_096;
/// Edits a context may differ by, as S3 (D-052).
const CONTEXT_EDITS: u8 = 2;
/// A phonetic match: a low-weight safety net that never decides alone (ADR-0032).
const PHONETIC_BONUS: i32 = 5;
/// An exact match of a name a program wrote (D-228): below every match of a name a person
/// wrote (exact 60, loose 55, after a dropped suffix 50) and level with a prefix.
const MACHINE_BONUS: i32 = 30;
/// A loose match once a droppable suffix is removed ("Janakpurdham").
const DROP_BONUS: i32 = LOOSE_BONUS - 5;
/// Added to a row of the unit type a query names by its abbreviation ("Janaki RM").
const UNIT_BONUS: i32 = 20;
/// A match this good (one edit or better) ends the search for any first letter.
const STRONG: i32 = FUZZY_BONUS - FUZZY_PER_EDIT;
/// The prefix tier takes the best rows by importance (D-086).
const PREFIX_K: usize = 16;
/// The prefix tier runs from this many characters (code points of the folded query): two, so
/// a half-typed Brahmic syllable lists its names while typing (`हङ्` folds to `हं`, TG-18,
/// D-236). Keystroke p99 on the `tithipatra` layer 72 → 286 µs, still a tenth of a frame on a
/// low-end phone (G-phone).
const PREFIX_MIN: usize = 2;
/// A context ("Name, Parent") is matched as a prefix from this many characters (D-119).
const CONTEXT_PREFIX_MIN: usize = 3;
/// A substring match (D-199): below prefix (30) and one edit (25), above two edits (15) and
/// phonetic (5). Not strong, so it never stops the unanchored fuzzy pass.
#[cfg(feature = "substring")]
const SUBSTRING_BONUS: i32 = 20;
/// The substring tier takes the best rows by importance over every layer, as the prefix tier.
#[cfg(feature = "substring")]
const SUBSTRING_K: usize = 16;
/// The fuzzy tier runs from this many characters (D-086).
const FUZZY_MIN: usize = 4;
/// The most a focus point adds (D-226): at the focus, halving every `scale_m` away (Pelias's
/// exponential decay). Half the exact bonus: it reorders matches of like quality and
/// importance, and never lifts a weak match over a strong one of the same importance.
const FOCUS_BONUS: f64 = 30.0;
/// In strict order each match bonus is scaled by this before importance and focus are added
/// (at most 255 + 30), so the tier decides first and they only break ties within it (D-226).
const STRICT_SCALE: i32 = 1_000;
/// A named parent in strict order: above every scaled tier (at most 80,000).
#[cfg(feature = "admin")]
const STRICT_CONTEXT_BONUS: i32 = 1_000_000;

/// How [`Engine::search_with`] ranks (D-226). The default is [`Engine::search`]'s order.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct SearchOptions {
    /// Prefer places near this point: the user's location, or a point in their home country
    /// with a wide scale.
    pub focus: Option<Focus>,
    /// Rank by match tier first (exact, loose, prefix, one edit, substring, two edits,
    /// phonetic), then importance and focus within a tier, instead of one additive score
    /// (D-052). A named parent still comes first.
    pub strict: bool,
}

impl SearchOptions {
    /// These options with `focus`.
    #[must_use]
    pub const fn with_focus(mut self, focus: Focus) -> Self {
        self.focus = Some(focus);
        self
    }

    /// These options in strict tier order, or not.
    #[must_use]
    pub const fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
}

/// A focus point and its scale: a place at the point gains 30, one `scale_m` away 15, two
/// away 7.5, and so on (D-226).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Focus {
    /// The point.
    pub at: Point<E6>,
    /// The distance, in metres, at which the gain halves; positive.
    pub scale_m: f64,
}

impl Focus {
    /// Pelias's default scale: 50 km, a city and its surroundings.
    pub const DEFAULT_SCALE_M: f64 = 50_000.0;

    /// A focus at `at` with the default scale.
    #[must_use]
    pub const fn new(at: Point<E6>) -> Self {
        Self {
            at,
            scale_m: Self::DEFAULT_SCALE_M,
        }
    }

    /// The gain of a place at `p`, rounded: 0 for a scale that is not positive.
    fn bonus(&self, p: Point<E6>) -> i32 {
        if self.scale_m.is_nan() || self.scale_m <= 0.0 {
            return 0;
        }
        let d = teigeodata_format::place_index::meters_between(
            self.at.lat_degrees(),
            self.at.lon_degrees(),
            p.lat_degrees(),
            p.lon_degrees(),
        );
        // In [0, 30]: the cast cannot truncate or wrap.
        #[allow(clippy::cast_possible_truncation, reason = "in [0, 30]")]
        let b = libm::round(FOCUS_BONUS * libm::exp2(-d / self.scale_m)) as i32;
        b
    }
}

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
    /// How to rank, and the layer's places when there is a focus.
    opts: &'q SearchOptions,
    places: Option<&'q Places<'q>>,
}

fn hit_less(a: &HitCand, b: &HitCand) -> bool {
    b.score.cmp(&a.score).then(a.id.cmp(&b.id)).is_lt()
}

impl Engine {
    /// Places matching `query`, best first (ties by ID), at most `k`, into `out`. The query is
    /// folded as the forge folds names; each layer scores its rows with the same formula
    /// (D-052): exact 60, loose 55, one edit 25 (queries of 4 to 63 characters, keys with the
    /// query's first letter; any first letter only when no layer has a visible match of one
    /// edit or better), plus the best 16 by importance over every layer for a prefix of 2
    /// characters or more (30), each plus the row's importance. With the `substring` feature, a
    /// query of 3 or more code points without a space also takes the best 16 by importance over
    /// every layer that has the `substring` component whose name holds it inside a word (20;
    /// D-199): "mandu" finds Kathmandu. After a comma the query names a
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
        self.search_with(query, k, &SearchOptions::default(), s, out)
    }

    /// [`Engine::search`] ranked by `opts` (D-226): a focus point adds up to 30 to each match,
    /// halving every `scale_m` away, before each layer keeps its best `k`; strict order ranks
    /// by match tier first. A hit's score compares only with the same query's.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt.
    pub fn search_with(
        &self,
        query: &str,
        k: usize,
        opts: &SearchOptions,
        s: &mut Scratch,
        out: &mut Vec<Hit>,
    ) -> Result<(), Error> {
        out.clear();
        s.hits.clear();
        s.segs.clear();
        s.prefix.clear();
        #[cfg(feature = "substring")]
        s.substring.clear();
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
        self.tiers_by_importance(q, chars, s)?;
        // Fuzzy keys start with the query's first letter; only when no layer has a strong match
        // (exact, loose, within one edit or a prefix) does a second pass take any first letter
        // (ADR-0032): first-letter typos are rare, and the anchored walk is several times faster.
        let mut anchor = q.chars().next();
        loop {
            let mut strong = false;
            for (i, l) in self.layers.iter().enumerate() {
                l.ensure(Kind::SEARCH)?;
                l.ensure(Kind::IDMAP)?;
                if opts.focus.is_some() {
                    l.ensure(Kind::PLACES)?;
                }
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
                    opts,
                    places: v.places.as_ref(),
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
            opts,
            places,
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
        add(search.get(MACHINE, q), MACHINE_BONUS, rows);
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
        #[cfg(feature = "substring")]
        rows.extend(
            s.substring
                .iter()
                .filter(|p| p.layer == me)
                .map(|p| (p.row, SUBSTRING_BONUS)),
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
        self.score(i, search, (opts, places), &s.context, rows);
        // Read IDs (and check visibility) only for the groups of equal score that can still
        // enter this layer's best k.
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

    /// The final scores of layer `i`'s matched `rows` (each holding its match bonus): scaled in
    /// strict order, plus the context bonus, the importance and the focus's gain (D-226).
    fn score(
        &self,
        i: usize,
        search: &Search<'_>,
        (opts, places): (&SearchOptions, Option<&Places<'_>>),
        context: &[u32],
        rows: &mut [(u32, i32)],
    ) {
        if opts.strict {
            for r in rows.iter_mut() {
                r.1 *= STRICT_SCALE;
            }
        }
        #[cfg(feature = "admin")]
        self.context_bonus(
            i,
            context,
            rows,
            if opts.strict {
                STRICT_CONTEXT_BONUS
            } else {
                CONTEXT_BONUS
            },
        );
        #[cfg(not(feature = "admin"))]
        let _ = (self, i, context);
        let focus = opts.focus.as_ref().zip(places);
        for r in rows.iter_mut() {
            r.1 += i32::from(search.importance(r.0));
            if let Some((f, places)) = focus
                && let Some(p) = places.point(r.0 as usize)
            {
                r.1 += f.bonus(p);
            }
        }
    }

    /// The tiers ranked by importance alone, over every layer: the best [`PREFIX_K`] rows
    /// under a key starting with `q` (from 2 characters), and, with the `substring` feature,
    /// the best [`SUBSTRING_K`] whose name holds `q` inside a word (one word, 3 code points or
    /// more, D-199). Only layers installed with `substring` have its section; the others add
    /// nothing to that tier.
    fn tiers_by_importance(&self, q: &str, chars: usize, s: &mut Scratch) -> Result<(), Error> {
        let by_rank =
            |a: &PrefixCand, b: &PrefixCand| b.importance.cmp(&a.importance).then(a.id.cmp(&b.id));
        if chars >= PREFIX_MIN {
            for (i, l) in self.layers.iter().enumerate() {
                l.ensure(Kind::SEARCH)?;
                l.ensure(Kind::IDMAP)?;
                let v = l.views();
                let (Some(search), Some(ids)) = (&v.search, &v.ids) else {
                    continue;
                };
                self.best_by_importance(i, search, ids, (EXACT, q), false, &mut s.prefix)?;
            }
            s.prefix.sort_unstable_by(by_rank);
            s.prefix.truncate(PREFIX_K);
        }
        #[cfg(feature = "substring")]
        if chars >= SUBSTRING_MIN && !q.contains(' ') {
            for (i, l) in self.layers.iter().enumerate() {
                let v = l.views();
                let (Some(sub), Some(ids)) = (&v.substring, &v.ids) else {
                    continue;
                };
                l.ensure(Kind::SUBSTRING)?;
                l.ensure(Kind::IDMAP)?;
                self.best_by_importance(i, sub, ids, (SUBSTRING, q), true, &mut s.substring)?;
            }
            s.substring.sort_unstable_by(by_rank);
            s.substring.truncate(SUBSTRING_K);
        }
        Ok(())
    }

    /// Appends layer `i`'s best [`PREFIX_K`] visible rows under a key of map `tag` starting
    /// with `q`, by importance. Skips keys and blocks of keys that cannot enter
    /// (`Search::best_in`). Not `strict` (the prefix tier), keys whose best row ties the worst
    /// kept one are still read once full, so ties are broken by ID; `strict` (the substring
    /// tier, D-199), they are skipped and ties fall in key order: a 3-letter substring ranges
    /// over thousands of equal keys, and reading them cost p99 7× in the note's bench.
    fn best_by_importance(
        &self,
        i: usize,
        search: &Search<'_>,
        ids: &IdMap<'_>,
        (tag, q): (u8, &str),
        strict: bool,
        prefix: &mut Vec<PrefixCand>,
    ) -> Result<(), Error> {
        let start = prefix.len();
        let floor = Cell::new(-1_i32);
        let mut failed = None;
        let me = index(i);
        search.best_in(
            search.range(tag, q),
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
                        floor.set(i32::from(low) - i32::from(!strict));
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
    /// Adds `bonus` to each of layer `i`'s `rows` whose ancestor (or itself) is one of the
    /// `context` rows.
    #[cfg(feature = "admin")]
    fn context_bonus(&self, i: usize, context: &[u32], rows: &mut [(u32, i32)], bonus: i32) {
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
                    r.1 += bonus;
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
/// form with its native generic replaced by a Latin spelling (also looked up loosely), the
/// query with a spelling convention applied (looked up loosely, D-233), and the query without a
/// droppable suffix. The bonuses as in S3 (D-052).
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
    // Each convention applied to every run it names at once: "vishnu vihar" → "bishnu bihar".
    for &(from, to) in &rules.spellings {
        if q.contains(from) {
            form.clear();
            let mut last = 0;
            for (at, _) in q.match_indices(from) {
                form.push_str(q.get(last..at).unwrap_or_default());
                form.push_str(to);
                last = at + from.len();
            }
            form.push_str(q.get(last..).unwrap_or_default());
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
    if chars >= CONTEXT_PREFIX_MIN {
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
        put(EXACT, "bishnubihar", 5);
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
            Rule::Spelling { from: "v", to: "b" },
        ];
        let extra = search::Extra {
            rules: Some(&rules),
            units: &[0, 3, 0, 0, 0, 0],
        };
        let b = search::encode_with(&keys, &[1; 6], &extra).unwrap();
        let s = Search::open(&Section::unpacked(Kind::SEARCH, 6, &b)).unwrap();
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
        // A spelling convention replaces every run at once, matched loosely (D-233).
        assert_eq!(hits("vishnu vihar"), [(5, LOOSE_BONUS)]);
        assert_eq!(hits("vishnubihar"), [(5, LOOSE_BONUS)]);
        assert_eq!(hits("vishnuvi"), []);
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
