//! Measures substring search (D-199, TG-06) on a `GeoNames` proxy of a country: the SUBSTRING
//! section's size against SEARCH's, and the whole `search` query's latency and recall with the
//! `substring` component installed and without it, on the same pack.
//!
//! ```text
//! cargo run --release -p teigeodata-engine --example substring -- data/cache/geonames-np/NP.txt
//! ```
//!
//! The proxy is the research note's (§3.2): the P and A rows, each with its name, ASCII name and
//! alternate names, keyed by the real fold, `search_keys` and `substring_keys`; importance is a
//! class base plus 12·log10(population). Place IDs are the `GeoNames` IDs. Queries: fixed ones,
//! then random mid-word substrings of 3 to 8 code points cut from random names (seeded). Each
//! query's time is the best of several runs; timing on a shared machine is an upper bound.

#![allow(
    clippy::print_stdout,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    reason = "a measuring tool: prints its table and panics on bad input"
)]

#[path = "../tests/common/mod.rs"]
mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::hint::black_box;
use std::time::Instant;

use common::{Model, OwnedName, Rng};
use teigeodata_core::{E7, Namespace, PlaceId, PlaceKind, Point, hilbert};
use teigeodata_engine::{Component, Components, Engine, Hit, Layer, LayerOptions, Scratch, Verify};
use teigeodata_format::search::{EXACT, LOOSE};
use teigeodata_format::{Kind, Pack};
use teigeodata_names::{SearchMap, fold, search_keys, substring_keys};

const RUNS: usize = 11;
const QUERIES: usize = 2_000;

struct Row {
    id: u64,
    point: Point<E7>,
    names: Vec<String>,
    importance: u8,
}

fn importance(class: &str, code: &str, population: u64) -> u8 {
    let base = match (class, code) {
        ("P", "PPLC") => 120.0,
        ("P", c) if c.starts_with("PPLA") => 100.0,
        ("P", _) => 40.0,
        (_, "ADM1") => 110.0,
        (_, "ADM2") => 100.0,
        _ => 70.0,
    };
    let pop = if population > 0 {
        12.0 * (population as f64).log10()
    } else {
        0.0
    };
    (base + pop).min(255.0) as u8
}

fn load(path: &str) -> Vec<Row> {
    let text = std::fs::read_to_string(path).expect("read the GeoNames file");
    let mut rows = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 15 || !matches!(f[6], "P" | "A") {
            continue;
        }
        let (Ok(lat), Ok(lon)) = (f[4].parse::<f64>(), f[5].parse::<f64>()) else {
            continue;
        };
        let Some(point) = Point::<E7>::from_units((lat * 1e7) as i32, (lon * 1e7) as i32) else {
            continue;
        };
        let mut names: Vec<String> = vec![f[1].to_string(), f[2].to_string()];
        names.extend(
            f[3].split(',')
                .filter(|n| !n.is_empty())
                .map(ToString::to_string),
        );
        let mut seen = BTreeSet::new();
        names.retain(|n| !n.trim().is_empty() && seen.insert(n.clone()));
        rows.push(Row {
            id: f[0].parse().unwrap(),
            point,
            names,
            importance: importance(f[6], f[7], f[14].parse().unwrap_or(0)),
        });
    }
    rows.sort_by_key(|r| (hilbert(r.point.to_e6()), r.id));
    rows
}

fn model(rows: &[Row]) -> Model {
    let ns = Namespace::from_country(*b"np").unwrap();
    let n = rows.len();
    let mut keys: BTreeMap<Vec<u8>, BTreeSet<u32>> = BTreeMap::new();
    for (row, r) in rows.iter().enumerate() {
        for name in &r.names {
            search_keys(name, &[], None, |m, k| {
                let tag = match m {
                    SearchMap::Exact => EXACT,
                    SearchMap::Loose => LOOSE,
                    SearchMap::Phonetic => return,
                };
                if !k.is_empty() && k.len() < 255 {
                    let mut key = vec![tag];
                    key.extend_from_slice(k.as_bytes());
                    keys.entry(key).or_default().insert(row as u32);
                }
            });
        }
    }
    let exact: BTreeMap<Vec<u8>, BTreeSet<u32>> = keys
        .iter()
        .filter(|(k, _)| k[0] == EXACT)
        .map(|(k, v)| (k[1..].to_vec(), v.clone()))
        .collect();
    keys.retain(|k, set| {
        if k[0] == LOOSE
            && let Some(twin) = exact.get(&k[1..])
        {
            set.retain(|r| !twin.contains(r));
        }
        !set.is_empty()
    });
    let mut m = Model {
        address: "np".into(),
        component: "places".into(),
        release: "2026.11.0".into(),
        bbox: None,
        ids: rows
            .iter()
            .map(|r| PlaceId::new(ns, r.id).unwrap())
            .collect(),
        points: rows.iter().map(|r| r.point.to_e6()).collect(),
        kinds: vec![PlaceKind::Settlement; n],
        levels: vec![0; n],
        classes: vec![0; n],
        flags: vec![0; n],
        elevations: vec![None; n],
        names: rows
            .iter()
            .map(|r| {
                r.names
                    .iter()
                    .map(|t| OwnedName {
                        text: t.clone(),
                        lang: None,
                        script: None,
                        kind: None,
                        valid: (None, None),
                        rule: 0,
                    })
                    .collect()
            })
            .collect(),
        keys,
        importance: rows.iter().map(|r| r.importance).collect(),
        rules: None,
        units: Vec::new(),
        node: 16,
        redirects: Vec::new(),
        zones: None,
        admin: None,
        arcs: None,
        parents: Vec::new(),
        iso: Vec::new(),
        raw: Vec::new(),
    };
    m.with_substring();
    m
}

