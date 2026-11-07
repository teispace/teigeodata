//! B3: the NAMES section on the `GeoNames` Nepal names: bytes per place, and the time to read
//! one place's canonical name and all its names, with the text ends as the writer codes them
//! (delta when 10% smaller) and with O(1) codings only (FOR or ramp).
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
use teigeodata_core::bitpack::{Coding, encode};
use teigeodata_core::table::{Codec, Table, encode_with};
use teigeodata_format::names::{self, NameRecord, NameType, Names, TEXT_BUFFER, TEXT_END, Tag};
use teigeodata_format::{Encoding, Kind, Pack, Writer};

fn places() -> Option<Vec<Vec<String>>> {
    let text = std::fs::read_to_string(geonames_path()).ok()?;
    let mut out = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        let (Some(name), Some(ascii), Some(alt)) = (cols.get(1), cols.get(2), cols.get(3)) else {
            continue;
        };
        let mut place: Vec<String> = Vec::new();
        for n in [*name, *ascii].into_iter().chain(alt.split(',')) {
            if !n.is_empty() && !place.iter().any(|p| p == n) {
                place.push(n.to_owned());
            }
        }
        if !place.is_empty() {
            out.push(place);
        }
    }
    Some(out)
}

fn script(s: &str) -> Option<Tag> {
    match s.chars().find(|c| c.is_alphabetic())? {
        c if ('\u{0900}'..='\u{097f}').contains(&c) => Tag::new("Deva"),
        c if c.is_ascii() || ('\u{00c0}'..='\u{024f}').contains(&c) => Tag::new("Latn"),
        _ => None,
    }
}

/// The body with `TEXT_END` recoded: the smaller of FOR and ramp if `o1`, else as
/// `encode_auto` picks (delta when 10% smaller).
fn with_text_end(body: &[u8], o1: bool) -> Vec<u8> {
    let Ok(t) = Table::parse(body) else {
        return Vec::new();
    };
    let Ok(col) = t.column(TEXT_END) else {
        return Vec::new();
    };
    let mut v = vec![0; col.len()];
    col.decode_all(&mut v);
    let recoded = if o1 {
        let (f, r) = (encode(&v, Coding::For), encode(&v, Coding::Ramp));
        if r.len() < f.len() { r } else { f }
    } else {
        teigeodata_core::bitpack::encode_auto(&v)
    };
    let cols: Vec<(u16, Codec, &[u8])> = t
        .ids()
        .filter_map(|id| {
            if id == TEXT_END {
                Some((id, Codec::Bitpack, recoded.as_slice()))
            } else {
                t.column_raw(id).map(|(c, b)| (id, c, b))
            }
        })
        .collect();
    encode_with(&cols).unwrap_or_default()
}

fn main() {
    let Some(all) = places() else {
        println!("missing {}", geonames_path().display());
        return;
    };
    let n = all.len();
    let recs: Vec<Vec<NameRecord<'_>>> = all
        .iter()
        .map(|p| {
            p.iter()
                .enumerate()
                .map(|(i, t)| NameRecord {
                    text: t,
                    script: script(t),
                    kind: Some(if i == 0 {
                        NameType::Official
                    } else {
                        NameType::Alt
                    }),
                    ..NameRecord::default()
                })
                .collect()
        })
        .collect();
    let slices: Vec<&[NameRecord<'_>]> = recs.iter().map(Vec::as_slice).collect();
    let t0 = Instant::now();
    let Ok(body) = names::encode(&slices) else {
        return;
    };
    println!(
        "{n} places; encoded in {:.0} ms\n",
        t0.elapsed().as_secs_f64() * 1e3
    );
    println!(
        "| text ends | NAMES B | B per place | canonical name | all names of a place | check |"
    );
    println!("|---|---:|---:|---:|---:|---:|");
    let mut rng = Rng::new(11);
    let probes: Vec<usize> = (0..1_000_000)
        .map(|_| (rng.next_u64() % n as u64) as usize)
        .collect();
    for (label, o1) in [("as written (auto)", false), ("O(1) codings only", true)] {
        let b = with_text_end(&body, o1);
        let Ok(mut w) = Writer::new("np", "places", "2026.11.0") else {
            return;
        };
        if w.section_bytes(Kind::NAMES, Encoding::TABLE, n as u64, &b)
            .is_err()
        {
            return;
        }
        let Ok(bytes) = w.layout(false).and_then(|l| l.assemble(&[&b])) else {
            return;
        };
        let Ok(pack) = Pack::open(&bytes) else {
            return;
        };
        let Some(Ok(v)) = pack.section(Kind::NAMES).map(|s| Names::open(&s)) else {
            return;
        };
        let t0 = Instant::now();
        let ok = v.check().is_ok();
        let check_ms = t0.elapsed().as_secs_f64() * 1e3;
        let mut buf = [0_u8; TEXT_BUFFER];
        let mut sum = 0_usize;
        let t0 = Instant::now();
        for &r in &probes {
            sum += v.of(black_box(r)).start;
        }
        let locate = t0.elapsed().as_secs_f64() * 1e9 / probes.len() as f64;
        let t0 = Instant::now();
        for &r in &probes {
            if let Some(c) = v.canonical(black_box(r)) {
                sum += c.text(&mut buf).map_or(0, str::len);
            }
        }
        let one = t0.elapsed().as_secs_f64() * 1e9 / probes.len() as f64;
        let t0 = Instant::now();
        for &r in &probes {
            for name in v.names(black_box(r)) {
                sum += name.text(&mut buf).map_or(0, str::len);
            }
        }
        let every = t0.elapsed().as_secs_f64() * 1e9 / probes.len() as f64;
        println!(
            "| {label} | {} | {:.2} | {one:.1} ns (locate {locate:.1}) | {every:.1} ns | {check_ms:.1} ms {} |",
            b.len(),
            b.len() as f64 / n as f64,
            if ok { "ok" } else { "FAILED" },
        );
        black_box(sum);
    }
}
