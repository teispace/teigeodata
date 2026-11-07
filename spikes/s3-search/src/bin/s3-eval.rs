//! Spike S3: evaluates a name-search design on one country's golden set (ADR-0032,
//! names-and-search §5). World-general (principle P16): every country rule (abbreviations,
//! droppable suffixes, lexicon) comes from `data/countries/<cc>/`; the phonetic key is a named
//! algorithm from the library.
//!
//! Usage: `s3-eval <cc> <GeoNames CC.txt> [<COD-AB workbook.xlsx>]`
//!
//! Tiers, strongest first: exact fold (names and alternates) → loose (spaces removed, droppable
//! suffix removed, lexicon native generic replaced) → phonetic key → fuzzy (Levenshtein
//! automaton over the exact FST) → prefix. Within a tier: context match, unit-type match, then
//! importance. Results are entity groups (records of one entity across sources).
#![allow(
    clippy::print_stdout,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::struct_excessive_bools,
    clippy::needless_range_loop,
    missing_docs,
    reason = "an evaluation spike over pinned local inputs"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use fst::automaton::Str;
use fst::{Automaton, IntoStreamer, Map, MapBuilder, Streamer};
use serde::Deserialize;
use spike_s3_search::{
    Rec, fold, levenshtein, lexicon, load, pkey_indic_v0, pkey_indic_v1, profile,
};
use teigeodata_format::search::fuzzy::Fuzzy;
use teigeodata_format::search::{
    self as tsearch, EXACT as T_EXACT, LOOSE as T_LOOSE, PHONETIC as T_PHON, Search,
};
use teigeodata_format::{Encoding, Kind, Pack, Writer};

#[derive(Deserialize)]
struct GoldenFile {
    queries: Vec<GoldenQuery>,
}

#[derive(Deserialize)]
struct GoldenQuery {
    id: String,
    class: String,
    query: String,
    expect: BTreeMap<String, u8>,
    max_rank: u8,
}

/// An FST from key to a postings list of record indexes.
struct Index {
    map: Map<Vec<u8>>,
    postings: Vec<Vec<u32>>,
}

/// B7: the same maps as one SEARCH section in a pack (`S3_INDEX=pack`), with a prefix tier
/// that takes the most important keys (`S3_PREFIX=best`, the default with the pack) or the
/// first in key order (`S3_PREFIX=order`, as S3).
struct Packed {
    ix: Search<'static>,
    body_len: usize,
    columns: Vec<(u16, usize)>,
    best_prefix: bool,
}

fn packed(maps: [(u8, &BTreeMap<String, BTreeSet<u32>>); 3], recs: &[Rec]) -> Packed {
    let mut keys: BTreeMap<Vec<u8>, BTreeSet<u32>> = BTreeMap::new();
    let exact = maps[0].1;
    let dedupe = std::env::var_os("S3_NO_DEDUPE").is_none();
    for (tag, m) in maps {
        for (k, v) in m {
            // A loose key keeps only the rows its exact twin (the same text) lacks: a loose
            // lookup reads both.
            let v: BTreeSet<u32> = match exact.get(k) {
                Some(e) if dedupe && tag == T_LOOSE => v.difference(e).copied().collect(),
                _ => v.clone(),
            };
            if v.is_empty() {
                continue;
            }
            let mut key = vec![tag];
            key.extend_from_slice(k.as_bytes());
            keys.insert(key, v);
        }
    }
    let importance: Vec<u8> = recs.iter().map(|r| r.importance.min(255) as u8).collect();
    let body = tsearch::encode(&keys, &importance).unwrap();
    let t = teigeodata_core::table::Table::parse(&body).unwrap();
    let columns = t
        .ids()
        .map(|id| (id, t.column_raw(id).map_or(0, |c| c.1.len())))
        .collect();
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
    Packed {
        ix,
        body_len: body.len(),
        columns,
        best_prefix: std::env::var("S3_PREFIX").map_or(true, |v| v != "order"),
    }
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
    fn bytes(&self) -> usize {
        self.map.as_fst().as_bytes().len()
            + self.postings.iter().map(|p| 4 + 4 * p.len()).sum::<usize>()
    }
}

struct Engine<'a> {
    recs: &'a [Rec],
    exact: Index,
    loose: Index,
    phon: Index,
    /// Folded abbreviation → unit type.
    abbr: BTreeMap<String, String>,
    /// Folded native-script generic → its folded Latin spellings.
    native: Vec<(String, Vec<String>)>,
    /// Each record's folded parent, for context matching.
    districts: Vec<String>,
    droppable: Vec<String>,
    knobs: Knobs,
    pack: Option<Packed>,
}

