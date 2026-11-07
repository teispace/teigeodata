//! B7 automaton A/B: one-edit fuzzy matching of every golden query against the folded Nepal
//! names, anchored on the first character, as S3 does.
//!
//! - `fst+dp`: S3's byte-level OSA automaton walking an `fst` map (the S3 baseline).
//! - `sorted+dp`: a character-level OSA dynamic program walking sorted keys, reusing the state
//!   of the shared prefix with the previous key and skipping keys under a dead prefix.
//! - `sorted+nfa`: the same walk with the bit-parallel NFA (`teigeodata_format::search::fuzzy`).
//!
//! Every variant's matches are compared: the character-level ones must agree exactly; the byte
//! level differs on non-Latin keys by design.
//!
//! `cargo run --release -p spike-s3-search --bin fuzzy-ab -- np <NP.txt> <COD-AB xlsx>`
#![allow(
    clippy::print_stdout,
    clippy::cast_sign_loss,
    clippy::single_match_else,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    missing_docs,
    reason = "a measurement spike over pinned local inputs"
)]

use std::collections::BTreeSet;
use std::hint::black_box;
use std::time::Instant;

use fst::automaton::Str;
use fst::{Automaton, IntoStreamer, Set, Streamer};
use serde::Deserialize;
use spike_s3_search::{fold, load, profile};
use teigeodata_format::search::fst::{Builder, Fst};
use teigeodata_format::search::fuzzy::{Fuzzy, State};

#[derive(Deserialize)]
struct GoldenFile {
    queries: Vec<GoldenQuery>,
}

#[derive(Deserialize)]
struct GoldenQuery {
    query: String,
}

const MAX: usize = 63;

/// S3's automaton, on bytes.
struct ByteDp<'a> {
    q: &'a [u8],
}

#[derive(Clone, Copy)]
struct BState {
    prev: [u8; MAX + 1],
    cur: [u8; MAX + 1],
    last: u8,
    has_prev: bool,
}

impl Automaton for ByteDp<'_> {
    type State = Option<BState>;
    fn start(&self) -> Self::State {
        let mut cur = [0_u8; MAX + 1];
        for (j, c) in cur.iter_mut().enumerate().take(self.q.len() + 1) {
            *c = (j as u8).min(2);
        }
        Some(BState {
            prev: [0; MAX + 1],
            cur,
            last: 0,
            has_prev: false,
        })
    }
    fn is_match(&self, s: &Self::State) -> bool {
        s.as_ref().is_some_and(|s| s.cur[self.q.len()] <= 1)
    }
    fn can_match(&self, s: &Self::State) -> bool {
        s.is_some()
    }
    fn accept(&self, s: &Self::State, b: u8) -> Self::State {
        let s = s.as_ref()?;
        let q = self.q;
        let mut next = [0_u8; MAX + 1];
        next[0] = (s.cur[0] + 1).min(2);
        let mut alive = next[0] <= 1;
        for j in 1..=q.len() {
            let cost = u8::from(q[j - 1] != b);
            let mut v = (s.cur[j] + 1).min(next[j - 1] + 1).min(s.cur[j - 1] + cost);
            if j > 1 && s.has_prev && b == q[j - 2] && s.last == q[j - 1] {
                v = v.min(s.prev[j - 2] + 1);
            }
            next[j] = v.min(2);
            alive |= next[j] <= 1;
        }
        alive.then_some(BState {
            prev: s.cur,
            cur: next,
            last: b,
            has_prev: true,
        })
    }
}

/// A character-level OSA row automaton, one edit.
struct CharDp {
    q: Vec<char>,
}

#[derive(Clone, Copy)]
struct CState {
    prev: [u8; MAX + 1],
    cur: [u8; MAX + 1],
    last: char,
    has_prev: bool,
}

impl CharDp {
    fn start(&self) -> CState {
        let mut cur = [0_u8; MAX + 1];
        for (j, c) in cur.iter_mut().enumerate().take(self.q.len() + 1) {
            *c = (j as u8).min(2);
        }
        CState {
            prev: [0; MAX + 1],
            cur,
            last: '\0',
            has_prev: false,
        }
    }
    fn step(&self, s: &CState, c: char) -> Option<CState> {
        let q = &self.q;
        let mut next = [0_u8; MAX + 1];
        next[0] = (s.cur[0] + 1).min(2);
        let mut alive = next[0] <= 1;
        for j in 1..=q.len() {
            let cost = u8::from(q[j - 1] != c);
            let mut v = (s.cur[j] + 1).min(next[j - 1] + 1).min(s.cur[j - 1] + cost);
            if j > 1 && s.has_prev && c == q[j - 2] && s.last == q[j - 1] {
                v = v.min(s.prev[j - 2] + 1);
            }
            next[j] = v.min(2);
            alive |= next[j] <= 1;
        }
        alive.then_some(CState {
            prev: s.cur,
            cur: next,
            last: c,
            has_prev: true,
        })
    }
    fn is_match(&self, s: &CState) -> bool {
        s.cur[self.q.len()] <= 1
    }
}

