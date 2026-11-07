//! The container: L1 (open), L2 (eager section hashing) and L3 (signature verification), from
//! the benchmark catalogue.
#![allow(
    missing_docs,
    reason = "criterion's macros generate undocumented items"
)]

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use teigeodata_bench::{bench_key, pack_of_size};
use teigeodata_format::Pack;

const MB: usize = 1 << 20;

fn packs() -> Vec<(usize, Vec<u8>)> {
    [1, 10, 100]
        .into_iter()
        .filter_map(|mb| pack_of_size(mb * MB, 1).ok().map(|p| (mb, p)))
        .collect()
}

fn open(c: &mut Criterion) {
    let packs = packs();
    let mut g = c.benchmark_group("L1-open");
    for (mb, bytes) in &packs {
        g.bench_with_input(
            BenchmarkId::from_parameter(format!("{mb}MB")),
            bytes,
            |b, p| {
                b.iter(|| Pack::open(black_box(p)).is_ok());
            },
        );
    }
    g.finish();

    let mut g = c.benchmark_group("L2-verify-eager");
    g.sample_size(10);
    for (mb, bytes) in &packs {
        let Ok(pack) = Pack::open(bytes) else {
            continue;
        };
        g.throughput(Throughput::Bytes(bytes.len() as u64));
        g.bench_with_input(
            BenchmarkId::from_parameter(format!("{mb}MB")),
            &pack,
            |b, p| {
                b.iter(|| p.verify_sections().is_ok());
            },
        );
    }
    g.finish();

    let keys = [bench_key().verifying_key()];
    let Some((_, small)) = packs.first() else {
        return;
    };
    let Ok(pack) = Pack::open(small) else { return };
    c.bench_function("L3-verify-signature", |b| {
        b.iter(|| pack.verify_signature(black_box(&keys)).is_ok());
    });
}

criterion_group!(benches, open);
criterion_main!(benches);
