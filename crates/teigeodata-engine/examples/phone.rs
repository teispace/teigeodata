//! TG-17: the phone-class bench and its gate ([note](../../../docs/02-research/notes/phone-class-bench.md)).
//! Measures a layer as an app uses it — open, the first query of each kind (which verifies the
//! sections it reads), search one keystroke at a time, and the point queries — scales each time
//! to a low-end phone by a stated factor, and fails when an estimate is over its target. The
//! heap is counted by a counting allocator: live after open and the first queries, and peak.
//!
//! ```text
//! cargo run --release -p teigeodata-engine --example phone -- data/build/embed/tithipatra-lite.tgd [--factor 6.5]
//! ```
//!
//! Each time is the minimum of several rounds, so load on a shared machine inflates it only
//! when every round is slowed; an estimate over target on a loaded machine is rerun before it
//! is believed. The page cache is warm (a relaunch); a cold first read from flash is not
//! measured here.

#![allow(
    clippy::print_stdout,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::cast_sign_loss,
    clippy::panic,
    reason = "a measuring tool: prints its table, panics on bad input"
)]

#[path = "../tests/common/mod.rs"]
mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use common::{Model, Rng};
use teigeodata_core::{E6, E7, Point};
use teigeodata_engine::{Engine, Layer, LayerOptions, Scratch};
use teigeodata_format::Kind;

/// Live heap bytes and their peak.
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Counting;

#[allow(
    unsafe_code,
    reason = "a counting global allocator for the heap figures"
)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
        PEAK.fetch_max(live, Ordering::Relaxed);
        // SAFETY: the caller's contract is the system allocator's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: as above.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Single-core speed of this machine's fast core over a low-end phone's: Geekbench 6
/// single-core, Apple M2 Pro about 2,689 against `MediaTek Helio G85` (`Galaxy A05` class) about
/// 424, 6.3×, rounded up (note §2).
const FACTOR: f64 = 6.5;

/// Phone targets, each a time on the low-end phone (note §3).
const TARGETS: [(&str, Duration); 7] = [
    ("open", Duration::from_millis(50)),
    ("first queries", Duration::from_millis(150)),
    ("keystroke p99", Duration::from_millis(16)),
    ("keystroke max", Duration::from_millis(50)),
    ("point query p99", Duration::from_millis(2)),
    ("get p99", Duration::from_micros(500)),
    // Once, in the background, after the first launch (D-218).
    ("derive SUBSTRING", Duration::from_secs(3)),
];

/// The heap targets, bytes: after open and the first queries, and at its peak.
const HEAP_LIVE: usize = 4 << 20;
const HEAP_PEAK: usize = 16 << 20;
/// The heap target while deriving SUBSTRING once in the background (54 MiB measured for the
/// Tithipatra layer, 2026-11-12).
const HEAP_DERIVE: usize = 64 << 20;

fn min_of(rounds: usize, mut f: impl FnMut()) -> Duration {
    (0..rounds)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed()
        })
        .min()
        .unwrap_or_default()
}

