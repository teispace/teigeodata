//! B4: the BLOBS section on the `GeoNames` Nepal rows as cold per-place text: bytes per blob and
//! the time to read one at random and all in order, by frame size, with and without a trained
//! dictionary, compressed as the forge does (C zstd, level 19, no dictionary ID or checksum)
//! and decoded with `ruzstd`.
#![allow(
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "a report printed for the results page; sizes far below 2^52"
)]

use std::hint::black_box;
use std::time::Instant;

use teigeodata_bench::{Rng, geonames_path, pack_with};
use teigeodata_format::blobs::{self, Blobs};
use teigeodata_format::{Encoding, Kind, Pack};

fn c19(dict: Option<&[u8]>) -> impl FnMut(&[u8]) -> Option<Vec<u8>> {
    let c = match dict {
        Some(d) => zstd::bulk::Compressor::with_dictionary(19, d),
        None => zstd::bulk::Compressor::new(19),
    };
    let mut c = c.ok();
    if let Some(c) = c.as_mut() {
        let _ = c.include_dictid(false);
        let _ = c.include_checksum(false);
    }
    move |raw| c.as_mut()?.compress(raw).ok()
}

fn main() {
    let Ok(text) = std::fs::read_to_string(geonames_path()) else {
        println!("missing {}", geonames_path().display());
        return;
    };
    let rows: Vec<&[u8]> = text.lines().map(str::as_bytes).collect();
    let n = rows.len();
    let raw: usize = rows.iter().map(|r| r.len()).sum();
    println!(
        "{n} blobs, {raw} B raw ({:.1} B each)\n",
        raw as f64 / n as f64
    );
    let dict = zstd::dict::from_samples(&rows, 16 << 10).unwrap_or_default();
    println!(
        "| Frames | Dictionary | BLOBS B | B per blob | ratio | random blob | all in order | encode |"
    );
    println!("|---:|---|---:|---:|---:|---:|---:|---:|");
    let mut rng = Rng::new(4);
    let probes: Vec<usize> = (0..20_000)
        .map(|_| (rng.next_u64() % n as u64) as usize)
        .collect();
    for kib in [1_usize, 4, 16, 64] {
        for d in [None, Some(dict.as_slice())] {
            let t0 = Instant::now();
            let Ok(body) = blobs::encode(&rows, d, kib << 10, c19(d)) else {
                println!("| {kib} KiB | encode failed |");
                continue;
            };
            let enc = t0.elapsed().as_secs_f64();
            let section = (Kind::BLOBS, Encoding::TABLE, n as u64, body.as_slice());
            let Ok(bytes) = pack_with("blobs", 1, &[section], false) else {
                return;
            };
            let Ok(pack) = Pack::open(&bytes) else {
                return;
            };
            let Some(Ok(b)) = pack.section(Kind::BLOBS).map(|s| Blobs::open(&s)) else {
                return;
            };
            let Ok(mut dec) = b.decoder() else {
                return;
            };
            let mut sum = 0_usize;
            let t0 = Instant::now();
            for &i in &probes {
                sum += dec.get(black_box(i)).map_or(0, <[u8]>::len);
            }
            let one = t0.elapsed().as_secs_f64() * 1e6 / probes.len() as f64;
            let t0 = Instant::now();
            for i in 0..n {
                sum += dec.get(black_box(i)).map_or(0, <[u8]>::len);
            }
            let all = t0.elapsed().as_secs_f64();
            black_box(sum);
            println!(
                "| {kib} KiB | {} | {} | {:.1} | {:.3} | {one:.1} µs | {:.1} ms ({:.0} MB/s) | {:.1} s |",
                if d.is_some() { "16 KiB" } else { "none" },
                body.len(),
                body.len() as f64 / n as f64,
                body.len() as f64 / raw as f64,
                all * 1e3,
                raw as f64 / all / 1e6,
                enc,
            );
        }
    }
}
