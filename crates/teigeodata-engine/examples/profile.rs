//! Runs one query kind in a loop for a fixed time, for a sampling profiler (phase 8.3). Each
//! run is one suite, so every sample belongs to it:
//!
//! ```text
//! CARGO_PROFILE_RELEASE_DEBUG=line-tables-only \
//!   cargo build --release -p teigeodata-engine --example profile --features …
//! xctrace record --template 'Time Profiler' --output q1.trace \
//!   --launch -- target/release/examples/profile <pack.tgd> reverse 10
//! cargo xtask profile-top q1.trace
//! ```
//!
//! Suites: `get` (Q7), `reverse` (Q1, k = 10), `radius` (Q8, 10 km, k = 50), `admin` (Q2),
//! `tz` (Q3), `search` (Q6, k = 10), `open` (L1, mapped open with head checks), `first`
//! (L4, open then the first query of each kind). It prints the calls made and the mean
//! time per call, which the profile's totals are read against. With `<calls>x` in place of
//! the seconds it makes a fixed number of calls, so `/usr/bin/time -l` counts the
//! instructions of two builds on the same work.

#![allow(
    clippy::print_stdout,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    clippy::panic,
    reason = "a profiling driver: prints one line and panics on bad input"
)]

#[path = "../tests/common/mod.rs"]
mod common;

use std::hint::black_box;
use std::time::{Duration, Instant};

use common::probes;
use teigeodata_engine::{Engine, Layer, LayerOptions, Scratch};

fn main() {
    let mut args = std::env::args().skip(1);
    let usage = "usage: profile <pack.tgd> <get|reverse|radius|admin|tz|search|open|first> [seconds | <calls>x]";
    let path = args.next().expect(usage);
    let suite = args.next().expect(usage);
    // `10` runs for ten seconds; `100000x` makes exactly that many calls (rounded up to a
    // batch), for an instruction count that compares two builds (`/usr/bin/time -l`).
    let limit = args.next().unwrap_or_else(|| "10".into());
    let (seconds, max_calls) = match limit.strip_suffix('x') {
        Some(n) => (u64::MAX, n.parse::<usize>().expect(usage)),
        None => (limit.parse::<u64>().expect(usage), usize::MAX),
    };
    let bytes = std::fs::read(&path).unwrap();
    let p = probes(&bytes);
    drop(bytes);

    let mut e = Engine::new();
    e.add(Layer::open(&path, LayerOptions::default()).unwrap());
    let mut s = Scratch::default();
    let (mut near, mut admin, mut hits) = (Vec::new(), Vec::new(), Vec::new());
    // Warm every section once, so a query suite measures warm queries only.
    if suite != "open" && suite != "first" {
        black_box(e.get(p.ids[0]).unwrap());
        e.reverse(p.points[0], 10, &mut s, &mut near).unwrap();
        e.admin_at(p.points7[0], &mut s, &mut admin).unwrap();
        black_box(e.tz_at(p.points7[0]).unwrap());
        e.search(&p.queries[0], 10, &mut s, &mut hits).unwrap();
    }

    let end = Duration::from_secs(seconds.min(86_400));
    let start = Instant::now();
    let mut calls = 0_usize;
    let (ni, nq) = (p.ids.len(), p.queries.len());
    while calls < max_calls && start.elapsed() < end {
        // A batch between clock reads, so reading the clock stays out of the profile.
        for _ in 0..64 {
            let i = calls % ni;
            match suite.as_str() {
                "get" => {
                    black_box(e.get(p.ids[i]).unwrap());
                }
                "reverse" => e.reverse(p.points[i], 10, &mut s, &mut near).unwrap(),
                "radius" => e
                    .within_radius(p.points[i], 10_000.0, 50, &mut s, &mut near)
                    .unwrap(),
                "admin" => {
                    black_box(e.admin_at(p.points7[i], &mut s, &mut admin).unwrap());
                }
                "tz" => {
                    black_box(e.tz_at(p.points7[i]).unwrap());
                }
                "search" => e
                    .search(&p.queries[calls % nq], 10, &mut s, &mut hits)
                    .unwrap(),
                "open" => {
                    black_box(Layer::open(&path, LayerOptions::default()).unwrap());
                }
                "first" => {
                    let mut f = Engine::new();
                    f.add(Layer::open(&path, LayerOptions::default()).unwrap());
                    black_box(f.get(p.ids[i]).unwrap());
                    f.reverse(p.points[i], 10, &mut s, &mut near).unwrap();
                    f.admin_at(p.points7[i], &mut s, &mut admin).unwrap();
                    black_box(f.tz_at(p.points7[i]).unwrap());
                    f.search(&p.queries[calls % nq], 10, &mut s, &mut hits)
                        .unwrap();
                }
                _ => panic!("{usage}"),
            }
            calls += 1;
        }
    }
    let t = start.elapsed();
    println!(
        "{suite}: {calls} calls in {:.2} s, {:.0} ns per call",
        t.as_secs_f64(),
        t.as_nanos() as f64 / calls as f64
    );
}
