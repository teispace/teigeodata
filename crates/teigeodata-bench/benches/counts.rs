//! Instruction counts of the hot paths under Callgrind (gungraun): the load-independent PR gate
//! (docs/06-engineering/ci.md). It builds everywhere, so lints and type checks run on every host;
//! it runs on Linux, where Valgrind is. Every input is synthetic and deterministic, built in a setup function so its cost is
//! not counted.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    unused_qualifications,
    reason = "gungraun's macros generate undocumented items and qualified paths; setup data is valid by construction"
)]

#[cfg_attr(
    not(target_os = "linux"),
    allow(dead_code, reason = "run on Linux only, where Valgrind is")
)]
mod counts {
    use std::hint::black_box;

    use gungraun::prelude::*;
    use teigeodata_bench::{bench_key, pack_of_size, pack_with, world_points};
    use teigeodata_core::bitpack::{Coding, Packed, encode};
    use teigeodata_core::bits::{self, Bits};
    use teigeodata_core::{Namespace, PlaceId, hilbert};
    use teigeodata_format::blobs::{self, Blobs};
    use teigeodata_format::idmap::{self, IdMap};
    use teigeodata_format::names::{self, NameRecord, Names, TEXT_BUFFER};
    use teigeodata_format::{Encoding, Kind, Pack};