/// A/B switches, from the environment: `S3_FUZZY2` (the length from which two edits are
/// allowed; 0 = never), `S3_NO_FUZZY`, `S3_NO_PREFIX`, `S3_NO_PHONETIC`, `S3_PHONETIC_BONUS`, `S3_UNANCHORED`, `S3_PREFIX_SCAN`.
#[derive(Clone, Copy)]
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

/// With `S3_ALIASES=<tsv>` (the forge `aliases` output), every alias joins its anchor
/// record's alternate names, so the index answers names that only OSM or Wikidata know.
fn add_aliases(recs: &mut [Rec]) {
    let Some(path) = std::env::var_os("S3_ALIASES") else {
        return;
    };
    let text = std::fs::read_to_string(&path).expect("read the aliases file");
    let mut by_key: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for line in text.lines().skip(1) {
        let mut f = line.split('\t');
        if let (Some(anchor), Some(alias)) = (f.next(), f.next()) {
            by_key.entry(anchor).or_default().push(alias);
        }
    }
    let mut added = 0;
    for r in recs.iter_mut() {
        if let Some(aliases) = by_key.get(r.key.as_str()) {
            for a in aliases {
                if !r.alts.iter().any(|x| x == a) && r.name != *a {
                    r.alts.push((*a).to_string());
                    added += 1;
                }
            }
        }
    }
    println!("aliases: {added} added from {}", path.to_string_lossy());
}

/// The phonetic key under test: `indic-v0`, or `indic-v1` with `S3_PKEY=v1`.
fn pkey(s: &str) -> String {
    static V1: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *V1.get_or_init(|| std::env::var("S3_PKEY").is_ok_and(|v| v == "v1")) {
        pkey_indic_v1(s)
    } else {
        pkey_indic_v0(s)
    }
}

fn despace(s: &str) -> String {
    s.replace(' ', "")
}

