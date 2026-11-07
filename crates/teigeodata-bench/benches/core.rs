//! Core types: Hilbert order (A/B against the reference loop) and the ID text form.
#![allow(
    missing_docs,
    reason = "criterion's macros generate undocumented items"
)]

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use teigeodata_bench::{hilbert_reference, world_points};
use teigeodata_core::{Namespace, PlaceId, hilbert};

fn hilbert_order(c: &mut Criterion) {
    let points = world_points(100_000, 7);
    let mut g = c.benchmark_group("hilbert");
    g.throughput(Throughput::Elements(points.len() as u64));
    g.bench_function("prefix-scan", |b| {
        b.iter(|| points.iter().fold(0, |a, p| a ^ hilbert(black_box(*p))));
    });
    g.bench_function("reference-loop", |b| {
        b.iter(|| {
            points
                .iter()
                .fold(0, |a, p| a ^ hilbert_reference(black_box(*p)))
        });
    });
    g.finish();
}

fn id_text(c: &mut Criterion) {
    let mut g = c.benchmark_group("place-id");
    for serial in [1_u64, 0x3_a5f, (1 << 48) - 1] {
        let Some(id) = Namespace::from_country(*b"np").and_then(|ns| PlaceId::new(ns, serial))
        else {
            continue;
        };
        let text = id.to_string();
        g.bench_with_input(BenchmarkId::new("encode", text.len()), &id, |b, id| {
            let mut buf = [0; 20];
            b.iter(|| black_box(id).encode(&mut buf).len());
        });
        g.bench_with_input(BenchmarkId::new("parse", text.len()), &text, |b, t| {
            b.iter(|| black_box(t.as_str()).parse::<PlaceId>());
        });
    }
    g.finish();
}

criterion_group!(benches, hilbert_order, id_text);
criterion_main!(benches);
