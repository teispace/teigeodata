//! Phase 0 spike S1: coordinate and name codecs on a real Nepal sample (GeoNames `NP.txt`).
//!
//! Measures bytes per place and decode speed for:
//! - coordinates on the 1e-7° grid (ADR-0029) and a 1e-6° grid, in source, Morton and Hilbert
//!   order, as raw `i32`, delta+zigzag+LEB128, delta+zigzag+bit-packed blocks of 128, and zstd;
//! - elevation (the DEM column) delta-coded in the same orders;
//! - names with random access: raw + offsets, FSST, zstd blocks of 64, lz4 blocks of 64,
//!   front coding of the sorted unique keys; plus whole-column zstd as a lower bound.
//!
//! A spike: run it, read the table, record the result in the research note. Not shipped.
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::doc_markdown,
    missing_docs,
    reason = "a measurement spike: prints its report and fails loudly on bad input"
)]

use std::hint::black_box;
use std::time::Instant;

struct Place {
    lat7: i32,
    lon7: i32,
    dem: i32,
    name: String,
    alternates: Vec<String>,
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: spike-s1-codecs <NP.txt>");
    let text = std::fs::read_to_string(&path).expect("read the GeoNames file");
    let places = parse(&text);
    let n = places.len();
    println!("# Spike S1: codecs on GeoNames NP ({n} places)\n");

    coordinates(&places);
    elevation(&places);
    names(&places);
}

fn parse(text: &str) -> Vec<Place> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 19 {
                return None;
            }
            let lat: f64 = f[4].parse().ok()?;
            let lon: f64 = f[5].parse().ok()?;
            Some(Place {
                lat7: (lat * 1e7).round() as i32,
                lon7: (lon * 1e7).round() as i32,
                dem: f[16].parse().unwrap_or(0),
                name: f[1].to_string(),
                alternates: f[3]
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect(),
            })
        })
        .collect()
}

// ---------- orders ----------

/// Maps a 1e-7° coordinate pair onto an unsigned 2^31 grid per axis.
fn grid(lat7: i32, lon7: i32) -> (u64, u64) {
    let x = (i64::from(lon7) + 1_800_000_000) as u64; // 0..3.6e9 < 2^32
    let y = (i64::from(lat7) + 900_000_000) as u64;
    (x >> 1, y >> 1) // 31 bits each
}

fn morton(x: u64, y: u64) -> u64 {
    fn spread(mut v: u64) -> u64 {
        v &= 0x7fff_ffff;
        v = (v | (v << 16)) & 0x0000_ffff_0000_ffff;
        v = (v | (v << 8)) & 0x00ff_00ff_00ff_00ff;
        v = (v | (v << 4)) & 0x0f0f_0f0f_0f0f_0f0f;
        v = (v | (v << 2)) & 0x3333_3333_3333_3333;
        v = (v | (v << 1)) & 0x5555_5555_5555_5555;
        v
    }
    spread(x) | (spread(y) << 1)
}

/// Hilbert index on a 2^31 × 2^31 grid (the classic iterative xy→d).
fn hilbert(mut x: u64, mut y: u64) -> u64 {
    let n: u64 = 1 << 31;
    let mut d: u64 = 0;
    let mut s = n >> 1;
    while s > 0 {
        let rx = u64::from(x & s > 0);
        let ry = u64::from(y & s > 0);
        d += s * s * ((3 * rx) ^ ry);
        if ry == 0 {
            if rx == 1 {
                x = n - 1 - x;
                y = n - 1 - y;
            }
            std::mem::swap(&mut x, &mut y);
        }
        s >>= 1;
    }
    d
}

fn order(places: &[Place], key: impl Fn(&Place) -> u64) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..places.len()).collect();
    idx.sort_by_key(|&i| (key(&places[i]), i));
    idx
}

// ---------- integer codecs ----------

fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

fn unzigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

fn deltas(values: &[i64]) -> Vec<u64> {
    let mut prev = 0i64;
    values
        .iter()
        .map(|&v| {
            let d = v - prev;
            prev = v;
            zigzag(d)
        })
        .collect()
}

fn leb128(values: &[u64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 3);
    for &v in values {
        let mut v = v;
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
    }
    out
}

fn leb128_decode_sum(bytes: &[u8], n: usize) -> i64 {
    let mut acc = 0i64;
    let mut sum = 0i64;
    let mut i = 0;
    for _ in 0..n {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let b = bytes[i];
            i += 1;
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        acc += unzigzag(v);
        sum = sum.wrapping_add(acc);
    }
    sum
}

