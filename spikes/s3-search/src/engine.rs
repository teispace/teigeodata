//! The S3 query pipeline (ADR-0032, names-and-search §5), shared by `s3-eval` (an index built
//! from the sources) and `pack-eval` (the SEARCH section of a forge-built pack).
//!
//! Tiers, strongest first: exact fold (names and alternates) → loose (spaces removed, droppable
//! suffix removed, lexicon native generic replaced) → phonetic key → fuzzy (Levenshtein
//! automaton over the exact FST) → prefix. Within a tier: context match, unit-type match, then
//! importance. Results are entity groups (records of one entity across sources).

use std::collections::{BTreeMap, BTreeSet};

use fst::automaton::Str;
use fst::{Automaton, IntoStreamer, Map, MapBuilder, Streamer};
use teigeodata_format::search::fuzzy::Fuzzy;
use teigeodata_format::search::{
    self as tsearch, EXACT as T_EXACT, LOOSE as T_LOOSE, PHONETIC as T_PHON, Search,
};
use teigeodata_format::{Encoding, Kind, Pack, Writer};
use teigeodata_names::PhoneticKey;

use crate::{Rec, fold, levenshtein, lexicon, profile};

/// An FST from key to a postings list of record indexes.
pub struct Index {
    pub map: Map<Vec<u8>>,
    pub postings: Vec<Vec<u32>>,
}

/// B7: the same maps as one SEARCH section in a pack (`S3_INDEX=pack`, or a forge-built pack),
/// with a prefix tier that takes the most important keys (`S3_PREFIX=best`, the default with the
/// pack) or the first in key order (`S3_PREFIX=order`, as S3).
pub struct Packed<'a> {
    pub ix: Search<'a>,
    pub body_len: usize,
    pub columns: Vec<(u16, usize)>,
    pub best_prefix: bool,
}

impl<'a> Packed<'a> {
    /// Wraps an opened SEARCH section whose table body is `body`.
    #[must_use]
    pub fn new(ix: Search<'a>, body: &[u8]) -> Self {
        let columns = teigeodata_core::table::Table::parse(body)
            .map(|t| {
                t.ids()
                    .map(|id| (id, t.column_raw(id).map_or(0, |c| c.1.len())))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            ix,
            body_len: body.len(),
            columns,
            best_prefix: std::env::var("S3_PREFIX").map_or(true, |v| v != "order"),
        }
    }
}

fn packed(maps: [(u8, &BTreeMap<String, BTreeSet<u32>>); 3], recs: &[Rec]) -> Packed<'static> {
    let importance: Vec<u8> = recs.iter().map(|r| r.importance.min(255) as u8).collect();
    let body = if std::env::var_os("S3_NO_DEDUPE").is_none() {
        // The shared builder (D-086): what the forge writes.
        let m = tsearch::Maps {
            exact: maps[0].1.clone(),
            loose: maps[1].1.clone(),
            phonetic: maps[2].1.clone(),
        };
        tsearch::encode_maps(&m, &importance).unwrap()
    } else {
        let mut keys: BTreeMap<Vec<u8>, BTreeSet<u32>> = BTreeMap::new();
        for (tag, m) in maps {
            for (k, v) in m {
                let mut key = vec![tag];
                key.extend_from_slice(k.as_bytes());
                keys.insert(key, v.clone());
            }
        }
        tsearch::encode(&keys, &importance).unwrap()
    };
    let mut w = Writer::new("np", "search", "2026.11.0").unwrap();
    w.section_bytes(Kind::SEARCH, Encoding::TABLE, recs.len() as u64, &body)
        .unwrap();
    // Sections of 1 MiB or more carry block hashes (D-078).
    let list = teigeodata_format::blocks::encode(&[(Kind::SEARCH, &body)]).unwrap();
    if let Some(l) = &list {
        w.section_bytes(
            Kind::BLOCK_HASHES,
            Encoding::RAW,
            teigeodata_format::blocks::item_count(l),
            l,
        )
        .unwrap();
    }
    let layout = w.layout(false).unwrap();
    let refs: Vec<&[u8]> = layout
        .placed()
        .iter()
        .map(|p| {
            if p.kind == Kind::SEARCH {
                body.as_slice()
            } else {
                list.as_deref().unwrap_or(&[])
            }
        })
        .collect();
    let bytes: &'static [u8] = Box::leak(layout.assemble(&refs).unwrap().into_boxed_slice());
    let pack: &'static Pack<'static> = Box::leak(Box::new(Pack::open(bytes).unwrap()));
    let ix = Search::open(&pack.section(Kind::SEARCH).unwrap()).unwrap();
    ix.check().unwrap();
    Packed::new(ix, &body)
}

