//! B1 sizes: bytes per value of every codec on the `GeoNames` Nepal columns (Hilbert order).
#![allow(clippy::print_stdout, reason = "a report printed for the results page")]

use bitpacking::{BitPacker, BitPacker4x};
use teigeodata_bench::{geonames_columns, geonames_path, leb128_delta};
use teigeodata_core::bitpack::{Coding, encode};

fn rival_bytes(values: &[u32]) -> usize {
    let bp = BitPacker4x::new();
    let mut buf = [0_u32; 128];
    let mut out = vec![0; 1024];
    values
        .chunks(128)
        .map(|chunk| {
            let min = chunk.iter().copied().min().unwrap_or(0);
            buf.fill(0);
            for (b, v) in buf.iter_mut().zip(chunk) {
                *b = v - min;
            }
            // Per block: a 4-byte base and a 1-byte width, as ours stores.
            bp.compress(&buf, &mut out, bp.num_bits(&buf)) + 5
        })
        .sum()
}

fn main() {
    let path = geonames_path();
    let Some(cols) = geonames_columns(&path) else {
        println!("missing {}", path.display());
        return;
    };
    let n = cols.lat.len();
    println!("{n} places from {}\n", path.display());
    println!("| column | ours FOR | ours delta | LEB128 delta | bitpacking 4x FOR |");
    println!("|---|---:|---:|---:|---:|");
    #[allow(clippy::cast_precision_loss, reason = "byte counts far below 2^52")]
    let per = |b: usize| b as f64 / n as f64;
    for (name, v) in [
        ("lat", &cols.lat),
        ("lon", &cols.lon),
        ("elevation", &cols.elevation),
        ("ids (sorted)", &cols.ids),
    ] {
        let ours = [Coding::For, Coding::Delta].map(|c| per(encode(v, c).len()));
        println!(
            "| {name} | {:.3} | {:.3} | {:.3} | {:.3} |",
            ours[0],
            ours[1],
            per(leb128_delta(v).len()),
            per(rival_bytes(v))
        );
    }
}