/// Bit-packed blocks of 128: one width byte per block, then 128 values of that width.
fn bitpack128(values: &[u64]) -> Vec<u8> {
    let mut out = Vec::new();
    for block in values.chunks(128) {
        let max = block.iter().copied().max().unwrap_or(0);
        let width = 64 - max.leading_zeros();
        out.push(width as u8);
        let mut acc: u128 = 0;
        let mut bits = 0u32;
        for &v in block {
            acc |= u128::from(v) << bits;
            bits += width;
            while bits >= 8 {
                out.push((acc & 0xff) as u8);
                acc >>= 8;
                bits -= 8;
            }
        }
        if bits > 0 {
            out.push((acc & 0xff) as u8);
        }
    }
    out
}

fn bitpack128_decode_sum(bytes: &[u8], n: usize) -> i64 {
    let mut i = 0;
    let mut left = n;
    let mut acc = 0i64;
    let mut sum = 0i64;
    while left > 0 {
        let width = u32::from(bytes[i]);
        i += 1;
        let count = left.min(128);
        let mask = if width == 64 {
            u64::MAX
        } else {
            (1u64 << width) - 1
        };
        let mut buf: u128 = 0;
        let mut have = 0u32;
        for _ in 0..count {
            while have < width {
                buf |= u128::from(bytes[i]) << have;
                i += 1;
                have += 8;
            }
            let v = (buf as u64) & mask;
            buf >>= width;
            have -= width;
            acc += unzigzag(v);
            sum = sum.wrapping_add(acc);
        }
        // Each block ends on a byte boundary.
        left -= count;
    }
    sum
}

fn zstd_len(bytes: &[u8], level: i32) -> usize {
    zstd::bulk::compress(bytes, level).expect("zstd").len()
}

fn per_place(bytes: usize, n: usize) -> f64 {
    bytes as f64 / n as f64
}

fn time_ns_per_item(n: usize, reps: u32, mut f: impl FnMut() -> i64) -> f64 {
    let start = Instant::now();
    let mut guard = 0i64;
    for _ in 0..reps {
        guard = guard.wrapping_add(black_box(f()));
    }
    black_box(guard);
    start.elapsed().as_nanos() as f64 / (f64::from(reps) * n as f64)
}

// ---------- reports ----------

fn coordinates(places: &[Place]) {
    let n = places.len();
    println!("## Coordinates (lat+lon), bytes per place\n");
    println!(
        "| grid | order | raw i32 | delta+LEB128 | delta+bitpack128 | delta+LEB128+zstd19 | decode LEB128 ns/coord | decode bitpack ns/coord |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|---:|");
    let orders: [(&str, Vec<usize>); 3] = [
        ("source", (0..n).collect()),
        (
            "morton",
            order(places, |p| {
                let (x, y) = grid(p.lat7, p.lon7);
                morton(x, y)
            }),
        ),
        (
            "hilbert",
            order(places, |p| {
                let (x, y) = grid(p.lat7, p.lon7);
                hilbert(x, y)
            }),
        ),
    ];
    for (grid_name, div) in [("1e-7", 1i64), ("1e-6", 10i64)] {
        for (name, idx) in &orders {
            let lat: Vec<i64> = idx
                .iter()
                .map(|&i| i64::from(places[i].lat7) / div)
                .collect();
            let lon: Vec<i64> = idx
                .iter()
                .map(|&i| i64::from(places[i].lon7) / div)
                .collect();
            let (dlat, dlon) = (deltas(&lat), deltas(&lon));
            let (llat, llon) = (leb128(&dlat), leb128(&dlon));
            let (blat, blon) = (bitpack128(&dlat), bitpack128(&dlon));
            assert_eq!(
                leb128_decode_sum(&llat, n),
                lat.iter().fold(0i64, |a, &v| a.wrapping_add(v))
            );
            assert_eq!(
                bitpack128_decode_sum(&blat, n),
                lat.iter().fold(0i64, |a, &v| a.wrapping_add(v))
            );
            let leb = llat.len() + llon.len();
            let bp = blat.len() + blon.len();
            let z = zstd_len(&llat, 19) + zstd_len(&llon, 19);
            let t_leb = time_ns_per_item(n, 50, || leb128_decode_sum(&llat, n));
            let t_bp = time_ns_per_item(n, 50, || bitpack128_decode_sum(&blat, n));
            println!(
                "| {grid_name} | {name} | 8.00 | {:.2} | {:.2} | {:.2} | {t_leb:.2} | {t_bp:.2} |",
                per_place(leb, n),
                per_place(bp, n),
                per_place(z, n)
            );
        }
    }
    println!();
}

