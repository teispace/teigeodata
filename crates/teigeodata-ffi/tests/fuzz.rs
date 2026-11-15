//! The C ABI's fuzz harness on seeded inputs (D-331). libFuzzer runs the same function nightly
//! (`fuzz/fuzz_targets/ffi.rs`) on the same seeds. `TG_FUZZ_ITERS` sets the number of inputs.

#![allow(
    clippy::expect_used,
    reason = "a test: a failure panics with its message"
)]

use teigeodata_ffi::fuzz;

/// The bindings' fixture packs, opened together as one engine's layers.
const SEEDS: [&str; 4] = ["synth", "outline", "dem", "tz"];

fn seeds() -> Vec<Vec<u8>> {
    SEEDS
        .iter()
        .map(|n| {
            let path = format!(
                "{}/../../bindings/fixtures/{n}.tgd",
                env!("CARGO_MANIFEST_DIR")
            );
            std::fs::read(&path).expect(&path)
        })
        .collect()
}

#[test]
fn every_engine_call_survives_fuzzed_arguments() {
    let owned = seeds();
    let seeds: Vec<&[u8]> = owned.iter().map(Vec::as_slice).collect();
    let n: u64 = std::env::var("TG_FUZZ_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(400);
    let mut s: u64 = 0xabc;
    let mut buf = Vec::new();
    for _ in 0..n {
        buf.clear();
        for _ in 0..64 {
            s = s
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            buf.extend_from_slice(&s.to_le_bytes());
        }
        let len = usize::try_from(s % 512).unwrap_or(0);
        buf.truncate(len);
        fuzz::abi(&seeds, &buf);
    }
}

/// Every `tg_engine_*` function the ABI exports is called by the harness, so a function the IDL
/// gains is fuzzed from the day it lands.
#[test]
fn the_harness_calls_every_engine_function() {
    let generated = include_str!("../src/generated.rs");
    let harness = include_str!("../src/fuzz.rs");
    let missing: Vec<&str> = generated
        .split("pub unsafe extern \"C\" fn ")
        .skip(1)
        .filter_map(|s| s.split('(').next())
        .filter(|f| f.starts_with("tg_engine_"))
        .filter(|f| !harness.contains(&format!("{f}(")))
        .collect();
    assert!(
        missing.is_empty(),
        "the fuzz harness does not call {missing:?}"
    );
}