/// The fixed queries, then mid-word substrings of 3 to 8 code points of random names, with the
/// row each was cut from (0: none expected).
fn queries(rows: &[Row]) -> Vec<(String, u64)> {
    let fixed = [
        ("mandu", 1_283_240),
        ("athmand", 1_283_240),
        ("काठ", 1_283_240),
        ("माडौं", 1_283_240),
        ("khara", 1_282_898),
        ("खरा", 1_282_898),
        ("ratnagar", 1_283_582),
        ("राटनगर", 1_283_582),
        ("Pokhara", 1_282_898),
        ("pur", 0),
        ("ara", 0),
    ];
    let mut out: Vec<(String, u64)> = fixed.iter().map(|&(q, id)| (q.into(), id)).collect();
    let mut r = Rng(0x7e06_5eed);
    while out.len() < QUERIES {
        let row = &rows[r.below(rows.len() as u64) as usize];
        let name = fold(&row.names[r.below(row.names.len() as u64) as usize]);
        // Where a mid-word query may start: the substring keys, cut as the forge cuts them.
        let mut starts = Vec::new();
        substring_keys(&name, |k| starts.push(k));
        if starts.is_empty() {
            continue;
        }
        let key: Vec<char> = starts[r.below(starts.len() as u64) as usize]
            .chars()
            .collect();
        let len = (3 + r.below(6) as usize).min(key.len());
        out.push((key[..len].iter().collect(), row.id));
    }
    out
}

fn engine(bytes: &[u8], substring: bool) -> Engine {
    let all = Components::all();
    let installed = if substring {
        all
    } else {
        all.without(Components::of(&[Component::Substring]))
    };
    let mut e = Engine::new();
    e.add(
        Layer::from_bytes(
            bytes.to_vec(),
            LayerOptions {
                installed: Some(installed),
                verify: Verify::Eager,
                ..LayerOptions::default()
            },
        )
        .unwrap(),
    );
    e
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i]
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: substring <GeoNames country file>");
    let rows = load(&path);
    let names: usize = rows.iter().map(|r| r.names.len()).sum();
    let bytes = model(&rows).encode();
    let pack = Pack::open(&bytes).unwrap();
    let size = |k| pack.section(k).map_or(0, |s| s.bytes().len());
    let (search, substring) = (size(Kind::SEARCH), size(Kind::SUBSTRING));
    let n = rows.len() as f64;
    println!(
        "{} rows, {names} names; SEARCH {search} B ({:.1} B per place); SUBSTRING {substring} B ({:.1} B per place, ×{:.2} SEARCH)",
        rows.len(),
        search as f64 / n,
        substring as f64 / n,
        substring as f64 / search as f64
    );
    let qs = queries(&rows);
    let ns = Namespace::from_country(*b"np").unwrap();
    for substring in [false, true] {
        let e = engine(&bytes, substring);
        let mut s = Scratch::default();
        let mut out: Vec<Hit> = Vec::with_capacity(16);
        let mut times = Vec::with_capacity(qs.len());
        let (mut top1, mut top10) = (0, 0);
        for (q, id) in &qs {
            e.search(q, 10, &mut s, &mut out).unwrap();
            let best = (0..RUNS)
                .map(|_| {
                    let t = Instant::now();
                    e.search(black_box(q), 10, &mut s, &mut out).unwrap();
                    t.elapsed().as_nanos() as f64 / 1e3
                })
                .fold(f64::INFINITY, f64::min);
            times.push(best);
            let want = PlaceId::new(ns, (*id).max(1)).unwrap();
            if let Some(rank) = out.iter().position(|h| h.place.id == want) {
                top1 += usize::from(rank == 0);
                top10 += 1;
            }
        }
        let mut sorted = times.clone();
        sorted.sort_by(f64::total_cmp);
        println!(
            "substring {}: p50 {:.1} µs, p99 {:.1} µs, max {:.1} µs; the source row first {top1}, in the top 10 {top10} of {}",
            if substring { "on " } else { "off" },
            percentile(&sorted, 0.5),
            percentile(&sorted, 0.99),
            sorted.last().copied().unwrap_or(0.0),
            qs.len()
        );
        let mut long: Vec<f64> = times
            .iter()
            .zip(&qs)
            .filter(|(_, (q, _))| q.chars().count() > 3)
            .map(|(&t, _)| t)
            .collect();
        long.sort_by(f64::total_cmp);
        println!(
            "  queries of 4 or more code points ({}): p50 {:.1} µs, p99 {:.1} µs",
            long.len(),
            percentile(&long, 0.5),
            percentile(&long, 0.99)
        );
        let mut slow: Vec<(f64, &str)> = times
            .iter()
            .zip(&qs)
            .map(|(&t, (q, _))| (t, q.as_str()))
            .collect();
        slow.sort_by(|a, b| b.0.total_cmp(&a.0));
        let slow: Vec<String> = slow
            .iter()
            .take(6)
            .map(|(t, q)| format!("{q} {t:.0} µs"))
            .collect();
        println!("  slowest: {}", slow.join(", "));
        for (q, id) in qs.iter().take(9) {
            e.search(q, 10, &mut s, &mut out).unwrap();
            let want = PlaceId::new(ns, *id).unwrap();
            let rank = out.iter().position(|h| h.place.id == want).map(|r| r + 1);
            println!("  {q}: rank {rank:?}");
        }
    }
}
