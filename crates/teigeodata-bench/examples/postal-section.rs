//! The POSTAL section (D-090, D-105) on real code sets: its size and each column's, and
//! the latency of an exact lookup, a 10-result prefix query, code text by ordinal and place →
//! codes.
//!
//! Sets: the Nepal pack's own section (`POSTAL_PACK`, else the `forge make np` main pack), and,
//! from the files in `POSTAL_DIR`, GeoNames India (post offices), US ZCTA 2026, GeoNames GB full
//! and the world (GeoNames `allCountries`, with the full GB, CA and NL files). As in the research
//! model, a record's place is the Hilbert rank of its row's point among the set's rows, and each
//! country is one scheme whose templates are its codes' shapes. Run in release:
//! `POSTAL_DIR=<dir> cargo run --release -p teigeodata-bench --example postal-section`.
#![allow(
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::doc_markdown,
    reason = "a report printed for the results page; inputs checked by the run"
)]

use std::collections::BTreeMap;
use std::hint::black_box;
use std::ops::ControlFlow;
use std::path::Path;
use std::time::Instant;

use teigeodata_bench::Rng;
use teigeodata_core::table::Table;
use teigeodata_core::{E6, Point, hilbert};
use teigeodata_format::postal::{self, Code, CodeKind, Postal, Rule};
use teigeodata_format::search::fst::MAX_KEY;
use teigeodata_format::{Encoding, Kind, Pack, Writer};

const PACK: &str = "data/build/pack/np/np-places-main.tgd";

fn min_ns(runs: usize, per: usize, mut f: impl FnMut()) -> f64 {
    (0..runs)
        .map(|_| {
            let t0 = Instant::now();
            f();
            t0.elapsed().as_nanos() as f64 / per as f64
        })
        .fold(f64::INFINITY, f64::min)
}

/// One code of a source file: (country, code, lat, lon).
type Row = (String, String, f64, f64);

/// GeoNames postal rows of `path` (tab-separated: country, code, …, lat, lon at 9 and 10),
/// those of `keep` countries, or all but `skip`.
fn geonames(path: &Path, keep: Option<&[&str]>, skip: &[&str]) -> Vec<Row> {
    let text = std::fs::read_to_string(path).expect("a GeoNames postal file");
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let (cc, code) = (*f.first()?, *f.get(1)?);
            if keep.is_some_and(|k| !k.contains(&cc)) || skip.contains(&cc) || code.is_empty() {
                return None;
            }
            let lat = f.get(9)?.parse().ok()?;
            let lon = f.get(10)?.parse().ok()?;
            Some((cc.to_string(), code.to_string(), lat, lon))
        })
        .collect()
}

/// The 2026 ZCTA gazetteer (`|`-separated: GEOID first, INTPTLAT and INTPTLONG last).
fn zcta(path: &Path) -> Vec<Row> {
    let text = std::fs::read_to_string(path).expect("the ZCTA gazetteer");
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split('|').collect();
            let lon = f.last()?.trim().parse().ok()?;
            let lat = f.get(f.len().checked_sub(2)?)?.trim().parse().ok()?;
            Some(("US".to_string(), (*f.first()?).to_string(), lat, lon))
        })
        .collect()
}

/// A code's shape: `9` per digit, `A` per letter, other characters as they are.
fn shape(code: &str) -> Option<String> {
    code.chars()
        .map(|c| match c {
            '0'..='9' => Some('9'),
            'A'..='Z' | 'a'..='z' => Some('A'),
            ' ' | '-' => Some(c),
            c if c.is_ascii_graphic() && !matches!(c, '?' | '\\') => Some(c),
            _ => None,
        })
        .collect()
}