/// A walk over sorted keys: `step` advances a state by one character (`None` when dead).
/// States are kept per character depth; a key reuses the states of the prefix it shares with
/// the previous key, and keys under a dead prefix are skipped without stepping.
fn walk<S: Copy>(
    keys: &[(Vec<char>, u32)],
    lcp: &[u32],
    range: (usize, usize),
    start: S,
    step: impl Fn(&S, char) -> Option<S>,
    is_match: impl Fn(&S) -> bool,
    out: &mut Vec<u32>,
) -> u64 {
    let mut stack: Vec<S> = Vec::with_capacity(MAX + 2);
    stack.push(start);
    // Depth at which the previous key died, or u32::MAX if it was read to its end.
    let mut dead_at = u32::MAX;
    let mut steps = 0;
    for i in range.0..range.1 {
        let (key, id) = (&keys[i].0, keys[i].1);
        let shared = if i == range.0 { 0 } else { lcp[i] };
        if shared > dead_at {
            // Same dead prefix as the previous key.
            continue;
        }
        stack.truncate(shared as usize + 1);
        dead_at = u32::MAX;
        let mut ok = true;
        for (d, &c) in key.iter().enumerate().skip(shared as usize) {
            steps += 1;
            match step(&stack[d], c) {
                Some(n) => stack.push(n),
                None => {
                    dead_at = d as u32;
                    ok = false;
                    break;
                }
            }
        }
        if ok && is_match(stack.last().unwrap()) {
            out.push(id);
        }
    }
    steps
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cc = &args[0];
    let prof = profile(&format!("data/countries/{cc}/profile.yaml"));
    let recs = load(&prof, &args[1], args.get(2).map(String::as_str));
    let mut set = BTreeSet::new();
    for r in &recs {
        for n in std::iter::once(&r.name).chain(&r.alts) {
            let f = fold(n);
            if !f.is_empty() {
                set.insert(f);
            }
        }
    }
    let strs: Vec<String> = set.into_iter().collect();
    let fst_set = Set::from_iter(strs.iter()).unwrap();
    let fst_map = fst::Map::from_iter(strs.iter().enumerate().map(|(i, k)| (k, i as u64))).unwrap();
    println!(
        "fst Map with ordinals {} B",
        fst_map.as_fst().as_bytes().len()
    );
    let mut b = Builder::new();
    for k in &strs {
        b.insert(k.as_bytes()).unwrap();
    }
    let (own_bytes, own_n) = b.finish().unwrap();
    let own = Fst::new(&own_bytes, own_n).unwrap();
    own.check(|_, _| {}).unwrap();
    println!("own fst {} B", own_bytes.len());
    let keys: Vec<(Vec<char>, u32)> = strs
        .iter()
        .enumerate()
        .map(|(i, s)| (s.chars().collect(), i as u32))
        .collect();
    let lcp: Vec<u32> = (0..keys.len())
        .map(|i| {
            if i == 0 {
                0
            } else {
                keys[i - 1]
                    .0
                    .iter()
                    .zip(&keys[i].0)
                    .take_while(|(a, b)| a == b)
                    .count() as u32
            }
        })
        .collect();
    let golden: GoldenFile = serde_saphyr::from_str(
        &std::fs::read_to_string(format!("tests/golden/{cc}.yaml")).unwrap(),
    )
    .unwrap();
    let queries: Vec<String> = golden
        .queries
        .iter()
        .map(|q| fold(q.query.split(',').next().unwrap_or("")))
        .filter(|q| (4..=31).contains(&q.len()))
        .collect();
    println!(
        "keys {}, chars {}, fst {} B, queries {}",
        keys.len(),
        keys.iter().map(|k| k.0.len()).sum::<usize>(),
        fst_set.as_fst().as_bytes().len(),
        queries.len()
    );

    // The range of keys starting with the query's first character.
    let anchor = |q: &str| -> (usize, usize) {
        let first = q.chars().next().unwrap();
        let lo = strs.partition_point(|s| s.chars().next() < Some(first));
        let hi = strs.partition_point(|s| s.chars().next() <= Some(first));
        (lo, hi)
    };

    let mut disagree = 0;
    let mut own_diff = 0;
    let mut tried = 0_usize;
    let mut scanned = 0_u64;
    let mut byte_diff = 0;
    let mut steps = (0_u64, 0_u64);
    let mut lat: [Vec<f64>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for q in &queries {
        let range = anchor(q);
        let first = &q[..q.chars().next().map_or(0, char::len_utf8)];
        let f = Fuzzy::new(q, 1).unwrap();
        let dp = CharDp {
            q: q.chars().collect(),
        };
        let bdp = ByteDp { q: q.as_bytes() };
        let run_fst = |out: &mut Vec<u32>| {
            let aut = ByteDp { q: q.as_bytes() }.intersection(Str::new(first).starts_with());
            let mut s = fst_set.search(aut).into_stream();
            while let Some(k) = s.next() {
                out.push(strs.binary_search_by(|x| x.as_bytes().cmp(k)).unwrap() as u32);
            }
        };
        let run_dp = |out: &mut Vec<u32>| {
            walk(
                &keys,
                &lcp,
                range,
                dp.start(),
                |s, c| dp.step(s, c),
                |s| dp.is_match(s),
                out,
            )
        };
        let run_nfa = |out: &mut Vec<u32>| {
            walk(
                &keys,
                &lcp,
                range,
                f.start(),
                |s: &State, c| {
                    let n = f.step(s, c);
                    f.alive(&n).then_some(n)
                },
                |s| f.distance(s).is_some(),
                out,
            )
        };
        let anchor_c = q.chars().next();
        let run_own = |out: &mut Vec<u32>| {
            own.walk(&[], &f, anchor_c, usize::MAX, |o, _| {
                out.push(o);
                std::ops::ControlFlow::Continue(())
            });
        };
        let (mut a, mut b, mut c) = (Vec::new(), Vec::new(), Vec::new());
        // Transitions tried: the smallest budget that completes the walk, by doubling.
        let mut lo = 1;
        while own.walk(&[], &f, anchor_c, lo, |_, _| {
            std::ops::ControlFlow::Continue(())
        }) == teigeodata_format::search::fst::Walked::Budget
        {
            lo *= 2;
        }
        tried += lo / 2;
        let mut d = Vec::new();
        run_own(&mut d);
        if d != c_check(&keys, &lcp, range, &f) {
            own_diff += 1;
        }
        run_fst(&mut a);
        steps.0 += run_dp(&mut b);
        steps.1 += run_nfa(&mut c);
        if b != c {
            disagree += 1;
        }
        scanned += (range.1 - range.0) as u64;
        if a != b {
            byte_diff += 1;
        }
        let _ = &bdp;
        let time = |run: &dyn Fn(&mut Vec<u32>)| {
            let mut out = Vec::with_capacity(64);
            median(
                (0..15)
                    .map(|_| {
                        out.clear();
                        let t = Instant::now();
                        run(&mut out);
                        black_box(&out);
                        t.elapsed().as_nanos() as f64
                    })
                    .collect(),
            )
        };
        lat[0].push(time(&|o| run_fst(o)));
        lat[1].push(time(&|o| {
            run_dp(o);
        }));
        lat[2].push(time(&|o| {
            run_nfa(o);
        }));
        lat[3].push(time(&|o| run_own(o)));
    }
    println!(
        "character-level disagreements (dp vs nfa): {disagree}; queries whose byte-level matches differ: {byte_diff}; steps dp {} nfa {}",
        steps.0, steps.1
    );
    println!(
        "own FST walk disagreements with the sorted NFA walk: {own_diff}; transitions tried per query about {}",
        tried / queries.len()
    );
    println!(
        "keys in the anchor ranges: {scanned} ({} per query)",
        scanned / queries.len() as u64
    );
    println!("| Variant | p50 µs | p99 µs | mean µs |");
    println!("|---|---:|---:|---:|");
    for (name, v) in [
        "fst + byte DP (S3)",
        "sorted + char DP",
        "sorted + char NFA",
        "own FST + char NFA",
    ]
    .iter()
    .zip(lat.iter_mut())
    {
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        println!(
            "| {name} | {:.1} | {:.1} | {:.1} |",
            pct(v, 0.5) / 1e3,
            pct(v, 0.99) / 1e3,
            mean / 1e3
        );
    }
}

fn c_check(keys: &[(Vec<char>, u32)], lcp: &[u32], range: (usize, usize), f: &Fuzzy) -> Vec<u32> {
    let mut out = Vec::new();
    walk(
        keys,
        lcp,
        range,
        f.start(),
        |s: &State, c| {
            let n = f.step(s, c);
            f.alive(&n).then_some(n)
        },
        |s| f.distance(s).is_some(),
        &mut out,
    );
    out
}