fn elevation(places: &[Place]) {
    let n = places.len();
    println!("## Elevation (GeoNames DEM column, metres), bytes per place\n");
    println!("| order | raw i16 | delta+LEB128 | delta+bitpack128 |");
    println!("|---|---:|---:|---:|");
    for (name, idx) in [
        ("source", (0..n).collect::<Vec<_>>()),
        (
            "hilbert",
            order(places, |p| {
                let (x, y) = grid(p.lat7, p.lon7);
                hilbert(x, y)
            }),
        ),
    ] {
        let v: Vec<i64> = idx.iter().map(|&i| i64::from(places[i].dem)).collect();
        let d = deltas(&v);
        println!(
            "| {name} | 2.00 | {:.2} | {:.2} |",
            per_place(leb128(&d).len(), n),
            per_place(bitpack128(&d).len(), n)
        );
    }
    println!();
}

fn names(places: &[Place]) {
    let primary: Vec<&str> = places.iter().map(|p| p.name.as_str()).collect();
    let all: Vec<&str> = places
        .iter()
        .flat_map(|p| {
            std::iter::once(p.name.as_str()).chain(p.alternates.iter().map(String::as_str))
        })
        .collect();
    println!("## Names with random access\n");
    println!(
        "Offsets are counted as 4 bytes per string for raw and FSST, and 4 per block for blocks.\n"
    );
    println!(
        "| set | strings | raw bytes | raw+offsets | FSST (+table) | zstd-3 blocks of 64 | lz4 blocks of 64 | whole zstd-19 (no random access) | FSST ns/string | zstd ns/block | lz4 ns/block |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for (label, set) in [("primary", &primary), ("all names", &all)] {
        let raw: usize = set.iter().map(|s| s.len()).sum();
        let offsets = 4 * set.len();

        let lines: Vec<&[u8]> = set.iter().map(|s| s.as_bytes()).collect();
        let compressor = fsst::Compressor::train(&lines);
        let compressed = compressor.compress_bulk(&lines);
        let fsst_bytes: usize = compressed.iter().map(Vec::len).sum::<usize>() + 255 * 9;
        let dec = compressor.decompressor();
        for (c, s) in compressed.iter().zip(set.iter()).take(1000) {
            assert_eq!(dec.decompress(c), s.as_bytes());
        }
        let t_fsst = time_ns_per_item(compressed.len(), 5, || {
            compressed
                .iter()
                .map(|c| dec.decompress(c).len() as i64)
                .sum()
        });

        let mut zblocks = 0;
        let mut lblocks = 0;
        let mut zenc = Vec::new();
        let mut lenc = Vec::new();
        for block in set.chunks(64) {
            let joined = block.join("\0");
            let z = zstd::bulk::compress(joined.as_bytes(), 3).expect("zstd");
            let l = lz4_flex::block::compress_prepend_size(joined.as_bytes());
            zblocks += z.len() + 4;
            lblocks += l.len() + 4;
            zenc.push((z, joined.len()));
            lenc.push(l);
        }
        // Cost of fetching one string = decompressing its whole block.
        let t_zblock = time_ns_per_item(zenc.len(), 3, || {
            zenc.iter()
                .map(|(z, cap)| zstd::bulk::decompress(z, *cap).expect("zstd").len() as i64)
                .sum()
        });
        let t_lblock = time_ns_per_item(lenc.len(), 3, || {
            lenc.iter()
                .map(|l| {
                    lz4_flex::block::decompress_size_prepended(l)
                        .expect("lz4")
                        .len() as i64
                })
                .sum()
        });
        let whole = zstd_len(set.join("\0").as_bytes(), 19);
        println!(
            "| {label} | {} | {raw} | {} | {} | {zblocks} | {lblocks} | {whole} | {t_fsst:.1} | {t_zblock:.0} | {t_lblock:.0} |",
            set.len(),
            raw + offsets,
            fsst_bytes + offsets,
        );
    }

    // Front coding of the sorted unique lowercase keys (the search dictionary's input).
    let mut keys: Vec<String> = all.iter().map(|s| s.to_lowercase()).collect();
    keys.sort();
    keys.dedup();
    let raw: usize = keys.iter().map(String::len).sum();
    let mut fc = 0usize;
    let mut prev = "";
    for k in &keys {
        let common = prev
            .bytes()
            .zip(k.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        fc += 2 + (k.len() - common); // a shared-length byte, a suffix-length byte, the suffix
        prev = k;
    }
    println!(
        "\nSearch keys: {} unique lowercase names, {raw} raw bytes, {fc} bytes front-coded ({:.1}%).",
        keys.len(),
        100.0 * fc as f64 / raw as f64
    );
}