/// Rules and codes for `rows`: one scheme per country with its shapes as templates (most
/// common first, at most 255), places by the Hilbert rank of their points.
fn build(rows: &[Row]) -> (Vec<Rule>, Vec<Code>, u32, usize) {
    let mut shapes: BTreeMap<&str, BTreeMap<String, usize>> = BTreeMap::new();
    for (cc, code, _, _) in rows {
        if let Some(s) = shape(code) {
            *shapes.entry(cc).or_default().entry(s).or_default() += 1;
        }
    }
    let rules: Vec<Rule> = shapes
        .iter()
        .map(|(cc, s)| {
            let mut t: Vec<(&String, &usize)> = s.iter().collect();
            t.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            Rule {
                name: cc.to_lowercase(),
                upper: true,
                ignore: " -".into(),
                templates: t.iter().take(255).map(|x| x.0.clone()).collect(),
                ..Rule::default()
            }
        })
        .collect();
    let hil = |lat: f64, lon: f64| {
        Point::<E6>::from_units((lat * 1e6) as i32, (lon * 1e6) as i32).map_or(0, hilbert)
    };
    // Each row is its own place, ranked by the Hilbert order of its point (ties by row).
    let mut order: Vec<(u64, usize)> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| (hil(r.2, r.3), i))
        .collect();
    order.sort_unstable();
    let mut rank = vec![0_u32; rows.len()];
    for (k, &(_, i)) in order.iter().enumerate() {
        if let Some(slot) = rank.get_mut(i) {
            *slot = k as u32;
        }
    }
    let places = rows.len() as u32;
    let mut seen = std::collections::HashSet::new();
    let mut dropped = 0;
    let mut codes = Vec::with_capacity(rows.len());
    for (i, (cc, code, _, _)) in rows.iter().enumerate() {
        let place = rank.get(i).copied().unwrap_or(0);
        let canon: String = code.to_uppercase().replace([' ', '-'], "");
        if shape(code).is_none() || !seen.insert((cc.clone(), canon, place)) {
            dropped += 1;
            continue;
        }
        codes.push(Code {
            scheme: cc.to_lowercase(),
            code: code.clone(),
            place,
            kind: CodeKind::PostOffice,
            valid_from: None,
            valid_to: None,
            redirect: None,
        });
    }
    (rules, codes, places, dropped)
}

fn report(name: &str, body: &[u8], records: usize, places: usize, probes: &[(String, String)]) {
    let file = Writer::new("np", "postal", "2026.11.0")
        .unwrap()
        .pack(
            &[(Kind::POSTAL, Encoding::TABLE, records as u64, body)],
            None,
        )
        .unwrap();
    let pack = Pack::open(&file).unwrap();
    let section = pack.section(Kind::POSTAL).unwrap();
    measure(name, &section, places, probes);
}