impl Index {
    fn build(entries: BTreeMap<String, BTreeSet<u32>>) -> Self {
        let mut b = MapBuilder::memory();
        let mut postings = Vec::with_capacity(entries.len());
        for (k, v) in entries {
            b.insert(k.as_bytes(), postings.len() as u64).unwrap();
            postings.push(v.into_iter().collect());
        }
        Self {
            map: Map::new(b.into_inner().unwrap()).unwrap(),
            postings,
        }
    }
    fn get(&self, k: &str) -> &[u32] {
        self.map
            .get(k)
            .map_or(&[][..], |i| &self.postings[i as usize])
    }
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.map.as_fst().as_bytes().len()
            + self.postings.iter().map(|p| 4 + 4 * p.len()).sum::<usize>()
    }
}

pub struct Engine<'a> {
    recs: &'a [Rec],
    pub exact: Index,
    pub loose: Index,
    pub phon: Index,
    /// Folded abbreviation → unit type.
    abbr: BTreeMap<String, String>,
    /// Folded native-script generic → its folded Latin spellings.
    native: Vec<(String, Vec<String>)>,
    /// Each record's folded parent, for context matching.
    districts: Vec<String>,
    droppable: Vec<String>,
    knobs: Knobs,
    /// The phonetic key of queries (and, for S3, of the index); `None` = no phonetic tier.
    phonetic: Option<PhoneticKey>,
    pub pack: Option<Packed<'a>>,
}

/// A/B switches, from the environment: `S3_FUZZY2` (the length from which two edits are
/// allowed; 0 = never), `S3_NO_FUZZY`, `S3_NO_PREFIX`, `S3_NO_PHONETIC`, `S3_PHONETIC_BONUS`, `S3_UNANCHORED`, `S3_PREFIX_SCAN`.
#[derive(Clone, Copy)]
#[allow(clippy::struct_excessive_bools, reason = "independent A/B switches")]
struct Knobs {
    fuzzy2_from: usize,
    fuzzy: bool,
    prefix: bool,
    phonetic: bool,
    phonetic_bonus: i64,
    /// Fuzzy matches must share the first byte (Lucene's `prefix_length` = 1).
    anchored: bool,
    /// How many FST keys the prefix tier reads at most.
    prefix_scan: usize,
}

