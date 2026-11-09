//! Runs every harness of `teigeodata_sync::fuzz` on seeded random inputs (D-072). libFuzzer
//! runs the same harnesses nightly (`fuzz/`). `TG_FUZZ_ITERS` sets the number of inputs per
//! harness.

use teigeodata_sync::fuzz;

/// `TG_FUZZ_ITERS`, else `default`, or 1 under Miri.
fn iterations(default: u64) -> u64 {
    std::env::var("TG_FUZZ_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(if cfg!(miri) { 1 } else { default })
}

/// Seeded inputs of 0 to 2,047 random bytes (255 under Miri).
fn drive(seed: u64, n: u64, harness: fn(&[u8])) {
    let max: u64 = if cfg!(miri) { 256 } else { 2048 };
    let mut s = seed;
    let mut next = || {
        s = s.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = s;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    };
    let mut buf = Vec::new();
    for _ in 0..n {
        let len = usize::try_from(next() % max).unwrap_or(0);
        buf.clear();
        while buf.len() < len {
            buf.extend_from_slice(&next().to_le_bytes());
        }
        buf.truncate(len);
        harness(&buf);
    }
}

#[test]
fn seek_tables_layouts_and_plans() {
    drive(21, iterations(20_000), fuzz::table);
}

#[test]
fn attacked_assets() {
    drive(22, iterations(1_000), fuzz::asset);
}

#[test]
fn frames() {
    drive(23, iterations(10_000), fuzz::frame);
}
