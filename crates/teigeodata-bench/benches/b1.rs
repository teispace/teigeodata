//! B1: integer blocks on real columns (`GeoNames` Nepal in Hilbert order). Decode-all and random
//! access, for our codec (128-value blocks, FOR or delta), LEB128 deltas and the
//! `bitpacking` crate. Sizes: `cargo run --release -p teigeodata-bench --example b1-sizes`.
#![allow(
    missing_docs,
    reason = "criterion's macros generate undocumented items"
)]
#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::print_stderr,
    reason = "a harness: buffers it sized itself, probes below n, a skip notice"
)]

use std::hint::black_box;

use bitpacking::{BitPacker, BitPacker4x};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use teigeodata_bench::{Rng, geonames_columns, geonames_path, leb128_delta, leb128_delta_decode};
use teigeodata_core::bitpack::{BLOCK_LEN, Coding, Packed, encode};

const VARIANTS: [(Coding, &str); 2] = [(Coding::For, "ours-for"), (Coding::Delta, "ours-delta")];

/// `bitpacking`'s 4x packer with a FOR base per block (it has none of its own).
struct Rival {
    blocks: Vec<(u32, u8, usize)>,
    bytes: Vec<u8>,
}

fn rival(values: &[u32]) -> Rival {
    let bp = BitPacker4x::new();
    let mut blocks = Vec::new();
    let mut bytes = vec![0; values.len() * 4 + 512];
    let mut at = 0;
    let mut buf = [0_u32; 128];
    for chunk in values.chunks(128) {
        let min = chunk.iter().copied().min().unwrap_or(0);
        buf.fill(0);
        for (b, v) in buf.iter_mut().zip(chunk) {
            *b = v - min;
        }
        let nb = bp.num_bits(&buf);
        let n = bp.compress(&buf, &mut bytes[at..], nb);
        blocks.push((min, nb, at));
        at += n;
    }
    bytes.truncate(at);
    Rival { blocks, bytes }
}

fn b1(c: &mut Criterion) {
    let Some(cols) = geonames_columns(&geonames_path()) else {
        eprintln!("B1 skipped: {} is missing", geonames_path().display());
        return;
    };
    let bp = BitPacker4x::new();
    for (name, values) in [("lat", &cols.lat), ("lon", &cols.lon), ("ids", &cols.ids)] {
        let n = values.len();
        let mut out = vec![0_u32; n.next_multiple_of(BLOCK_LEN)];
        let mut idx = Rng::new(9);
        let probes: Vec<usize> = (0..4096)
            .map(|_| (idx.next_u64() % n as u64) as usize)
            .collect();

        let mut group = c.benchmark_group(format!("B1-decode-{name}"));
        group.throughput(Throughput::Elements(n as u64));
        for (coding, label) in VARIANTS {
            let bytes = encode(values, coding);
            let Ok(col) = Packed::parse(&bytes) else {
                continue;
            };
            group.bench_function(label, |b| b.iter(|| col.decode_all(black_box(&mut out))));
        }
        let leb = leb128_delta(values);
        group.bench_function("leb128-delta", |b| {
            b.iter(|| leb128_delta_decode(black_box(&leb), &mut out));
        });
        let packed4 = rival(values);
        group.bench_function("bitpacking-4x-for", |b| {
            b.iter(|| {
                for (i, &(min, nb, at)) in packed4.blocks.iter().enumerate() {
                    let o = &mut out[i * 128..i * 128 + 128];
                    bp.decompress(&packed4.bytes[at..], o, nb);
                    for v in o.iter_mut() {
                        *v += min;
                    }
                }
            });
        });
        group.finish();

        let mut group = c.benchmark_group(format!("B1-get-{name}"));
        group.throughput(Throughput::Elements(probes.len() as u64));
        for (coding, label) in VARIANTS {
            let bytes = encode(values, coding);
            let Ok(col) = Packed::parse(&bytes) else {
                continue;
            };
            group.bench_with_input(BenchmarkId::from_parameter(label), &probes, |b, probes| {
                b.iter(|| {
                    probes
                        .iter()
                        .fold(0_u32, |a, &i| a ^ col.get(i).unwrap_or(0))
                });
            });
        }
        group.finish();
    }
}

criterion_group!(benches, b1);
criterion_main!(benches);