impl Knobs {
    fn from_env() -> Self {
        let on = |k: &str| std::env::var_os(k).is_none();
        Self {
            fuzzy2_from: std::env::var("S3_FUZZY2")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            fuzzy: on("S3_NO_FUZZY"),
            prefix: on("S3_NO_PREFIX"),
            phonetic: on("S3_NO_PHONETIC"),
            anchored: std::env::var_os("S3_UNANCHORED").is_none(),
            prefix_scan: std::env::var("S3_PREFIX_SCAN")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(PREFIX_SCAN),
            phonetic_bonus: std::env::var("S3_PHONETIC_BONUS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(PHONETIC),
        }
    }
}

/// The phonetic key S3 tests: `indic-v0`, or `indic-v1` with `S3_PKEY=v1`.
#[must_use]
pub fn s3_pkey() -> PhoneticKey {
    if std::env::var("S3_PKEY").is_ok_and(|v| v == "v1") {
        PhoneticKey::IndicV1
    } else {
        PhoneticKey::IndicV0
    }
}

fn despace(s: &str) -> String {
    s.replace(' ', "")
}

/// The country rules the query side reads from `data/countries/<cc>/`.
struct Rules {
    droppable: Vec<String>,
    native: Vec<(String, Vec<String>)>,
    abbr: BTreeMap<String, String>,
    phonetic: Option<PhoneticKey>,
}

fn rules(cc_dir: &str) -> Rules {
    let prof = profile(&format!("{cc_dir}/profile.yaml"));
    let lex_path = format!("{cc_dir}/lexicon.yaml");
    let lex = if std::path::Path::new(&lex_path).exists() {
        lexicon(&lex_path).terms
    } else {
        Vec::new()
    };
    let droppable: Vec<String> = prof
        .droppable_suffixes
        .iter()
        .map(|t| fold(&t.text))
        .collect();
    let mut native: Vec<(String, Vec<String>)> = lex
        .iter()
        .flat_map(|t| {
            let sp: Vec<String> = t.spellings().map(|s| fold(s)).collect();
            t.native.iter().map(move |n| (fold(&n.text), sp.clone()))
        })
        .collect();
    // Longest native form first, so the longest generic wins.
    native.sort_by_key(|(n, _)| std::cmp::Reverse(n.chars().count()));
    let abbr = prof
        .unit_types
        .iter()
        .flat_map(|u| {
            u.abbreviations
                .iter()
                .map(|a| (fold(&a.text), u.id.clone()))
        })
        .collect();
    let phonetic = prof
        .phonetic_key
        .as_deref()
        .map(|n| PhoneticKey::from_name(n).unwrap_or_else(|| panic!("unknown phonetic key {n}")));
    Rules {
        droppable,
        native,
        abbr,
        phonetic,
    }
}

impl<'a> Engine<'a> {
    /// S3: builds the index from `recs` (and, with `S3_INDEX=pack`, packs it as a SEARCH
    /// section and queries that).
    #[must_use]
    pub fn new(recs: &'a [Rec], cc_dir: &str) -> Self {
        let r = rules(cc_dir);
        let pkey = s3_pkey();
        let (mut exact, mut loose, mut phon) = (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
        let add = |m: &mut BTreeMap<String, BTreeSet<u32>>, k: String, i: u32| {
            if !k.is_empty() {
                m.entry(k).or_default().insert(i);
            }
        };
        for (i, rec) in recs.iter().enumerate() {
            let i = i as u32;
            for n in std::iter::once(&rec.name).chain(&rec.alts) {
                // The shared key builder: what the forge writes. The phonetic key is added
                // here, since the spike can switch it (`S3_PKEY`).
                teigeodata_names::search_keys(n, &r.droppable, None, |map, k| match map {
                    teigeodata_names::SearchMap::Exact => add(&mut exact, k, i),
                    _ => add(&mut loose, k, i),
                });
                add(&mut phon, pkey.key(n), i);
            }
        }
        let pack = (std::env::var("S3_INDEX").is_ok_and(|v| v == "pack")).then(|| {
            packed(
                [(T_EXACT, &exact), (T_LOOSE, &loose), (T_PHON, &phon)],
                recs,
            )
        });
        Self {
            pack,
            recs,
            exact: Index::build(exact),
            loose: Index::build(loose),
            phon: Index::build(phon),
            abbr: r.abbr,
            native: r.native,
            knobs: Knobs::from_env(),
            phonetic: Some(pkey),
            districts: recs.iter().map(|r| fold(&r.district)).collect(),
            droppable: r.droppable,
        }
    }

    /// The same pipeline over a SEARCH section the forge wrote: record `i` is pack row `i`, and
    /// the query's phonetic key is the profile's (what the forge keyed the names with).
    #[must_use]
    pub fn on_pack(recs: &'a [Rec], pack: Packed<'a>, cc_dir: &str) -> Self {
        let r = rules(cc_dir);
        let empty = || Index::build(BTreeMap::new());
        Self {
            pack: Some(pack),
            recs,
            exact: empty(),
            loose: empty(),
            phon: empty(),
            abbr: r.abbr,
            native: r.native,
            knobs: Knobs::from_env(),
            phonetic: r.phonetic,
            districts: recs.iter().map(|r| fold(&r.district)).collect(),
            droppable: r.droppable,
        }
    }

    /// The phonetic key of `s` under this engine's algorithm (empty without one).
    #[must_use]
    pub fn pkey(&self, s: &str) -> String {
        self.phonetic.map_or_else(String::new, |p| p.key(s))
    }

    /// Adds every record whose folded name is within `dist` OSA edits of `q`.
    fn fuzzy(&self, q: &str, dist: u8, anchored: bool, best: &mut BTreeMap<u32, i64>) {
        if let Some(p) = &self.pack {
            let Some(f) = Fuzzy::new(q, dist) else { return };
            let anchor = if anchored { q.chars().next() } else { None };
            p.ix.walk(T_EXACT, &f, anchor, usize::MAX, |o, e| {
                let bonus = FUZZY - FUZZY_PER_EDIT * i64::from(e);
                p.ix.rows_of(o, |i| {
                    let v = best.entry(i).or_insert(i64::MIN);
                    *v = (*v).max(bonus);
                    std::ops::ControlFlow::Continue(())
                });
                std::ops::ControlFlow::Continue(())
            });
            return;
        }
        let aut = Damerau::new(q, dist);
        if anchored {
            let first = &q[..q.chars().next().map_or(0, char::len_utf8)];
            self.collect(aut.intersection(Str::new(first).starts_with()), q, best);
        } else {
            self.collect(aut, q, best);
        }
    }

    fn collect<A: Automaton>(&self, aut: A, q: &str, best: &mut BTreeMap<u32, i64>) {
        let mut s = self.exact.map.search(aut).into_stream();
        while let Some((key, v)) = s.next() {
            let bonus = FUZZY - FUZZY_PER_EDIT * osa(key, q.as_bytes()) as i64;
            for &i in &self.exact.postings[v as usize] {
                let e = best.entry(i).or_insert(i64::MIN);
                *e = (*e).max(bonus);
            }
        }
    }

    /// The rows of `key` in map `tag`, from the pack or the S3 index.
    fn get(&self, tag: u8, key: &str) -> Vec<u32> {
        if let Some(p) = &self.pack {
            let mut v = Vec::new();
            let mut add = |tag| {
                if let Some(o) = p.ix.get(tag, key) {
                    p.ix.rows_of(o, |r| {
                        v.push(r);
                        std::ops::ControlFlow::Continue(())
                    });
                }
            };
            add(tag);
            if tag == T_LOOSE {
                add(T_EXACT);
            }
            return v;
        }
        let idx = match tag {
            T_EXACT => &self.exact,
            T_LOOSE => &self.loose,
            _ => &self.phon,
        };
        idx.get(key).to_vec()
    }

    /// The prefix tier on the pack: the first `prefix_scan` keys in order (as S3), or the rows
    /// of highest importance (the best `K`, skipping keys and blocks that cannot enter).
    fn prefix_packed(&self, p: &Packed<'_>, q: &str, best: &mut BTreeMap<u32, i64>) {
        const K: usize = 16;
        let range = p.ix.range(T_EXACT, q);
        if !p.best_prefix {
            for o in range.take(self.knobs.prefix_scan) {
                p.ix.rows_of(o, |i| {
                    let v = best.entry(i).or_insert(i64::MIN);
                    *v = (*v).max(PREFIX);
                    std::ops::ControlFlow::Continue(())
                });
            }
            return;
        }
        // The best K rows by importance, as (importance, row), and the floor to beat once full.
        let top = std::cell::RefCell::new(Vec::<(u8, u32)>::with_capacity(K));
        let low = std::cell::Cell::new(-1_i32);
        p.ix.best_in(
            range,
            || low.get(),
            |o| {
                p.ix.rows_of(o, |r| {
                    let imp = p.ix.importance(r);
                    if i32::from(imp) <= low.get() {
                        return std::ops::ControlFlow::Break(());
                    }
                    let mut t = top.borrow_mut();
                    if t.iter().any(|e| e.1 == r) {
                        return std::ops::ControlFlow::Continue(());
                    }
                    if t.len() == K {
                        let at = t
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, e)| e.0)
                            .map_or(0, |(i, _)| i);
                        t.swap_remove(at);
                    }
                    t.push((imp, r));
                    if t.len() == K {
                        low.set(i32::from(t.iter().map(|e| e.0).min().unwrap_or(0)));
                    }
                    std::ops::ControlFlow::Continue(())
                });
                std::ops::ControlFlow::Continue(())
            },
        );
        for &(_, i) in top.borrow().iter() {
            let v = best.entry(i).or_insert(i64::MIN);
            *v = (*v).max(PREFIX);
        }
    }

