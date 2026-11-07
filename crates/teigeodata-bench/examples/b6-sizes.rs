//! B6 sizes: bytes per row of a presence set stored as a bitmap with a rank directory against
//! the sorted list of present rows (a packed column, the best of FOR, delta and ramp), at
//! densities from 0.1% to 99%, scattered at random and in runs (optional data often comes from
//! one source covering one region, which Hilbert order keeps together).
#![allow(
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "a report printed for the results page; sizes far below 2^52; rows below 2^32"
)]

use teigeodata_bench::Rng;
use teigeodata_core::bitpack::encode_auto;
use teigeodata_core::bits;

/// Rows present with probability `p`: independently, or in runs of mean length 64.
fn rows(n: usize, p: f64, runs: bool, rng: &mut Rng) -> Vec<u32> {
    let mut unit = || (rng.next_u64() >> 11) as f64 / (1_u64 << 53) as f64;
    let mut out = Vec::new();
    let mut on = false;
    for r in 0..n {
        if runs {
            // A two-state chain with mean run 64 when on and the stationary density `p`.
            let leave_on = 1.0 / 64.0;
            let leave_off = leave_on * p / (1.0 - p).max(1e-9);
            if unit() < if on { leave_on } else { leave_off } {
                on = !on;
            }
        } else {
            on = unit() < p;
        }
        if on {
            out.push(r as u32);
        }
    }
    out
}

fn main() {
    println!("| rows | density | pattern | bitmap B/row | sorted rows B/row | smaller |");
    println!("|---:|---:|---|---:|---:|---|");
    for n in [150_000_usize, 10_000_000] {
        for p in [0.001_f64, 0.01, 0.03, 0.1, 0.5, 0.9, 0.99] {
            for runs in [false, true] {
                let mut rng = Rng::new(n as u64 ^ p.to_bits());
                let present = rows(n, p, runs, &mut rng);
                let Ok(bitmap) = bits::encode(n, present.iter().map(|&r| r as usize)) else {
                    return;
                };
                let sorted = encode_auto(&present);
                let per = |b: usize| b as f64 / n as f64;
                println!(
                    "| {n} | {:.1}% | {} | {:.4} | {:.4} | {} |",
                    present.len() as f64 * 100.0 / n as f64,
                    if runs { "runs" } else { "random" },
                    per(bitmap.len()),
                    per(sorted.len()),
                    if bitmap.len() <= sorted.len() {
                        "bitmap"
                    } else {
                        "sorted"
                    },
                );
            }
        }
    }
}
