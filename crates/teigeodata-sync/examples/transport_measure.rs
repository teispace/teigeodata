//! Measures the transport asset of a real pack: size per zstd level against whole-file zstd,
//! frames, write time (with the self-check), decode speed, and the bytes to fetch one section.
//!
//! ```text
//! cargo run --release -p teigeodata-sync --example transport_measure -- <pack.tgd> [levels...] [--out=<asset>]
//! ```
//!
//! The pack is read in place. With `--out`, the asset of the last level is written there, for
//! a check with the `zstd` CLI (`zstd -d -c <asset> | cmp - <pack.tgd>`).
#![allow(
    clippy::print_stdout,
    clippy::unwrap_used,
    clippy::cast_precision_loss,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::manual_assert_eq,
    reason = "a measuring tool: prints its table, fails loudly (without printing 11 MB), sizes fit"
)]

use std::time::{Duration, Instant};

use teigeodata_format::Pack;
use teigeodata_sync::transport::{Decoder, Layout, SeekTable, encode};

fn best<T>(runs: usize, mut f: impl FnMut() -> T) -> (Duration, T) {
    let mut out = None;
    let mut best = Duration::MAX;
    for _ in 0..runs {
        let t = Instant::now();
        let v = f();
        best = best.min(t.elapsed());
        out = Some(v);
    }
    (best, out.unwrap())
}

fn mb_s(bytes: usize, d: Duration) -> f64 {
    bytes as f64 / d.as_secs_f64() / 1e6
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: transport_measure <pack.tgd> [levels...]");
    let mut out_path = None;
    let mut levels = Vec::new();
    for a in args {
        match a.strip_prefix("--out=") {
            Some(p) => out_path = Some(p.to_string()),
            None => levels.push(a.parse::<i32>().unwrap()),
        }
    }
    if levels.is_empty() {
        levels = vec![3, 9, 19];
    }
    let pack = std::fs::read(&path).unwrap();
    let p = Pack::open_verified(&pack).unwrap();
    println!(
        "pack: {path}, {} B, {} sections",
        pack.len(),
        p.sections().len()
    );
    println!();
    println!(
        "| level | whole-file zstd B | asset B | vs whole | frames | seek table B | write s | ruzstd decode MB/s | C zstd decode MB/s |"
    );
    println!("|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    let mut fetch_rows = Vec::new();
    for &level in &levels {
        let whole = zstd::bulk::compress(&pack, level).unwrap().len();
        let (write, asset) = best(1, || {
            encode(&pack, |b| zstd::bulk::compress(b, level).ok()).unwrap()
        });
        let layout = Layout::of(&p).unwrap();
        let tail = SeekTable::tail_len(&asset).unwrap();
        let frames = SeekTable::parse(&asset[asset.len() - tail..])
            .unwrap()
            .check(asset.len() as u64, layout)
            .unwrap();
        let mut d = Decoder::new();
        let mut buf = Vec::new();
        let (ruzstd, n) = best(5, || {
            let mut n = 0;
            for (i, f) in frames.iter().enumerate() {
                let src = &asset[f.compressed.start as usize..f.compressed.end as usize];
                d.frame(i, &f, src, &mut buf).unwrap();
                n += buf.len();
            }
            n
        });
        assert_eq!(n, pack.len());
        let (c, out) = best(5, || zstd::stream::decode_all(asset.as_slice()).unwrap());
        assert!(out == pack);
        println!(
            "| {level} | {whole} | {} | {:+.2} % | {} | {} | {:.2} | {:.0} | {:.0} |",
            asset.len(),
            (asset.len() as f64 / whole as f64 - 1.0) * 100.0,
            frames.len(),
            frames.table_len(),
            write.as_secs_f64(),
            mb_s(n, ruzstd),
            mb_s(n, c),
        );
        if level == *levels.last().unwrap() {
            if let Some(p) = &out_path {
                std::fs::write(p, &asset).unwrap();
            }
            for s in frames.layout().sections() {
                let plan = frames.plan_sections(&[s.kind]).unwrap();
                let bytes: u64 = plan
                    .iter()
                    .map(|f| f.compressed.end - f.compressed.start)
                    .sum();
                let n: usize = plan.iter().map(|f| f.frames.len()).sum();
                fetch_rows.push((s.kind, s.len, n, plan.len(), bytes));
            }
            let head = frames.frame(0).unwrap();
            fetch_rows.insert(
                0,
                (
                    teigeodata_format::Kind(0),
                    head.decoded.end,
                    1,
                    1,
                    head.compressed.end - head.compressed.start,
                ),
            );
        }
    }
    println!();
    println!(
        "level {}: bytes to fetch one section (frames and requests)",
        levels.last().unwrap()
    );
    println!();
    println!("| section | raw B | frames | requests | fetch B |");
    println!("|---|---:|---:|---:|---:|");
    for (kind, len, n, reqs, bytes) in fetch_rows {
        let name = if kind.0 == 0 {
            "head".to_string()
        } else {
            kind.to_string()
        };
        println!("| {name} | {len} | {n} | {reqs} | {bytes} |");
    }
}
