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
use teigeodata_bench::{
    Rng, geonames_columns, geonames_path, geonames_tree, leb128_delta, leb128_delta_decode,
};
use teigeodata_core::bitpack::{BLOCK_LEN, Coding, Packed, encode};
use teigeodata_core::{Namespace, PlaceId};
use teigeodata_format::hierarchy::{self, Hierarchy};
use teigeodata_format::idmap::{self, IdMap};
use teigeodata_format::places::{self, PlaceColumns, Places};
use teigeodata_format::{Encoding, Kind, Pack, Writer};

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

/// PLACES and HIERARCHY of the Nepal tree: reads at random rows, and the deep checks.
fn sections(c: &mut Criterion) {
    let Some(tree) = geonames_tree(&geonames_path()) else {
        return;
    };
    let n = tree.points.len();
    let cols = PlaceColumns {
        points: &tree.points,
        kinds: &tree.kinds,
        admin_levels: &tree.levels,
        ..PlaceColumns::default()
    };
    let (Ok(pl), Ok(hi)) = (places::encode(&cols), hierarchy::encode(&tree.parents)) else {
        return;
    };
    let items = n as u64;
    let Ok(mut w) = Writer::new("np", "places", "2026.11.0") else {
        return;
    };
    if w.section_bytes(Kind::PLACES, Encoding::TABLE, items, &pl)
        .is_err()
        || w.section_bytes(Kind::HIERARCHY, Encoding::TABLE, items, &hi)
            .is_err()
    {
        return;
    }
    let Ok(bytes) = w.layout(false).and_then(|l| l.assemble(&[&pl, &hi])) else {
        return;
    };
    let Ok(pack) = Pack::open(&bytes) else {
        return;
    };
    let (Some(ps), Some(hs)) = (pack.section(Kind::PLACES), pack.section(Kind::HIERARCHY)) else {
        return;
    };
    let (Ok(places), Ok(tree_view)) = (Places::open(&ps), Hierarchy::open(&hs)) else {
        return;
    };
    let mut rng = Rng::new(5);
    let rows: Vec<usize> = (0..4096)
        .map(|_| (rng.next_u64() % n as u64) as usize)
        .collect();
    // Units with children: reading a children list is how a browse starts.
    let units: Vec<usize> = (0..n).filter(|&r| tree.levels[r] > 0).collect();

    let mut group = c.benchmark_group("B1-sections");
    group.throughput(Throughput::Elements(rows.len() as u64));
    group.bench_function("open-places-and-hierarchy", |b| {
        b.iter(|| Places::open(black_box(&ps)).is_ok() && Hierarchy::open(black_box(&hs)).is_ok());
    });
    group.bench_function("point", |b| {
        b.iter(|| {
            rows.iter().fold(0_i32, |a, &r| {
                a ^ places.point(r).map_or(0, teigeodata_core::Point::lat)
            })
        });
    });
    group.bench_function("kind", |b| {
        b.iter(|| rows.iter().filter(|&&r| places.kind(r).is_some()).count());
    });
    group.bench_function("parent", |b| {
        b.iter(|| {
            rows.iter()
                .fold(0, |a, &r| a ^ tree_view.parent(r).unwrap_or(0))
        });
    });
    group.bench_function("ancestors", |b| {
        b.iter(|| {
            rows.iter()
                .map(|&r| tree_view.ancestors(r).count())
                .sum::<usize>()
        });
    });
    group.throughput(Throughput::Elements(units.len() as u64));
    group.bench_function("children-of-every-unit", |b| {
        b.iter(|| {
            units
                .iter()
                .map(|&r| tree_view.children(r).count())
                .sum::<usize>()
        });
    });
    group.throughput(Throughput::Elements(items));
    group.bench_function("check-places", |b| b.iter(|| places.check().is_ok()));
    group.bench_function("check-hierarchy", |b| b.iter(|| tree_view.check().is_ok()));
    group.finish();
}

/// B5: ID → row through IDMAP, at a country's size and at 10⁶, with serials allocated in row
/// order (the forge's first build) and shuffled (the worst case). The out-of-tree comparison with
/// `ptr_hash` is on the results page.
fn ids(c: &mut Criterion) {
    let Some(np) = Namespace::from_country(*b"np") else {
        return;
    };
    let mut group = c.benchmark_group("B5-idmap");
    for n in [150_000_usize, 1_000_000] {
        for order in ["row-order", "shuffled"] {
            let mut rng = Rng::new(n as u64);
            let mut v: Vec<PlaceId> = (1..=n as u64).filter_map(|s| PlaceId::new(np, s)).collect();
            if order == "shuffled" {
                for i in (1..v.len()).rev() {
                    v.swap(i, (rng.next_u64() % (i as u64 + 1)) as usize);
                }
            }
            let Ok(body) = idmap::encode(&v) else {
                return;
            };
            let Ok(mut w) = Writer::new("np", "places", "2026.11.0") else {
                return;
            };
            if w.section_bytes(Kind::IDMAP, Encoding::TABLE, n as u64, &body)
                .is_err()
            {
                return;
            }
            let Ok(bytes) = w.layout(false).and_then(|l| l.assemble(&[&body])) else {
                return;
            };
            let Ok(pack) = Pack::open(&bytes) else {
                return;
            };
            let Some(Ok(map)) = pack.section(Kind::IDMAP).map(|s| IdMap::open(&s)) else {
                return;
            };
            let hits: Vec<PlaceId> = (0..4096)
                .map(|_| v[(rng.next_u64() % n as u64) as usize])
                .collect();
            let misses: Vec<PlaceId> = (0..4096)
                .filter_map(|_| PlaceId::new(np, n as u64 + 1 + rng.next_u64() % n as u64))
                .collect();
            group.throughput(Throughput::Elements(4096));
            group.bench_function(format!("row-hit/{n}/{order}"), |b| {
                b.iter(|| {
                    hits.iter()
                        .filter_map(|&k| map.row(black_box(k)))
                        .sum::<usize>()
                });
            });
            group.bench_function(format!("row-miss/{n}/{order}"), |b| {
                b.iter(|| {
                    misses
                        .iter()
                        .filter(|&&k| map.row(black_box(k)).is_some())
                        .count()
                });
            });
            group.bench_function(format!("id/{n}/{order}"), |b| {
                b.iter(|| (0..4096).filter_map(|i| map.id(black_box(i * 31))).count());
            });
        }
    }
    group.finish();
}

criterion_group!(benches, b1, sections, ids);
criterion_main!(benches);