    /// Setup data lives for the whole run: leaked once per benchmark process.
    fn leak(v: Vec<u8>) -> &'static [u8] {
        Box::leak(v.into_boxed_slice())
    }

    fn column(coding: Coding) -> &'static [u8] {
        let v: Vec<u32> = (0..10_000_u32).map(|i| i * 17 + i % 5).collect();
        leak(encode(&v, coding))
    }

    #[library_benchmark]
    #[bench::for_(column(Coding::For))]
    #[bench::delta(column(Coding::Delta))]
    #[bench::ramp(column(Coding::Ramp))]
    fn bitpack_get(bytes: &'static [u8]) -> u32 {
        let p = Packed::parse(bytes).unwrap();
        black_box(p.get(black_box(5_555)).unwrap_or(0))
    }

    #[library_benchmark]
    #[bench::delta(column(Coding::Delta))]
    fn bitpack_decode_all(bytes: &'static [u8]) -> u32 {
        let p = Packed::parse(bytes).unwrap();
        let mut out = vec![0; p.len()];
        p.decode_all(&mut out);
        black_box(out.last().copied().unwrap_or(0))
    }

    #[library_benchmark]
    fn hilbert_1000() -> u64 {
        let points = world_points(1_000, 7);
        black_box(points.iter().fold(0, |a, p| a ^ hilbert(black_box(*p))))
    }

    fn bitmap() -> &'static [u8] {
        leak(bits::encode(100_000, (0..100_000).filter(|i| i % 7 == 0)).unwrap())
    }

    #[library_benchmark]
    #[bench::rank(bitmap())]
    fn bits_rank(bytes: &'static [u8]) -> usize {
        let b = Bits::parse(bytes).unwrap();
        black_box(b.rank(black_box(77_777)))
    }

    fn pack(mb: usize) -> &'static [u8] {
        leak(pack_of_size(mb << 20, 1).unwrap())
    }

    #[library_benchmark]
    #[bench::one_mib(pack(1))]
    fn pack_open(bytes: &'static [u8]) -> usize {
        black_box(Pack::open(black_box(bytes)).map_or(0, |p| p.sections().count()))
    }

    #[library_benchmark]
    #[bench::one_mib(pack(1))]
    fn pack_verify_eager(bytes: &'static [u8]) -> bool {
        black_box(Pack::open(bytes).unwrap().verify_sections().is_ok())
    }

    #[library_benchmark]
    #[bench::one_mib(pack(1))]
    fn pack_signature(bytes: &'static [u8]) -> usize {
        let key = bench_key().verifying_key();
        black_box(
            Pack::open(bytes)
                .unwrap()
                .verify_signature(&[key])
                .unwrap_or(9),
        )
    }

    #[library_benchmark]
    #[bench::four_mib(pack(4))]
    fn block_first_read(bytes: &'static [u8]) -> usize {
        let p = Pack::open(bytes).unwrap();
        let blocks = p.blocks(Kind::IDMAP).unwrap().unwrap();
        black_box(blocks.verify(black_box(3)).map_or(0, <[u8]>::len))
    }

    fn ids_pack() -> &'static [u8] {
        let ns = Namespace::from_country(*b"np").unwrap();
        let ids: Vec<PlaceId> = (1..=20_000_u64)
            .map(|s| PlaceId::new(ns, s * 3).unwrap())
            .collect();
        let body = idmap::encode(&ids).unwrap();
        let section = (
            Kind::IDMAP,
            Encoding::TABLE,
            ids.len() as u64,
            body.as_slice(),
        );
        leak(pack_with("places", 1, &[section], false).unwrap())
    }

    #[library_benchmark]
    #[bench::hit(ids_pack())]
    fn idmap_row(bytes: &'static [u8]) -> usize {
        let p = Pack::open(bytes).unwrap();
        let m = IdMap::open(&p.section(Kind::IDMAP).unwrap()).unwrap();
        let ns = Namespace::from_country(*b"np").unwrap();
        black_box(m.row(PlaceId::new(ns, 3 * 12_345).unwrap()).unwrap_or(0))
    }

    fn names_pack() -> &'static [u8] {
        let texts: Vec<[String; 2]> = (0..5_000)
            .map(|i| [format!("Place {i} Chowk"), format!("ठाउँ {i}")])
            .collect();
        let recs: Vec<Vec<NameRecord<'_>>> = texts
            .iter()
            .map(|t| {
                t.iter()
                    .map(|s| NameRecord {
                        text: s,
                        ..NameRecord::default()
                    })
                    .collect()
            })
            .collect();
        let slices: Vec<&[NameRecord<'_>]> = recs.iter().map(Vec::as_slice).collect();
        let body = names::encode(&slices).unwrap();
        let section = (
            Kind::NAMES,
            Encoding::TABLE,
            texts.len() as u64,
            body.as_slice(),
        );
        leak(pack_with("places", 1, &[section], false).unwrap())
    }

    #[library_benchmark]
    #[bench::canonical(names_pack())]
    fn names_canonical(bytes: &'static [u8]) -> usize {
        let p = Pack::open(bytes).unwrap();
        let n = Names::open(&p.section(Kind::NAMES).unwrap()).unwrap();
        let mut buf = [0; TEXT_BUFFER];
        black_box(
            n.canonical(black_box(2_345))
                .and_then(|c| c.text(&mut buf).ok().map(str::len))
                .unwrap_or(0),
        )
    }

    fn blobs_pack() -> &'static [u8] {
        let rows: Vec<Vec<u8>> = (0..5_000)
            .map(|i| format!("© contributors, record {i}; level {}", i % 7).into_bytes())
            .collect();
        let refs: Vec<&[u8]> = rows.iter().map(Vec::as_slice).collect();
        let mut c = zstd::bulk::Compressor::new(19).unwrap();
        c.include_checksum(false).unwrap();
        let body = blobs::encode(&refs, None, 16 << 10, |r| c.compress(r).ok()).unwrap();
        let section = (
            Kind::BLOBS,
            Encoding::TABLE,
            rows.len() as u64,
            body.as_slice(),
        );
        leak(pack_with("blobs", 1, &[section], false).unwrap())
    }

    #[library_benchmark]
    #[bench::random(blobs_pack())]
    fn blob_get(bytes: &'static [u8]) -> usize {
        let p = Pack::open(bytes).unwrap();
        let b = Blobs::open(&p.section(Kind::BLOBS).unwrap()).unwrap();
        let mut d = b.decoder().unwrap();
        black_box(d.get(black_box(2_345)).map_or(0, <[u8]>::len))
    }

    library_benchmark_group!(
        name = core_paths,
        benchmarks = [bitpack_get, bitpack_decode_all, hilbert_1000, bits_rank]
    );
    library_benchmark_group!(
        name = format_paths,
        benchmarks = [
            pack_open,
            pack_verify_eager,
            pack_signature,
            block_first_read,
            idmap_row,
            names_canonical,
            blob_get
        ]
    );
}

#[cfg(target_os = "linux")]
use counts::{core_paths, format_paths};

#[cfg(target_os = "linux")]
gungraun::main!(library_benchmark_groups = core_paths, format_paths);

#[cfg(not(target_os = "linux"))]
fn main() {}
