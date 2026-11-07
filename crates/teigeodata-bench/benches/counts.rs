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
    use std::ops::ControlFlow;

    use teigeodata_bench::{Rng, bench_key, pack_of_size, pack_with, world_points};
    use teigeodata_core::bitpack::{Coding, Packed, encode};
    use teigeodata_core::bits::{self, Bits};
    use teigeodata_core::{E6, Namespace, PlaceId, PlaceKind, Point, hilbert};
    use teigeodata_format::blobs::{self, Blobs};
    use teigeodata_format::idmap::{self, IdMap};
    use teigeodata_format::names::{self, NameRecord, Names, TEXT_BUFFER};
    use teigeodata_format::place_index::{self, Neighbour, PlaceIndex, Rect};
    use teigeodata_format::places::{self, PlaceColumns, Places};
    use teigeodata_format::region::{self, Options, Region};
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

    /// 100,000 uniform world places in Hilbert order, with their `PLACE_INDEX` at node 16.
    fn places_pack() -> &'static [u8] {
        let mut r = Rng::new(11);
        let mut points: Vec<Point<E6>> = (0..100_000)
            .filter_map(|_| {
                let v = r.next_u64();
                let lat = i32::try_from(v % 180_000_001).ok()? - 90_000_000;
                let lon = i32::try_from((v >> 28) % 360_000_001).ok()? - 180_000_000;
                Point::from_units(lat, lon)
            })
            .collect();
        points.sort_by_key(|p| (hilbert(*p), *p));
        let kinds = vec![PlaceKind::Settlement; points.len()];
        let body = places::encode(&PlaceColumns {
            points: &points,
            kinds: &kinds,
            ..PlaceColumns::default()
        })
        .unwrap();
        let index = place_index::encode(&points, 16).unwrap();
        let n = points.len() as u64;
        leak(
            pack_with(
                "places",
                1,
                &[
                    (Kind::PLACES, Encoding::TABLE, n, body.as_slice()),
                    (Kind::PLACE_INDEX, Encoding::TABLE, n, index.as_slice()),
                ],
                false,
            )
            .unwrap(),
        )
    }

    fn place_index(p: &Pack<'static>) -> PlaceIndex<'static> {
        let places = Places::open(&p.section(Kind::PLACES).unwrap()).unwrap();
        PlaceIndex::open(&p.section(Kind::PLACE_INDEX).unwrap(), places).unwrap()
    }

    // A 1° box around Kathmandu.
    #[library_benchmark]
    #[bench::district(places_pack())]
    fn place_within(bytes: &'static [u8]) -> usize {
        let p = Pack::open(bytes).unwrap();
        let ix = place_index(&p);
        let rect = Rect {
            min_lat: 27_200_000,
            max_lat: 28_200_000,
            min_lon: 84_800_000,
            max_lon: 85_800_000,
        };
        let mut n = 0;
        let _ = ix.within(black_box(rect), |_| {
            n += 1;
            ControlFlow::Continue(())
        });
        black_box(n)
    }

    // The 10 places nearest to a mid-latitude point.
    #[library_benchmark]
    #[bench::k10(places_pack())]
    fn place_nearest(bytes: &'static [u8]) -> usize {
        let p = Pack::open(bytes).unwrap();
        let ix = place_index(&p);
        let mut out = [Neighbour::default(); 10];
        let at = Point::<E6>::from_units(27_700_000, 85_300_000).unwrap();
        black_box(ix.nearest(black_box(at), &mut out))
    }

    /// A 4 × 4 jittered grid of regions over Nepal's box, with a world background: shared
    /// borders, edge-free leaves and border leaves.
    fn region_pack() -> &'static [u8] {
        let mut r = Rng::new(17);
        let (x0, y0, step) = (800_000_000_i32, 260_000_000_i32, 20_000_000_i32);
        let mut jit = |v: i32| v + i32::try_from(r.next_u64() % 2_000_001).unwrap() - 1_000_000;
        let lattice: Vec<Vec<[i32; 2]>> = (0..5)
            .map(|j| {
                (0..5)
                    .map(|i| {
                        let inner = |k: i32| k > 0 && k < 4;
                        let x = x0 + i * step;
                        let y = y0 + j * step;
                        [
                            if inner(i) { jit(x) } else { x },
                            if inner(j) { jit(y) } else { y },
                        ]
                    })
                    .collect()
            })
            .collect();
        let mut regions: Vec<Vec<Vec<[i32; 2]>>> = Vec::new();
        for (row, up) in lattice.iter().zip(lattice.iter().skip(1)) {
            for ((a, b), (c, d)) in row
                .iter()
                .zip(row.iter().skip(1))
                .zip(up.iter().zip(up.iter().skip(1)))
            {
                regions.push(vec![vec![*a, *b, *d, *c, *a]]);
            }
        }
        regions.push(vec![vec![
            [-1_800_000_000, -900_000_000],
            [1_800_000_000, -900_000_000],
            [1_800_000_000, 900_000_000],
            [-1_800_000_000, 900_000_000],
        ]]);
        let body = region::encode(&regions, &Options::default()).unwrap();
        let n = regions.len() as u64;
        leak(
            pack_with(
                "regions",
                1,
                &[(Kind::REGION, Encoding::TABLE, n, body.as_slice())],
                false,
            )
            .unwrap(),
        )
    }

    // Kathmandu, inside a border leaf of the grid.
    #[library_benchmark]
    #[bench::border(region_pack())]
    fn region_at(bytes: &'static [u8]) -> u32 {
        let p = Pack::open(bytes).unwrap();
        let ix = Region::open(&p.section(Kind::REGION).unwrap()).unwrap();
        let at = Point::<E6>::from_units(27_700_000, 85_300_000)
            .unwrap()
            .to_e7();
        black_box(ix.region(black_box(at)).unwrap_or(0))
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
            blob_get,
            place_within,
            place_nearest,
            region_at
        ]
    );
}

#[cfg(target_os = "linux")]
use counts::{core_paths, format_paths};

#[cfg(target_os = "linux")]
gungraun::main!(library_benchmark_groups = core_paths, format_paths);

#[cfg(not(target_os = "linux"))]
fn main() {}