    /// Returns entity groups, best first, at most `k`.
    #[must_use]
    pub fn search(&self, query: &str, k: usize) -> Vec<usize> {
        // Context: "Name, Parent".
        let (name_part, ctx) = match query.split_once(',') {
            Some((n, c)) => (n, Some(fold(c))),
            None => (query, None),
        };
        let mut q = fold(name_part);
        // A trailing unit-type abbreviation from the profile.
        let mut unit = None;
        if let Some((stem, last)) = q.rsplit_once(' ')
            && let Some(u) = self.abbr.get(last)
        {
            unit = Some(u.as_str());
            q = stem.to_string();
        }
        // Native-script generics → each Latin spelling, for mixed-script queries.
        let mut forms = vec![q.clone()];
        if let Some((n, spellings)) = self.native.iter().find(|(n, _)| q.ends_with(n.as_str())) {
            let stem = &q[..q.len() - n.len()];
            forms.extend(spellings.iter().map(|sp| format!("{stem}{sp}")));
        }

        // Match bonus per record, the best over all ways it matched.
        let mut best: BTreeMap<u32, i64> = BTreeMap::new();
        let hit = |best: &mut BTreeMap<u32, i64>, i: u32, bonus: i64| {
            let e = best.entry(i).or_insert(i64::MIN);
            *e = (*e).max(bonus);
        };
        for i in self.get(T_EXACT, &q) {
            hit(&mut best, i, EXACT);
        }
        for f in &forms {
            for i in self.get(T_LOOSE, &despace(f)) {
                hit(&mut best, i, LOOSE);
            }
            if self.knobs.phonetic
                && let Some(p) = self.phonetic
            {
                for i in self.get(T_PHON, &p.key(f)) {
                    hit(&mut best, i, self.knobs.phonetic_bonus);
                }
            }
        }
        for d in &self.droppable {
            if let Some(stem) = q.strip_suffix(d.as_str()) {
                for i in self.get(T_LOOSE, &despace(stem)) {
                    hit(&mut best, i, LOOSE - 5);
                }
            }
        }
        // The fuzzy length window: bytes for S3's byte automaton, characters for the pack's.
        let n = if self.pack.is_some() {
            q.chars().count()
        } else {
            q.len()
        };
        let fuzzy_max = if self.pack.is_some() {
            teigeodata_format::search::fuzzy::MAX_CHARS
        } else {
            FUZZY_MAX_LEN
        };
        if self.knobs.fuzzy && (4..=fuzzy_max).contains(&n) {
            self.fuzzy(&q, 1, self.knobs.anchored, &mut best);
        }
        if self.knobs.prefix
            && n >= 3
            && let Some(p) = &self.pack
        {
            self.prefix_packed(p, &q, &mut best);
        } else if self.knobs.prefix && n >= 3 {
            let mut s = self
                .exact
                .map
                .search(Str::new(&q).starts_with())
                .into_stream();
            let mut seen = 0;
            while let Some((_, v)) = s.next() {
                for &i in &self.exact.postings[v as usize] {
                    hit(&mut best, i, PREFIX);
                }
                seen += 1;
                if seen >= self.knobs.prefix_scan {
                    break;
                }
            }
        }

        // Fallbacks when no strong match (exact, loose or one edit; prefix and phonetic hits do
        // not count): one edit anywhere (first-letter typos), then two edits.
        let strong =
            |best: &BTreeMap<u32, i64>| best.values().any(|&b| b >= FUZZY - FUZZY_PER_EDIT);
        if self.knobs.fuzzy && !strong(&best) && (4..=fuzzy_max).contains(&n) {
            if self.knobs.anchored {
                self.fuzzy(&q, 1, false, &mut best);
            }
            let f2 = self.knobs.fuzzy2_from;
            if !strong(&best) && f2 > 0 && n >= f2 {
                self.fuzzy(&q, 2, false, &mut best);
            }
        }

        // Group by entity: (context match, score). Score = match bonus + importance + unit bonus.
        let mut groups: BTreeMap<usize, (bool, i64)> = BTreeMap::new();
        for (&i, &bonus) in &best {
            let r = &self.recs[i as usize];
            let c = ctx.as_ref().is_some_and(|c| {
                let d = &self.districts[i as usize];
                !d.is_empty() && (d == c || d.starts_with(c.as_str()) || levenshtein(d, c) <= 2)
            });
            let u = unit.is_some_and(|u| r.unit_type.as_deref() == Some(u));
            let score = bonus + i64::from(r.importance) + if u { UNIT } else { 0 };
            let e = groups.entry(r.group).or_insert((c, score));
            e.0 |= c;
            e.1 = e.1.max(score);
        }
        let mut v: Vec<(usize, (bool, i64))> = groups.into_iter().collect();
        // A context match outranks everything else: the user named the parent.
        v.sort_by_key(|(g, (c, s))| (!c, std::cmp::Reverse(*s), *g));
        v.truncate(k);
        v.into_iter().map(|(g, _)| g).collect()
    }
}

