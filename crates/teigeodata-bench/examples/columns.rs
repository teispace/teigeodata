//! Bytes per column of every table section of a pack: stored, and zstd-compressed alone at a
//! level (the wire's codec; a column compressed alone approximates its share of the frames).
//! Where to look when a pack is to shrink.
//!
//! ```text
//! cargo run --release -p teigeodata-bench --example columns -- <pack.tgd> [level] [kind…]
//! ```
#![allow(
    clippy::print_stdout,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    reason = "a measuring tool: prints its table, fails loudly"
)]

use teigeodata_core::table::Table;
use teigeodata_format::{Encoding, Pack};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: columns <pack.tgd> [level] [kind…]");
    let level: i32 = args.next().map_or(19, |l| l.parse().unwrap());
    let only: Vec<String> = args.map(|a| a.to_ascii_uppercase()).collect();
    let bytes = std::fs::read(&path).unwrap();
    let pack = Pack::open(&bytes).unwrap();
    println!("| section | part | column | codec | stored B | zstd {level} B | ratio |");
    println!("|---|---|--:|---|--:|--:|--:|");
    let (mut stored, mut wire) = (0_usize, 0_usize);
    for s in pack.sections() {
        let kind = s.kind().to_string();
        if !only.is_empty() && !only.contains(&kind) {
            continue;
        }
        if s.encoding() != Encoding::TABLE {
            continue;
        }
        let Ok(t) = Table::parse(s.bytes()) else {
            continue;
        };
        for id in t.ids() {
            let (codec, col) = t.column_raw(id).unwrap();
            let z = zstd::bulk::compress(col, level)
                .unwrap()
                .len()
                .min(col.len());
            stored += col.len();
            wire += z;
            println!(
                "| {kind} | {:?} | {id} | {codec:?} | {} | {z} | {:.2} |",
                s.part(),
                col.len(),
                z as f64 / col.len().max(1) as f64
            );
        }
    }
    println!("\ncolumns: {stored} B stored, {wire} B at zstd {level} alone");
}