/// The value at quantile `q` of `v` (sorted in place).
fn quantile(v: &mut [Duration], q: f64) -> Duration {
    v.sort_unstable();
    let i = ((v.len() as f64 - 1.0) * q).round() as usize;
    v.get(i).copied().unwrap_or_default()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: phone <layer.tgd> [--factor <f>]");
    let mut factor = FACTOR;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--factor" => {
                factor = args
                    .next()
                    .and_then(|f| f.parse().ok())
                    .expect("--factor <f>");
            }
            other => panic!("unknown argument {other}"),
        }
    }
    let bytes = std::fs::read(&path).unwrap();
    let model = Model::from_pack(&bytes);
    // The inputs a device derives SUBSTRING from: NAMES and SEARCH, as stored.
    let pack = teigeodata_format::Pack::open(&bytes).unwrap();
    let inputs: Vec<(Kind, u64, Vec<u8>)> = [Kind::NAMES, Kind::SEARCH]
        .into_iter()
        .map(|k| {
            let s = pack.section(k).expect("the layer has NAMES and SEARCH");
            (k, s.item_count(), s.bytes().to_vec())
        })
        .collect();
    let shipped = pack.section(Kind::SUBSTRING).map(|s| s.bytes().to_vec());
    drop(bytes);
    let mut r = Rng(17);
    let n = 2_000;
    let ids: Vec<_> = (0..n)
        .map(|_| model.ids[r.below(model.ids.len() as u64) as usize])
        .collect();
    let points: Vec<Point<E6>> = (0..n)
        .map(|_| {
            let p = model.points[r.below(model.points.len() as u64) as usize];
            Point::from_units(
                p.lat() + r.below(20_000) as i32 - 10_000,
                p.lon() + r.below(20_000) as i32 - 10_000,
            )
            .unwrap()
        })
        .collect();
    let points7: Vec<Point<E7>> = points.iter().map(|p| p.to_e7()).collect();
    // Queries as typed: each name of a random place, every prefix one character longer.
    let names: Vec<String> = (0..300)
        .filter_map(|_| {
            let row = r.below(model.names.len() as u64) as usize;
            let ns = &model.names[row];
            ns.get(r.below(ns.len().max(1) as u64) as usize)
                .map(|x| x.text.clone())
        })
        .collect();
    drop(model);
    let mut rows: Vec<(String, Duration)> = Vec::new();

    // Open: mapped, head checks only (block proofs wait for first use, D-142).
    let open = min_of(30, || {
        black_box(Layer::open(&path, LayerOptions::default()).unwrap());
    });
    rows.push(("open".into(), open));

    // The first query of each kind on a fresh layer, together: what the first screen costs.
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let (mut near, mut admin, mut hits) = (Vec::new(), Vec::new(), Vec::new());
    let mut live_after = 0;
    // Per kind, the minimum over rounds; the total is the minimum of the rounds' totals.
    let kinds = [
        "get",
        "reverse",
        "admin_at",
        "tz_at",
        "elevation_at",
        "search",
    ];
    let mut each = [Duration::MAX; 6];
    let mut first = Duration::MAX;
    for _ in 0..5 {
        let mut e = Engine::new();
        e.add(Layer::open(&path, LayerOptions::default()).unwrap());
        let mut s = Scratch::default();
        let mut t = [Duration::ZERO; 6];
        let mut time = |i: usize, f: &mut dyn FnMut()| {
            let start = Instant::now();
            f();
            t[i] = start.elapsed();
        };
        time(0, &mut || {
            black_box(e.get(ids[0]).unwrap());
        });
        time(1, &mut || {
            e.reverse(points[0], 10, &mut s, &mut near).unwrap();
        });
        time(2, &mut || {
            black_box(e.admin_at(points7[0], &mut s, &mut admin).unwrap());
        });
        time(3, &mut || {
            black_box(e.tz_at(points7[0]).unwrap());
        });
        time(4, &mut || {
            black_box(e.elevation_at(points7[0], &mut s).unwrap());
        });
        time(5, &mut || {
            e.search(&names[0], 10, &mut s, &mut hits).unwrap();
        });
        live_after = LIVE.load(Ordering::Relaxed).saturating_sub(base);
        for (m, v) in each.iter_mut().zip(t) {
            *m = (*m).min(v);
        }
        first = first.min(t.iter().sum());
        black_box(&e);
    }
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(base);
    rows.push(("first queries".into(), first));
    for (k, d) in kinds.iter().zip(each) {
        rows.push((format!("  first {k}"), d));
    }

    // Warm: one engine, as the app keeps it.
    let mut e = Engine::new();
    e.add(Layer::open(&path, LayerOptions::default()).unwrap());
    let mut s = Scratch::default();
    let mut keys = Vec::new();
    for q in &names {
        let mut ends: Vec<usize> = q.char_indices().map(|(i, _)| i).skip(1).collect();
        ends.push(q.len());
        for &end in &ends {
            let prefix = &q[..end];
            keys.push(min_of(3, || {
                e.search(prefix, 10, &mut s, &mut hits).unwrap();
            }));
        }
    }
    let typed = keys.len();
    rows.push(("keystroke p50".into(), quantile(&mut keys, 0.5)));
    rows.push(("keystroke p99".into(), quantile(&mut keys, 0.99)));
    rows.push(("keystroke max".into(), quantile(&mut keys, 1.0)));
    let mut pts: Vec<Duration> = (0..n)
        .map(|i| {
            min_of(3, || {
                e.reverse(points[i], 10, &mut s, &mut near).unwrap();
                e.admin_at(points7[i], &mut s, &mut admin).unwrap();
                black_box(e.tz_at(points7[i]).unwrap());
                black_box(e.elevation_at(points7[i], &mut s).unwrap());
            })
        })
        .collect();
    rows.push(("point query p99".into(), quantile(&mut pts, 0.99)));
    let mut gets: Vec<Duration> = ids
        .iter()
        .map(|&id| {
            min_of(3, || {
                black_box(e.get(id).unwrap());
            })
        })
        .collect();
    rows.push(("get p99".into(), quantile(&mut gets, 0.99)));
    let derive_peak_base = LIVE.load(Ordering::Relaxed);
    PEAK.store(derive_peak_base, Ordering::Relaxed);
    let derived = min_of(3, || {
        black_box(teigeodata_engine::derive::substring(&inputs).unwrap());
    });
    let derive_peak = PEAK
        .load(Ordering::Relaxed)
        .saturating_sub(derive_peak_base);
    // A layer that ships SUBSTRING proves the derived bytes equal the forge's.
    if let Some(shipped) = &shipped {
        let ours = teigeodata_engine::derive::substring(&inputs).unwrap();
        assert!(
            ours == *shipped,
            "the derived SUBSTRING differs from the layer's ({} against {} bytes)",
            ours.len(),
            shipped.len()
        );
        println!("derived SUBSTRING equals the layer's, {} bytes", ours.len());
    }
    rows.push(("derive SUBSTRING".into(), derived));

    println!(
        "phone-class bench: {path}, factor {factor} (this machine's core over a low-end phone's); {typed} keystrokes over {} names, {n} points",
        names.len()
    );
    println!("point query = reverse (10) + admin_at + tz_at + elevation_at\n");
    println!("| Measure | Here | Phone estimate | Target |");
    println!("|---|--:|--:|--:|");
    let mut failed = Vec::new();
    for (label, here) in &rows {
        let phone = here.mul_f64(factor);
        let target = TARGETS.iter().find(|t| t.0 == label).map(|t| t.1);
        let mark = match target {
            Some(t) if phone > t => {
                failed.push(format!("{label}: {phone:.2?} on a phone, over {t:?}"));
                " ✗"
            }
            _ => "",
        };
        println!(
            "| {label} | {here:.2?} | {phone:.2?}{mark} | {} |",
            target.map_or("–".into(), |t| format!("{t:?}"))
        );
    }
    let mib = |b: usize| b as f64 / f64::from(1 << 20);
    println!(
        "| heap after open and the first queries | {:.2} MiB | | {:.0} MiB |",
        mib(live_after),
        mib(HEAP_LIVE)
    );
    println!(
        "| heap peak | {:.2} MiB | | {:.0} MiB |",
        mib(peak),
        mib(HEAP_PEAK)
    );
    println!(
        "| heap peak while deriving SUBSTRING | {:.2} MiB | | {:.0} MiB |",
        mib(derive_peak),
        mib(HEAP_DERIVE)
    );
    if derive_peak > HEAP_DERIVE {
        failed.push(format!("heap while deriving {:.2} MiB", mib(derive_peak)));
    }
    if live_after > HEAP_LIVE {
        failed.push(format!("heap after open {:.2} MiB", mib(live_after)));
    }
    if peak > HEAP_PEAK {
        failed.push(format!("heap peak {:.2} MiB", mib(peak)));
    }
    if failed.is_empty() {
        println!("\nG-phone: ok");
    } else {
        println!("\nG-phone failed:\n  {}", failed.join("\n  "));
        std::process::exit(1);
    }
}