// Match bonuses, added to the 0–130 importance. One set for every country.
const EXACT: i64 = 60;
const LOOSE: i64 = 55;
const PHONETIC: i64 = 5;
const FUZZY: i64 = 35;
const FUZZY_PER_EDIT: i64 = 10;
const PREFIX: i64 = 30;
const UNIT: i64 = 20;
const PREFIX_SCAN: usize = 256;

/// Optimal-string-alignment distance (Levenshtein plus adjacent transposition), on bytes.
fn osa(a: &[u8], b: &[u8]) -> usize {
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=m {
        d[0][j] = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    d[n][m]
}

/// The longest query the fuzzy automaton takes, in bytes; longer queries skip fuzzy matching.
const FUZZY_MAX_LEN: usize = 31;

/// A Damerau (OSA) automaton over bytes for FST search: the state is the last two DP rows and
/// the last byte, in fixed arrays (no allocation per transition), pruned as soon as no cell is
/// within the distance.
struct Damerau<'a> {
    q: &'a [u8],
    max: u8,
}

impl<'a> Damerau<'a> {
    fn new(q: &'a str, max: u8) -> Self {
        Self {
            q: q.as_bytes(),
            max,
        }
    }
}

#[derive(Clone, Copy)]
struct DState {
    prev: [u8; FUZZY_MAX_LEN + 1],
    cur: [u8; FUZZY_MAX_LEN + 1],
    last: u8,
    has_prev: bool,
}

