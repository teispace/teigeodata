//! Throughput of one [`Shared`] engine at 1, 2, 4 and 8 threads (phase 8.3, the scaling gate).
//! Every thread takes the current snapshot for each query, as a server's handlers do, and runs
//! the same mix: `get`, `reverse` (k = 10), `admin_at`, `tz_at` and `search` (k = 10) in turn,
//! with its own scratch. Prints one Markdown row per thread count: queries per second over all
//! threads, the speed-up over one thread, and the efficiency (speed-up per thread).
//!
//! ```text
//! cargo run --release -p teigeodata-engine --example scaling --features … -- <pack.tgd> [secs] [threads,…]
//! ```
//!
//! `cargo xtask scaling` runs it and holds it to its floor. On a machine shared with other
//! work, and with performance and efficiency cores, the figures are a lower bound.

#![allow(
    clippy::print_stdout,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    clippy::panic,
    reason = "a measuring binary: prints its table and panics on bad input"
)]

#[path = "../tests/common/mod.rs"]
mod common;

use std::hint::black_box;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use common::{Probes, probes};
use teigeodata_engine::{Engine, Layer, LayerOptions, Scratch, Shared};

/// Queries made by one thread until `stop`, starting at probe `offset`.
fn worker(shared: &Shared, p: &Probes, offset: usize, stop: &AtomicBool) -> u64 {
    let mut s = Scratch::default();
    let (mut near, mut admin, mut hits) = (Vec::new(), Vec::new(), Vec::new());
    let (ni, nq) = (p.ids.len(), p.queries.len());
    let mut calls = 0_u64;
    let mut i = offset;
    while !stop.load(Ordering::Relaxed) {
        // Five queries per round, one of each kind, between reads of the stop flag.
        for kind in 0..5 {
            let e = shared.load();
            let k = i % ni;
            match kind {
                0 => {
                    black_box(e.get(p.ids[k]).unwrap());
                }
                1 => e.reverse(p.points[k], 10, &mut s, &mut near).unwrap(),
                2 => {
                    black_box(e.admin_at(p.points7[k], &mut s, &mut admin).unwrap());
                }
                3 => {
                    black_box(e.tz_at(p.points7[k]).unwrap());
                }
                _ => e.search(&p.queries[i % nq], 10, &mut s, &mut hits).unwrap(),
            }
            i = i.wrapping_add(1);
        }
        calls += 5;
    }
    calls
}

/// Queries per second over `threads` threads for `secs`.
fn throughput(shared: &Arc<Shared>, p: &Arc<Probes>, threads: usize, secs: f64) -> f64 {
    let stop = Arc::new(AtomicBool::new(false));
    let start = Arc::new(Barrier::new(threads + 1));
    let handles: Vec<_> = (0..threads)
        .map(|t| {
            let (shared, p, stop, start) = (
                Arc::clone(shared),
                Arc::clone(p),
                Arc::clone(&stop),
                Arc::clone(&start),
            );
            // Threads start at different probes, so they do not read the same rows in step.
            thread::spawn(move || {
                start.wait();
                worker(&shared, &p, t * 997, &stop)
            })
        })
        .collect();
    start.wait();
    let t0 = Instant::now();
    thread::sleep(Duration::from_secs_f64(secs));
    stop.store(true, Ordering::Relaxed);
    let calls: u64 = handles.into_iter().map(|h| h.join().unwrap()).sum();
    calls as f64 / t0.elapsed().as_secs_f64()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let usage = "usage: scaling <pack.tgd> [secs] [threads,…]";
    let path = args.next().expect(usage);
    let secs: f64 = args.next().map_or(3.0, |s| s.parse().expect(usage));
    let counts: Vec<usize> = args.next().map_or_else(
        || vec![1, 2, 4, 8],
        |s| s.split(',').map(|n| n.parse().expect(usage)).collect(),
    );
    let bytes = std::fs::read(&path).unwrap();
    let p = Arc::new(probes(&bytes));
    drop(bytes);
    let mut engine = Engine::new();
    engine.add(Layer::open(&path, LayerOptions::default()).unwrap());
    let shared = Arc::new(Shared::new(engine));
    // Warm: verify every section a query reads, on one thread, before timing.
    let mut s = Scratch::default();
    let (mut near, mut admin, mut hits) = (Vec::new(), Vec::new(), Vec::new());
    let e = shared.load();
    black_box(e.get(p.ids[0]).unwrap());
    e.reverse(p.points[0], 10, &mut s, &mut near).unwrap();
    black_box(e.admin_at(p.points7[0], &mut s, &mut admin).unwrap());
    black_box(e.tz_at(p.points7[0]).unwrap());
    e.search(&p.queries[0], 10, &mut s, &mut hits).unwrap();
    drop(e);

    println!("| Threads | Queries/s | Speed-up | Efficiency |");
    println!("|---:|---:|---:|---:|");
    let mut one = None;
    for &n in &counts {
        // The best of three rounds: a shared machine only ever slows a round down.
        let q = (0..3)
            .map(|_| throughput(&shared, &p, n, secs / 3.0))
            .fold(0.0, f64::max);
        // Per thread at the first count (1 unless asked otherwise).
        let base = *one.get_or_insert(q / n as f64);
        let speedup = q / base;
        println!(
            "| {n} | {q:.0} | {speedup:.2}× | {:.0}% |",
            speedup * 100.0 / n as f64
        );
    }
}
