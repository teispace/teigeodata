//! Measures an update between two real releases of one pack (4.7, D-174): the bytes section
//! re-download fetches over the new release's seekable frames, by `update_from`'s rule (a
//! frame is reused when every section it holds is in the old file with the same kind, part,
//! hash and length), and each section's compressed bytes, for modelling a delta pack.
//!
//! ```text
//! cargo run --release -p teigeodata-sync --example delta_measure -- <old.tgd> <new.tgd> [level]
//! ```
#![allow(
    clippy::print_stdout,
    clippy::unwrap_used,
    clippy::cast_precision_loss,
    clippy::expect_used,
    reason = "a measuring tool: prints its table, fails loudly"
)]

use std::collections::BTreeMap;

use teigeodata_format::{Pack, PackHead};
use teigeodata_sync::transport::{Layout, SeekTable, encode};

fn main() {
    let mut args = std::env::args().skip(1);
    let usage = "usage: delta_measure <old.tgd> <new.tgd> [level]";
    let old = std::fs::read(args.next().expect(usage)).unwrap();
    let new = std::fs::read(args.next().expect(usage)).unwrap();
    let level: i32 = args.next().map_or(19, |l| l.parse().unwrap());
    let oh = PackHead::parse(&old, old.len() as u64).unwrap();
    let nh = PackHead::parse(&new, new.len() as u64).unwrap();
    let p = Pack::open_verified(&new).unwrap();
    let asset = encode(&new, |b| zstd::bulk::compress(b, level).ok()).unwrap();
    let tail = SeekTable::tail_len(&asset).unwrap();
    let frames = SeekTable::parse(asset.get(asset.len() - tail..).unwrap())
        .unwrap()
        .check(asset.len() as u64, Layout::of(&p).unwrap())
        .unwrap();
    // Each new section: its byte range, kind name, rows, and whether the old file holds it.
    let spans: Vec<(u64, u64, String, u64, bool)> = nh
        .sections()
        .map(|s| {
            let same = oh
                .part(s.kind, s.part)
                .is_some_and(|o| o.hash == s.hash && o.length == s.length);
            (
                s.offset,
                s.offset + s.length,
                format!("{} part {:?}", s.kind, s.part),
                s.item_count,
                same,
            )
        })
        .collect();
    let body = spans.first().map_or(0, |s| s.0);
    // Per section: raw bytes, rows, unchanged, compressed bytes, compressed bytes to fetch.
    let mut by: BTreeMap<String, (u64, u64, bool, u64, u64)> = BTreeMap::new();
    let (mut fetch, mut total, mut head) = (0_u64, 0_u64, 0_u64);
    for f in frames.iter() {
        let c = f.compressed.end - f.compressed.start;
        total += c;
        if f.decoded.start < body {
            head += c;
            fetch += c;
            continue;
        }
        let inside: Vec<&(u64, u64, String, u64, bool)> = spans
            .iter()
            .filter(|s| s.0 < f.decoded.end && f.decoded.start < s.1)
            .collect();
        let reused = inside.iter().all(|s| s.4);
        if !reused {
            fetch += c;
        }
        for s in inside {
            let e = by.entry(s.2.clone()).or_insert((s.1 - s.0, s.3, s.4, 0, 0));
            e.3 += c;
            if !reused {
                e.4 += c;
            }
        }
    }
    println!(
        "| section | raw B | rows | unchanged | compressed B | fetch B |\n|---|---:|---:|---|---:|---:|"
    );
    println!("| head (and footer frame) | | | no | {head} | {head} |");
    for (k, (raw, rows, same, c, f)) in &by {
        println!(
            "| {k} | {raw} | {rows} | {} | {c} | {f} |",
            if *same { "yes" } else { "no" }
        );
    }
    println!(
        "\nlevel {level}: whole asset {} B ({total} B in frames); section re-download fetches {fetch} B ({:.1} %)",
        asset.len(),
        fetch as f64 / asset.len() as f64 * 100.0
    );
}
