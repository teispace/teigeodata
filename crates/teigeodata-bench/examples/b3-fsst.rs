//! B3: our FSST trainer and decoder on the `GeoNames` Nepal names (`name`, `asciiname`,
//! `alternatenames`, deduplicated per place, file order): the compressed text against the raw
//! text, one table against one per script group, the training time, and the decode rate.
#![allow(
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::indexing_slicing,
    reason = "a report printed for the results page; sizes far below 2^52; buffers sized here"
)]

use std::hint::black_box;
use std::time::Instant;

use teigeodata_bench::{Rng, geonames_path};
use teigeodata_core::fsst::Symbols;
use teigeodata_core::fsst::build::{Encoder, train};

/// 0 Latin (and other ASCII-range text), 1 Devanagari, 2 anything else: by the first letter.
fn script_group(s: &str) -> usize {
    match s.chars().find(|c| c.is_alphabetic()) {
        Some(c) if ('\u{0900}'..='\u{097f}').contains(&c) => 1,
        Some(c) if c.is_ascii() || ('\u{00c0}'..='\u{024f}').contains(&c) => 0,
        Some(_) => 2,
        None => 0,
    }
}

fn names() -> Option<Vec<String>> {
    let text = std::fs::read_to_string(geonames_path()).ok()?;
    let mut out = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        let (Some(name), Some(ascii), Some(alt)) = (cols.get(1), cols.get(2), cols.get(3)) else {
            continue;
        };
        let mut place: Vec<&str> = Vec::new();
        for n in [*name, *ascii].into_iter().chain(alt.split(',')) {
            if !n.is_empty() && !place.contains(&n) {
                place.push(n);
            }
        }
        out.extend(place.into_iter().map(String::from));
    }
    Some(out)
}

/// Bytes of the codes and of the table.
fn compressed(enc: &Encoder, strings: &[&String]) -> (usize, usize) {
    let mut codes = Vec::new();
    for s in strings {
        enc.encode(s.as_bytes(), &mut codes);
    }
    (codes.len(), enc.to_bytes().len())
}

fn main() {
    let Some(all) = names() else {
        println!("missing {}", geonames_path().display());
        return;
    };
    let raw: usize = all.iter().map(String::len).sum();
    println!("{} names, {raw} B of UTF-8\n", all.len());

    let t0 = Instant::now();
    let one = train(&all);
    let train_ms = t0.elapsed().as_secs_f64() * 1e3;
    let refs: Vec<&String> = all.iter().collect();
    let (text, table) = compressed(&one, &refs);
    println!("| tables | text B | tables B | ratio | train |");
    println!("|---|---:|---:|---:|---:|");
    println!(
        "| one | {text} | {table} | {:.3} | {train_ms:.0} ms |",
        (text + table) as f64 / raw as f64
    );

    // Per script group, each name with the smaller of its group's table and the shared one.
    let mut groups: [Vec<&String>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for s in &all {
        groups[script_group(s)].push(s);
    }
    let t0 = Instant::now();
    let tables: Vec<Encoder> = groups.iter().map(|g| train(g)).collect();
    let train_ms = t0.elapsed().as_secs_f64() * 1e3;
    let mut text = 0;
    for (g, group) in groups.iter().enumerate() {
        let (mut own, mut shared, mut best) = (0, 0, 0);
        let gr: usize = group.iter().map(|s| s.len()).sum();
        for s in group {
            let (a, b) = (
                tables[g].encoded_len(s.as_bytes()),
                one.encoded_len(s.as_bytes()),
            );
            (own, shared, best) = (own + a, shared + b, best + a.min(b));
        }
        println!(
            "|   group {g}: {} names, {gr} B | own table {:.3} | shared {:.3} | best {:.3} |",
            group.len(),
            own as f64 / gr.max(1) as f64,
            shared as f64 / gr.max(1) as f64,
            best as f64 / gr.max(1) as f64
        );
        text += best;
    }
    let table: usize =
        tables.iter().map(|t| t.to_bytes().len()).sum::<usize>() + one.to_bytes().len();
    println!(
        "| per script + shared | {text} | {table} | {:.3} | {train_ms:.0} ms |",
        (text + table) as f64 / raw as f64
    );

    // Decode: every name in order, and random single names.
    let bytes = one.to_bytes();
    let Ok(sym) = Symbols::parse(&bytes) else {
        return;
    };
    let mut heap = Vec::new();
    let mut ends = Vec::with_capacity(all.len());
    for s in &all {
        one.encode(s.as_bytes(), &mut heap);
        ends.push(heap.len());
    }
    let mut out = vec![0_u8; 8 * 1024];
    let span = |i: usize| (if i == 0 { 0 } else { ends[i - 1] })..ends[i];
    let mut check = 0;
    for (i, s) in all.iter().enumerate() {
        let n = sym.decode(&heap[span(i)], &mut out).unwrap_or(0);
        assert_eq!(&out[..n], s.as_bytes(), "round trip");
        check += n;
    }
    assert_eq!(check, raw);
    let rounds = 20;
    let t0 = Instant::now();
    for _ in 0..rounds {
        for i in 0..all.len() {
            check =
                check.wrapping_add(sym.decode(black_box(&heap[span(i)]), &mut out).unwrap_or(0));
        }
    }
    let secs = t0.elapsed().as_secs_f64();
    let mut rng = Rng::new(3);
    let probes: Vec<usize> = (0..1_000_000)
        .map(|_| (rng.next_u64() % all.len() as u64) as usize)
        .collect();
    let t0 = Instant::now();
    for &i in &probes {
        check = check.wrapping_add(sym.decode(black_box(&heap[span(i)]), &mut out).unwrap_or(0));
    }
    let per = t0.elapsed().as_secs_f64() * 1e9 / probes.len() as f64;
    println!(
        "\ndecode: all names {:.0} MB/s; one random name {per:.1} ns ({})",
        (raw * rounds) as f64 / secs / 1e6,
        check % 2
    );
}
