//! The engine's fuzz harness on seeded inputs (D-331). libFuzzer runs the same function
//! nightly (`fuzz/fuzz_targets/engine.rs`) on the same seeds. `TG_FUZZ_ITERS` sets the number
//! of inputs per seed.

#![allow(
    clippy::expect_used,
    reason = "a test: a failure panics with its message"
)]

mod common;

use teigeodata_engine::fuzz;
use teigeodata_format::{Kind, Pack};

/// The seeds, small and checked in: the bindings' fixture packs, and `fuzz/seeds/extra.tgd`
/// with the kinds the engine reads that they lack. The libFuzzer target embeds the same files.
const SEEDS: [&str; 5] = [
    "bindings/fixtures/synth.tgd",
    "bindings/fixtures/outline.tgd",
    "bindings/fixtures/dem.tgd",
    "bindings/fixtures/tz.tgd",
    "fuzz/seeds/extra.tgd",
];

fn path(name: &str) -> String {
    format!("{}/../../{name}", env!("CARGO_MANIFEST_DIR"))
}

fn seed(name: &str) -> Vec<u8> {
    std::fs::read(path(name)).expect(name)
}

/// The extra seed, with the kinds the other seeds lack: a small synthetic country with
/// SUBSTRING, the DEM fixture's grid as `DEM_COARSE`, the outline fixture's arcs with their
/// `ARCS_OVERVIEW`, and COMPONENTS (one component on every other row, with a field).
fn extra() -> Vec<u8> {
    use teigeodata_format::arcs::{self, Arcs};
    use teigeodata_format::components::{self, ComponentColumns};
    let mut m = common::synthetic(60, 31);
    m.with_substring();
    let dem = seed("bindings/fixtures/dem.tgd");
    let dem = Pack::open(&dem).expect("the DEM fixture");
    let grid = dem.section(Kind::DEM).expect("its DEM");
    m.raw
        .push((Kind::DEM_COARSE, grid.item_count(), grid.bytes().to_vec()));
    let outline = seed("bindings/fixtures/outline.tgd");
    let outline = Pack::open(&outline).expect("the outline fixture");
    let full = outline.section(Kind::ARCS).expect("its ARCS");
    let overview = arcs::overview(&Arcs::open(&full).expect("its arcs"), 1).expect("an overview");
    m.raw
        .push((Kind::ARCS, full.item_count(), full.bytes().to_vec()));
    m.raw
        .push((Kind::ARCS_OVERVIEW, full.item_count(), overview));
    let n = m.ids.len();
    let rows: Vec<u32> = (0..u32::try_from(n).expect("rows")).step_by(2).collect();
    let values: Vec<u32> = rows.iter().map(|r| r * 3).collect();
    let fields = [(1_u8, values.as_slice())];
    let comps = [ComponentColumns {
        id: 1,
        rows: &rows,
        fields: &fields,
    }];
    m.raw.push((
        Kind::COMPONENTS,
        n as u64,
        components::encode(n, &comps).expect("COMPONENTS"),
    ));
    m.encode()
}

/// `fuzz/seeds/extra.tgd` is [`extra`], byte for byte; `TEIGEODATA_WRITE_FIXTURE=1` rewrites
/// it.
#[test]
fn the_extra_seed_is_current() {
    let file = path("fuzz/seeds/extra.tgd");
    let bytes = extra();
    if std::env::var_os("TEIGEODATA_WRITE_FIXTURE").is_some() {
        std::fs::create_dir_all(std::path::Path::new(&file).parent().expect("a folder"))
            .expect("the seeds folder");
        std::fs::write(&file, &bytes).expect("the seed");
    }
    assert!(
        std::fs::read(&file).unwrap_or_default() == bytes,
        "{file} is stale: rerun with TEIGEODATA_WRITE_FIXTURE=1"
    );
}

fn iterations(default: u64) -> u64 {
    std::env::var("TG_FUZZ_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[test]
fn edited_packs_never_panic() {
    for name in SEEDS {
        let pack = seed(name);
        let mut s: u64 = 0x5eed ^ name.len() as u64;
        let mut buf = Vec::new();
        for _ in 0..iterations(300) {
            s = s.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = s;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^= z >> 31;
            buf.clear();
            let len = usize::try_from(z % 512).unwrap_or(0);
            let mut x = z;
            while buf.len() < len {
                x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                buf.extend_from_slice(&x.to_le_bytes());
            }
            buf.truncate(len);
            fuzz::engine(&pack, &buf);
        }
    }
}

/// The seeds hold every kind the engine reads, so an edit can reach every decoder.
#[test]
fn the_seeds_hold_every_kind_the_engine_reads() {
    let mut kinds = std::collections::BTreeSet::new();
    for name in SEEDS {
        let pack = seed(name);
        let p = Pack::open(&pack).expect(name);
        kinds.extend(p.sections().map(|s| s.kind().0));
        // A seed the engine refuses, or the format's check faults, would teach the fuzzer
        // nothing: each one is whole.
        teigeodata_format::check::sections(&p, |kind, r| {
            assert!(r.is_ok(), "{name}: {kind:?} fails its check: {r:?}");
        });
        teigeodata_engine::Layer::from_bytes(
            pack.clone(),
            teigeodata_engine::LayerOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{name}: the engine refuses it: {e}"));
    }
    let missing: Vec<u32> = (1..=Kind::DEM_COARSE.0)
        .filter(|k| *k != Kind::BLOCK_HASHES.0 && !kinds.contains(k))
        .collect();
    // BLOBS is not read by queries; the format's `sections` harness covers it.
    let not_read = [Kind::BLOBS.0];
    let missing: Vec<u32> = missing
        .into_iter()
        .filter(|k| !not_read.contains(k))
        .collect();
    assert!(missing.is_empty(), "no seed holds kinds {missing:?}");
}

/// A libFuzzer input as the target reads it: the first byte picks the seed.
fn replay(input: &[u8]) {
    let (pick, rest) = input.split_first().expect("an input");
    let name = SEEDS.get(usize::from(*pick) % SEEDS.len()).expect("a seed");
    fuzz::engine(&seed(name), rest);
}

/// Inputs libFuzzer found failing, kept so they stay fixed.
#[test]
fn found_by_libfuzzer() {
    // 2026-11-15, the first run of `engine`: a DEM whose tile count is near 2^64 overflowed
    // `tiles + 1` in `Dem::open` (`columns::boundaries` now refuses it; ARCS and POLYGONS had
    // the same sum).
    // 2026-11-15: a DEM grid with a tile side of 2^200 overflowed a query's shift; `Dem::open`
    // now checks every grid, model and threshold (O(grids)), as queries rely on them.
    replay(&[7, 20, 0, 20, 0, 251, 0, 37, 1, 22, 37, 37, 37, 0]);
    replay(&[
        7, 203, 13, 0, 219, 63, 232, 232, 232, 232, 232, 232, 232, 232, 232, 232, 232, 232, 232,
        232, 232, 232, 232, 232, 232, 232, 226, 232, 232, 232, 232, 232, 232, 232, 232, 232, 232,
        232, 232, 232, 232, 232, 232, 232, 232, 232, 232, 232, 26, 10, 84,
    ]);
}
