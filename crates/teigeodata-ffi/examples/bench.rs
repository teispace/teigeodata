//! The binding benchmark's Rust rows: the API called directly (the floor) and through the C
//! ABI. Every language runs the same four cases, best of five rounds, ns per call
//! (docs/02-research/benches/s4-bindings.md). Run: cargo run --release -p teigeodata-ffi
//! --example bench
#![allow(clippy::print_stdout, reason = "a benchmark reports on stdout")]
#![allow(unsafe_code, reason = "the C ABI rows call the exported functions")]

use core::ffi::c_void;
use std::hint::black_box;
use std::time::Instant;

use teigeodata_blob::{Reader, View};
use teigeodata_ffi::abi::{TgBytes, tg_bytes_free};
use teigeodata_ffi::generated::{tg_fold, tg_noop, tg_progress, tg_sample_rows};

/// Best of five rounds of `n` calls, in ns per call.
fn best(n: u32, mut f: impl FnMut()) -> f64 {
    (0..5)
        .map(|_| {
            let t = Instant::now();
            for _ in 0..n {
                f();
            }
            t.elapsed().as_secs_f64() * 1e9 / f64::from(n)
        })
        .fold(f64::INFINITY, f64::min)
}

fn lat_sum(bytes: &[u8]) -> f64 {
    match Reader::new(bytes).ok().and_then(|r| r.column("lat")) {
        Some(View::F64(lat)) => lat.f64s().sum(),
        _ => f64::NAN,
    }
}

unsafe extern "C" fn yes(_: *mut c_void, _: u32) -> bool {
    true
}

fn main() {
    let row = |lang: &str, case: &str, ns: f64| println!("{lang:<10} {case:<22} {ns:>10.1} ns");
    row("rust", "noop", best(10_000_000, teigeodata_api::noop));
    row(
        "rust",
        "fold",
        best(1_000_000, || {
            black_box(teigeodata_api::fold(black_box("Kathmandu")));
        }),
    );
    row(
        "rust",
        "sample_rows(1000)+sum",
        best(10_000, || {
            let b = teigeodata_api::sample_rows(black_box(1000));
            black_box(b.map(|b| lat_sum(b.as_bytes())).ok());
        }),
    );
    let per_cb = best(1_000, || {
        black_box(teigeodata_api::progress(1000, &mut |s| black_box(s) > 0).ok());
    }) / 1000.0;
    row("rust", "callback", per_cb);

    // SAFETY (all rows below): valid pointers for each call, buffers freed once.
    row(
        "c-abi",
        "noop",
        best(10_000_000, || unsafe {
            black_box(tg_noop());
        }),
    );
    let text = "Kathmandu";
    row(
        "c-abi",
        "fold",
        best(1_000_000, || unsafe {
            let mut out = TgBytes::EMPTY;
            black_box(tg_fold(text.as_ptr(), text.len(), &raw mut out));
            tg_bytes_free(out);
        }),
    );
    row(
        "c-abi",
        "sample_rows(1000)+sum",
        best(10_000, || unsafe {
            let mut out = TgBytes::EMPTY;
            tg_sample_rows(1000, &raw mut out);
            black_box(lat_sum(core::slice::from_raw_parts(out.ptr, out.len)));
            tg_bytes_free(out);
        }),
    );
    let per_cb = best(1_000, || unsafe {
        let mut n = 0;
        black_box(tg_progress(
            1000,
            Some(yes),
            core::ptr::null_mut(),
            &raw mut n,
        ));
    }) / 1000.0;
    row("c-abi", "callback", per_cb);
}
