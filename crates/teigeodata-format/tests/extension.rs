//! Extension columns (D-335): a column-table section that gains a column from
//! `table::EXTENSION` (`0x8000`) up, of a codec this reader may not know, still opens and checks
//! exactly as before. This is the rule that lets a format minor add an optional column without
//! a new major: every reader of the same major skips what it does not know.
#![allow(
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a panic reports a failure, and indexes are into tables built here"
)]

use std::path::PathBuf;

use teigeodata_core::table::EXTENSION;
use teigeodata_format::{Encoding, Kind, Pack, check, fuzz};

/// The binding fixtures: between them every column-table section kind the engine reads.
const SEEDS: [&str; 4] = ["synth.tgd", "outline.tgd", "dem.tgd", "tz.tgd"];

fn seed(name: &str) -> Vec<u8> {
    let at = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../bindings/fixtures")
        .join(name);
    std::fs::read(&at).unwrap_or_else(|e| panic!("{}: {e}", at.display()))
}

/// `table` with one more column at the end: ID `id`, codec byte `codec`, 8 bytes of body.
fn with_column(table: &[u8], id: u16, codec: u8) -> Vec<u8> {
    let count = usize::from(u16::from_le_bytes([table[0], table[1]]));
    let directory_end = 4 + count * 8;
    let mut out = Vec::with_capacity(table.len() + 16);
    out.extend_from_slice(&u16::try_from(count + 1).unwrap_or(u16::MAX).to_le_bytes());
    out.extend_from_slice(&table[2..directory_end]);
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&[codec, 0]);
    out.extend_from_slice(&8_u32.to_le_bytes());
    out.extend_from_slice(&table[directory_end..]);
    out.extend_from_slice(&[0xa5; 8]);
    out
}

/// `bytes` with every column-table section given the column `with_column(id, codec)`.
fn extended(bytes: &[u8], id: u16, codec: u8) -> Vec<u8> {
    let p = Pack::open(bytes).unwrap_or_else(|e| panic!("{e}"));
    let mut sections = fuzz::bodies(&p);
    for s in &mut sections {
        if s.2 == Encoding::TABLE {
            s.5 = with_column(&s.5, id, codec);
            s.4 = s.5.len() as u64;
        }
    }
    fuzz::write_again(&p, &sections).unwrap_or_else(|| panic!("the writer refused the pack"))
}

/// Each section's kind and full-check result, in table order.
fn checks(bytes: &[u8]) -> Vec<(Kind, String)> {
    let p = Pack::open_verified(bytes).unwrap_or_else(|e| panic!("{e}"));
    let mut out = Vec::new();
    check::sections(&p, |kind, r| out.push((kind, format!("{r:?}"))));
    out
}

#[test]
fn every_section_skips_an_unknown_extension_column() {
    for name in SEEDS {
        let base = seed(name);
        let want = checks(&base);
        assert!(
            want.iter().all(|(_, r)| r.starts_with("Ok")),
            "{name}: the fixture checks clean: {want:?}"
        );
        // A codec this reader knows (bytes), and one no reader knows yet.
        for codec in [2, 200] {
            for id in [EXTENSION, u16::MAX] {
                let got = checks(&extended(&base, id, codec));
                assert_eq!(got, want, "{name}: extension {id:#x} of codec {codec}");
            }
        }
    }
}

/// The same edit below `EXTENSION` is refused by every section: the section's own columns stay
/// strict, so a column that changes meaning is never ignored.
#[test]
fn every_section_refuses_an_unknown_column_of_its_own() {
    for name in SEEDS {
        let base = seed(name);
        let got = checks(&extended(&base, EXTENSION - 1, 2));
        let tables = Pack::open(&base)
            .unwrap_or_else(|e| panic!("{e}"))
            .sections()
            .filter(|s| s.encoding() == Encoding::TABLE)
            .count();
        let refused = got.iter().filter(|(_, r)| r.starts_with("Err")).count();
        assert_eq!(refused, tables, "{name}: {got:?}");
    }
}