impl Automaton for Damerau<'_> {
    type State = Option<DState>;
    fn start(&self) -> Self::State {
        let mut cur = [0u8; FUZZY_MAX_LEN + 1];
        for (j, c) in cur.iter_mut().enumerate().take(self.q.len() + 1) {
            *c = (j as u8).min(self.max + 1);
        }
        Some(DState {
            prev: [0; FUZZY_MAX_LEN + 1],
            cur,
            last: 0,
            has_prev: false,
        })
    }
    fn is_match(&self, s: &Self::State) -> bool {
        s.as_ref().is_some_and(|s| s.cur[self.q.len()] <= self.max)
    }
    fn can_match(&self, s: &Self::State) -> bool {
        s.is_some()
    }
    fn accept(&self, s: &Self::State, b: u8) -> Self::State {
        let s = s.as_ref()?;
        let q = self.q;
        let cap = self.max + 1;
        let mut next = [0u8; FUZZY_MAX_LEN + 1];
        next[0] = (s.cur[0] + 1).min(cap);
        let mut alive = next[0] <= self.max;
        for j in 1..=q.len() {
            let cost = u8::from(q[j - 1] != b);
            let mut v = (s.cur[j] + 1).min(next[j - 1] + 1).min(s.cur[j - 1] + cost);
            if j > 1 && s.has_prev && b == q[j - 2] && s.last == q[j - 1] {
                v = v.min(s.prev[j - 2] + 1);
            }
            next[j] = v.min(cap);
            alive |= next[j] <= self.max;
        }
        alive.then_some(DState {
            prev: s.cur,
            cur: next,
            last: b,
            has_prev: true,
        })
    }
}