fn measure(
    name: &str,
    section: &teigeodata_format::Section<'_>,
    places: usize,
    probes: &[(String, String)],
) {
    let p = Postal::open(section, places).unwrap();
    let t0 = Instant::now();
    p.check().unwrap();
    let check_ms = t0.elapsed().as_secs_f64() * 1e3;
    let table = Table::parse(section.bytes()).unwrap();
    let cols: Vec<String> = table
        .ids()
        .map(|id| {
            let len = table.column_raw(id).map_or(0, |c| c.1.len());
            format!("{id}:{len}")
        })
        .collect();
    let bytes = section.bytes().len();
    println!(
        "{name}: {} keys, {} records, {} schemes; section {bytes} B ({:.2} B/key, {:.2} B/record); check {check_ms:.1} ms; columns (id:bytes) {}",
        p.keys(),
        p.records(),
        p.schemes().into_iter().count(),
        bytes as f64 / f64::from(p.keys().max(1)),
        bytes as f64 / p.records().max(1) as f64,
        cols.join(" ")
    );
    let mut found = 0;
    let exact = min_ns(7, probes.len(), || {
        found = 0;
        for (scheme, code) in probes {
            p.lookup(code, Some(scheme), |o| {
                found += 1;
                black_box(o);
                ControlFlow::Break(())
            });
        }
    });
    let any = min_ns(7, probes.len(), || {
        for (_, code) in probes {
            p.lookup(code, None, |o| {
                black_box(o);
                ControlFlow::Continue(())
            });
        }
    });
    let prefix = min_ns(7, probes.len(), || {
        for (scheme, code) in probes {
            let cut: String = code.chars().take(3).collect();
            let mut n = 0;
            p.prefix(&cut, Some(scheme), |o| {
                black_box(o);
                n += 1;
                if n == 10 {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            });
        }
    });
    let mut rng = Rng::new(12);
    let ords: Vec<u32> = (0..probes.len())
        .map(|_| (rng.next_u64() % u64::from(p.keys())) as u32)
        .collect();
    let mut buf = [0_u8; MAX_KEY];
    let text = min_ns(7, ords.len(), || {
        for &o in &ords {
            black_box(p.code(o, &mut buf));
        }
    });
    let rows: Vec<u32> = (0..probes.len())
        .map(|_| (rng.next_u64() % places as u64) as u32)
        .collect();
    let at = min_ns(7, rows.len(), || {
        for &r in &rows {
            p.records_at(r, |x| {
                black_box(x);
                ControlFlow::Continue(())
            });
        }
    });
    println!(
        "  exact {:.0} ns ({found} of {} found), any scheme {:.0} ns, prefix of 3 → 10 {:.0} ns, code text {:.0} ns, place → codes {:.0} ns",
        exact,
        probes.len(),
        any,
        prefix,
        text,
        at
    );
}

fn probes_of(codes: &[Code], n: usize) -> Vec<(String, String)> {
    let mut rng = Rng::new(7);
    (0..n.min(codes.len()))
        .filter_map(|_| {
            let c = codes.get((rng.next_u64() % codes.len() as u64) as usize)?;
            Some((c.scheme.clone(), c.code.clone()))
        })
        .collect()
}

fn external(name: &str, rows: &[Row]) {
    let t0 = Instant::now();
    let (rules, codes, places, dropped) = build(rows);
    let body = postal::encode(&rules, &codes, places, true).unwrap();
    println!(
        "{name}: {} rows, {} codes ({dropped} dropped as duplicates or unusable), written in {:.2} s",
        rows.len(),
        codes.len(),
        t0.elapsed().as_secs_f64()
    );
    let probes = probes_of(&codes, 20_000);
    report(name, &body, codes.len(), places as usize, &probes);
}

fn main() {
    // Nepal: the pack's own section.
    let path = std::env::var("POSTAL_PACK").unwrap_or_else(|_| PACK.to_string());
    if let Ok(bytes) = std::fs::read(&path) {
        let pack = Pack::open(&bytes).unwrap();
        let places = pack.section(Kind::PLACES).unwrap().item_count() as usize;
        let section = pack.section(Kind::POSTAL).expect("a POSTAL section");
        let p = Postal::open(&section, places).unwrap();
        let mut buf = [0_u8; MAX_KEY];
        let probes: Vec<(String, String)> = (0..p.keys())
            .filter_map(|o| {
                p.code(o, &mut buf)
                    .map(|(s, c)| (s.to_string(), c.to_string()))
            })
            .collect();
        measure(&format!("NP pack ({path})"), &section, places, &probes);
    }
    let Ok(dir) = std::env::var("POSTAL_DIR") else {
        println!("POSTAL_DIR not set: the external sets were skipped");
        return;
    };
    let dir = Path::new(&dir);
    let all = dir.join("allCountries.txt");
    external("IN (GeoNames)", &geonames(&all, Some(&["IN"]), &[]));
    external(
        "US ZCTA 2026",
        &zcta(&dir.join("2026_Gaz_zcta_national.txt")),
    );
    external(
        "GB full (GeoNames)",
        &geonames(&dir.join("GB_full.txt"), None, &[]),
    );
    let mut world = geonames(&all, None, &["GB", "CA", "NL"]);
    for f in ["GB_full.txt", "CA_full.txt", "NL_full.txt"] {
        world.extend(geonames(&dir.join(f), None, &[]));
    }
    external("World (GeoNames, full GB, CA, NL)", &world);
}