impl<'a> Engine<'a> {
    fn new(recs: &'a [Rec], cc_dir: &str) -> Self {
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

        let (mut exact, mut loose, mut phon) = (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
        let add = |m: &mut BTreeMap<String, BTreeSet<u32>>, k: String, i: u32| {
            if !k.is_empty() {
                m.entry(k).or_default().insert(i);
            }
        };
        for (i, r) in recs.iter().enumerate() {
            let i = i as u32;
            for n in std::iter::once(&r.name).chain(&r.alts) {
                let f = fold(n);
                add(&mut loose, despace(&f), i);
                for d in &droppable {
                    if let Some(stem) = f.strip_suffix(d.as_str()) {
                        add(&mut loose, despace(stem), i);
                    }
                }
                add(&mut phon, pkey(n), i);
                add(&mut exact, f, i);
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
            abbr,
            native,
            knobs: Knobs::from_env(),
            districts: recs.iter().map(|r| fold(&r.district)).collect(),
            droppable,
        }
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
    fn prefix_packed(&self, p: &Packed, q: &str, best: &mut BTreeMap<u32, i64>) {
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
    fn search(&self, query: &str, k: usize) -> Vec<usize> {
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
            if self.knobs.phonetic {
                for i in self.get(T_PHON, &pkey(f)) {
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

/// One QWERTY neighbour, fixed, for the probe.
fn qwerty_neighbour(c: char) -> Option<char> {
    let rows = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
    rows.iter().find_map(|r| {
        let i = r.find(c)?;
        r.chars().nth(if i + 1 < r.len() { i + 1 } else { i - 1 })
    })
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    assert!(
        args.len() >= 2,
        "usage: s3-eval <cc> <GeoNames CC.txt> [<COD-AB xlsx>]"
    );
    let cc = &args[0];
    let dir = format!("data/countries/{cc}");
    let prof = profile(&format!("{dir}/profile.yaml"));
    let t0 = Instant::now();
    let mut recs = load(&prof, &args[1], args.get(2).map(String::as_str));
    add_aliases(&mut recs);
    let t_load = t0.elapsed();
    let t0 = Instant::now();
    let engine = Engine::new(&recs, &dir);
    let t_build = t0.elapsed();
    let golden: GoldenFile = serde_saphyr::from_str(
        &std::fs::read_to_string(format!("tests/golden/{cc}.yaml")).unwrap(),
    )
    .unwrap();

    // Key → group, and each group's member keys.
    let key_group: BTreeMap<&str, usize> = recs.iter().map(|r| (r.key.as_str(), r.group)).collect();

    // Per class: n, hits@1, hits@5, pass (within max_rank), mrr@10, ndcg@10.
    let mut per: BTreeMap<String, [f64; 6]> = BTreeMap::new();
    let mut misses: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut lat_ns: Vec<f64> = Vec::new();
    let mut lat_by_class: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for q in &golden.queries {
        // Grade per expected group (the best grade among its keys).
        let mut grade: BTreeMap<usize, u8> = BTreeMap::new();
        for (k, g) in &q.expect {
            if let Some(&gr) = key_group.get(k.as_str()) {
                let e = grade.entry(gr).or_default();
                *e = (*e).max(*g);
            }
        }
        let res = engine.search(&q.query, 10);
        // Latency: median of repeated runs.
        let mut runs = Vec::with_capacity(21);
        for _ in 0..21 {
            let t = Instant::now();
            std::hint::black_box(engine.search(std::hint::black_box(&q.query), 10));
            runs.push(t.elapsed().as_nanos() as f64);
        }
        let med = pct(&mut runs, 0.5);
        lat_ns.push(med);
        lat_by_class.entry(q.class.clone()).or_default().push(med);

        let first = res
            .iter()
            .position(|g| grade.get(g).is_some_and(|&x| x >= 2));
        let dcg: f64 = res
            .iter()
            .enumerate()
            .map(|(i, g)| {
                let rel = f64::from(*grade.get(g).unwrap_or(&0));
                (2f64.powf(rel) - 1.0) / ((i + 2) as f64).log2()
            })
            .sum();
        let mut ideal: Vec<u8> = grade.values().copied().collect();
        ideal.sort_unstable_by(|a, b| b.cmp(a));
        let idcg: f64 = ideal
            .iter()
            .take(10)
            .enumerate()
            .map(|(i, &rel)| (2f64.powf(f64::from(rel)) - 1.0) / ((i + 2) as f64).log2())
            .sum();
        let m = per.entry(q.class.clone()).or_default();
        m[0] += 1.0;
        m[1] += f64::from(u8::from(first == Some(0)));
        m[2] += f64::from(u8::from(first.is_some_and(|r| r < 5)));
        m[3] += f64::from(u8::from(first.is_some_and(|r| r < usize::from(q.max_rank))));
        m[4] += first.map_or(0.0, |r| 1.0 / (r + 1) as f64);
        m[5] += if idcg > 0.0 { dcg / idcg } else { 0.0 };
        if first.is_none_or(|r| r >= usize::from(q.max_rank)) {
            let got: Vec<String> = res
                .iter()
                .take(3)
                .map(|g| {
                    recs.iter()
                        .find(|r| r.group == *g)
                        .map_or(String::new(), |r| format!("{} [{}]", r.name, r.district))
                })
                .collect();
            let want = grade
                .keys()
                .next()
                .and_then(|g| recs.iter().find(|r| r.group == *g))
                .map_or(String::new(), |r| format!("{} [{}]", r.name, r.district));
            misses.entry(q.class.clone()).or_default().push(format!(
                "{} {:?} want {want} got {got:?} (rank {:?})",
                q.id,
                q.query,
                first.map(|r| r + 1)
            ));
        }
    }

    // Probe (not part of the golden set): a first-letter keyboard typo on every exact_latin
    // query, which the golden typo class never makes. Measures what fuzzy anchoring costs.
    let (mut probe_n, mut probe_hit5) = (0u32, 0u32);
    for q in golden.queries.iter().filter(|q| q.class == "exact_latin") {
        let mut cs = q.query.chars();
        let Some(first) = cs.next() else { continue };
        let rest: String = cs.collect();
        let Some(sub) = qwerty_neighbour(first.to_ascii_lowercase()) else {
            continue;
        };
        let typo = format!("{}{rest}", sub.to_ascii_uppercase());
        let want: BTreeSet<usize> = q
            .expect
            .iter()
            .filter(|(_, g)| **g >= 2)
            .filter_map(|(k, _)| key_group.get(k.as_str()).copied())
            .collect();
        probe_n += 1;
        if engine.search(&typo, 5).iter().any(|g| want.contains(g)) {
            probe_hit5 += 1;
        }
    }
    println!(
        "probe first-letter typo: recall@5 {:.3} ({probe_hit5}/{probe_n})",
        f64::from(probe_hit5) / f64::from(probe_n)
    );

    // Probe: one letter replaced in the middle of every exact_native query (one edit in
    // characters, two or three in UTF-8 bytes), never the first letter.
    let (mut native_n, mut native_hit5) = (0u32, 0u32);
    for q in golden.queries.iter().filter(|q| q.class == "exact_native") {
        let mut cs: Vec<char> = q.query.chars().collect();
        if cs.len() < 4 {
            continue;
        }
        let at = cs.len() / 2;
        cs[at] = if cs[at] == 'क' { 'ख' } else { 'क' };
        let typo: String = cs.into_iter().collect();
        let want: BTreeSet<usize> = q
            .expect
            .iter()
            .filter(|(_, g)| **g >= 2)
            .filter_map(|(k, _)| key_group.get(k.as_str()).copied())
            .collect();
        native_n += 1;
        if engine.search(&typo, 5).iter().any(|g| want.contains(g)) {
            native_hit5 += 1;
        }
    }
    println!(
        "probe native one-letter typo: recall@5 {:.3} ({native_hit5}/{native_n})",
        f64::from(native_hit5) / f64::from(native_n)
    );

    // pkey pair sets (names-and-search §5.3): positives are every pair of spellings of one
    // lexicon term, Latin and native; negatives are distinct folded names of different
    // entities in the same parent unit. Plus the bucket-size distribution.
    let lex_path = format!("{dir}/lexicon.yaml");
    let lex = if std::path::Path::new(&lex_path).exists() {
        lexicon(&lex_path).terms
    } else {
        Vec::new()
    };
    let (mut pos_n, mut pos_hit) = (0u32, 0u32);
    let mut pos_miss = Vec::new();
    for t in &lex {
        let forms: Vec<String> = t
            .spellings()
            .cloned()
            .chain(t.native.iter().map(|n| n.text.clone()))
            .collect();
        for (i, a) in forms.iter().enumerate() {
            for b in &forms[i + 1..] {
                pos_n += 1;
                if pkey(a) == pkey(b) {
                    pos_hit += 1;
                } else {
                    pos_miss.push(format!("{a}={} {b}={}", pkey(a), pkey(b)));
                }
            }
        }
    }
    let mut by_parent: BTreeMap<&str, Vec<(String, usize)>> = BTreeMap::new();
    for r in recs
        .iter()
        .filter(|r| r.current && !r.district.is_empty() && r.name.is_ascii())
    {
        by_parent
            .entry(r.district.as_str())
            .or_default()
            .push((fold(&r.name), r.group));
    }
    let (mut neg_n, mut neg_merge) = (0u64, 0u64);
    for v in by_parent.values_mut() {
        v.sort();
        v.dedup_by(|a, b| a.0 == b.0);
        let keys: Vec<String> = v.iter().map(|(n, _)| pkey(n)).collect();
        for i in 0..v.len() {
            for j in i + 1..v.len() {
                if v[i].1 != v[j].1 {
                    neg_n += 1;
                    neg_merge += u64::from(keys[i] == keys[j]);
                }
            }
        }
    }
    let mut buckets: Vec<f64> = engine
        .phon
        .postings
        .iter()
        .map(|p| p.len() as f64)
        .collect();
    println!(
        "pkey: positive pair recall {:.3} ({pos_hit}/{pos_n}); false-merge rate {:.5} ({neg_merge}/{neg_n}); bucket p50 {} p99 {} max {}",
        f64::from(pos_hit) / f64::from(pos_n.max(1)),
        neg_merge as f64 / neg_n.max(1) as f64,
        pct(&mut buckets, 0.5),
        pct(&mut buckets, 0.99),
        buckets.last().copied().unwrap_or(0.0)
    );
    if std::env::var_os("S3_MISSES").is_some() {
        println!("pkey positive misses: {pos_miss:?}");
    }
    println!(
        "records {}, groups {}, load {:.2?}, build {:.2?}",
        recs.len(),
        recs.iter().map(|r| r.group).collect::<BTreeSet<_>>().len(),
        t_load,
        t_build
    );
    if let Some(p) = &engine.pack {
        println!("SEARCH section: {} B; columns {:?}", p.body_len, p.columns);
    }
    println!(
        "index bytes: exact {}, loose {}, phonetic {}, total {}",
        engine.exact.bytes(),
        engine.loose.bytes(),
        engine.phon.bytes(),
        engine.exact.bytes() + engine.loose.bytes() + engine.phon.bytes()
    );
    println!(
        "| class | n | recall@1 | recall@5 | pass@max_rank | MRR@10 | nDCG@10 | p50 µs | p99 µs |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    let mut tot = [0f64; 6];
    for (c, m) in &per {
        let mut l = lat_by_class.get(c).cloned().unwrap_or_default();
        println!(
            "| {c} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.1} | {:.1} |",
            m[0],
            m[1] / m[0],
            m[2] / m[0],
            m[3] / m[0],
            m[4] / m[0],
            m[5] / m[0],
            pct(&mut l, 0.5) / 1e3,
            pct(&mut l, 0.99) / 1e3
        );
        for i in 0..6 {
            tot[i] += m[i];
        }
    }
    println!(
        "| **all** | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.1} | {:.1} |",
        tot[0],
        tot[1] / tot[0],
        tot[2] / tot[0],
        tot[3] / tot[0],
        tot[4] / tot[0],
        tot[5] / tot[0],
        pct(&mut lat_ns.clone(), 0.5) / 1e3,
        pct(&mut lat_ns, 0.99) / 1e3
    );
    if std::env::var_os("S3_MISSES").is_some() {
        for (c, v) in &misses {
            println!("\n## misses: {c} ({})", v.len());
            for m in v {
                println!("  {m}");
            }
        }
    }
}
