//! Section parts (ADR-0040): a kind split into several sections, each keyed by its part. The
//! writer orders them by kind and part and refuses a part on a kind that takes none or without
//! its kind's base part; the reader refuses the same in a table it is given (each fault is
//! planted in a valid pack whose head hash is then recomputed, so the table check is what
//! fails).
#![allow(
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    reason = "tests: a panic reports a failure, and indexes are into packs built here"
)]

use teigeodata_format::{Encoding, Error, Kind, Pack, Part, Writer};

const ENTRY: usize = 80;
const TABLE: usize = 128;

fn ne() -> Part {
    Part::language("ne-Deva").unwrap_or_else(|| panic!("ne packs"))
}

/// A pack of PLACES, NAMES in two parts and SEARCH, with a NAMES part of `ne_len` bytes.
fn pack(ne_len: usize) -> Vec<u8> {
    let ne_body: Vec<u8> = (0..ne_len).map(|i| (i % 251) as u8).collect();
    let w = Writer::new("np", "places", "2026.11.0").unwrap_or_else(|e| panic!("{e}"));
    w.pack_parts(
        &[
            (Kind::SEARCH, Part::BASE, Encoding::RAW, 1, b"keys"),
            (Kind::NAMES, ne(), Encoding::RAW, 2, &ne_body),
            (Kind::PLACES, Part::BASE, Encoding::RAW, 2, b"rows"),
            (Kind::NAMES, Part::BASE, Encoding::RAW, 2, b"canonical"),
        ],
        None,
    )
    .unwrap_or_else(|e| panic!("{e}"))
}

/// Recomputes the head hash in the header and the footer after a table edit.
fn rehash(b: &mut [u8]) {
    let count = u32::from_le_bytes(b[56..60].try_into().unwrap_or_default()) as usize;
    let manifest_len = u32::from_le_bytes(b[60..64].try_into().unwrap_or_default()) as usize;
    let end = TABLE + count * ENTRY + manifest_len;
    let mut h = blake3::Hasher::new();
    h.update(&b[..112]);
    h.update(&b[TABLE..end]);
    let d = *h.finalize().as_bytes();
    b[112..128].copy_from_slice(&d[..16]);
    let n = b.len();
    b[n - 16..].copy_from_slice(&d[..16]);
}

/// Sets entry `i`'s part (bytes 72–75 of the entry) and rehashes.
fn set_part(b: &[u8], i: usize, part: u32) -> Vec<u8> {
    let mut b = b.to_vec();
    let at = TABLE + i * ENTRY + 72;
    b[at..at + 4].copy_from_slice(&part.to_le_bytes());
    rehash(&mut b);
    b
}

#[test]
fn parts_are_written_in_kind_and_part_order_and_read_back() {
    let bytes = pack(40);
    let p = Pack::open_verified(&bytes).unwrap_or_else(|e| panic!("{e}"));
    let keys: Vec<(Kind, Part)> = p.sections().map(|s| (s.kind(), s.part())).collect();
    assert_eq!(
        keys,
        [
            (Kind::PLACES, Part::BASE),
            (Kind::NAMES, Part::BASE),
            (Kind::NAMES, ne()),
            (Kind::SEARCH, Part::BASE),
        ]
    );
    let base = p.section(Kind::NAMES).unwrap_or_else(|| panic!("base"));
    assert_eq!((base.part(), base.bytes()), (Part::BASE, &b"canonical"[..]));
    assert_eq!(p.part(Kind::NAMES, ne()).map(|s| s.bytes().len()), Some(40));
    assert_eq!(p.parts(Kind::NAMES).count(), 2);
    assert_eq!(p.parts(Kind::SEARCH).count(), 1);
    assert_eq!(p.parts(Kind::ELEVATION).count(), 0);
    assert!(p.part(Kind::NAMES, Part::REST).is_none());
    let head = p.head();
    assert_eq!(
        head.parts(Kind::NAMES).map(|e| e.part).collect::<Vec<_>>(),
        [Part::BASE, ne()]
    );
}

#[test]
fn a_part_of_1_mib_or_more_has_its_own_block_hashes() {
    let bytes = pack(1 << 20);
    let p = Pack::open_verified(&bytes).unwrap_or_else(|e| panic!("{e}"));
    let blocks = p
        .part_blocks(Kind::NAMES, ne())
        .unwrap_or_else(|e| panic!("{e}"))
        .unwrap_or_else(|| panic!("covered"));
    assert_eq!(blocks.len(), 64);
    assert!(blocks.verify(63).is_ok());
    // The base part is small: not covered.
    assert!(matches!(p.blocks(Kind::NAMES), Ok(None)));
}

#[test]
fn the_language_key_packs_the_primary_subtag() {
    assert_eq!(Part::language("ne"), Part::language("ne-Deva"));
    assert_eq!(Part::language("NE").map(|p| p.0), Some(0x656e));
    assert_eq!(Part::language("mai").map(|p| p.0), Some(0x0069_616d));
    assert_eq!(Part::language("x"), None);
    assert_eq!(Part::language("toolong"), None);
    assert_eq!(Part::language("n3"), None);
    assert_eq!(Part::outline(3, 2).0, 0x0203);
}

#[test]
fn the_writer_refuses_bad_parts() {
    let w = || Writer::new("np", "places", "2026.11.0").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        w().part_bytes(Kind::PLACES, Part(1), Encoding::RAW, 1, b"x")
            .err(),
        Some(Error::Write("a part of a kind that takes none"))
    );
    let mut lone = w();
    lone.part_bytes(Kind::NAMES, ne(), Encoding::RAW, 1, b"x")
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        lone.layout(false).err(),
        Some(Error::Write("a part comes without its kind's base part"))
    );
    let mut twice = w();
    twice
        .part_bytes(Kind::NAMES, ne(), Encoding::RAW, 1, b"x")
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        twice
            .part_bytes(Kind::NAMES, ne(), Encoding::RAW, 1, b"y")
            .is_err()
    );
}

#[test]
fn the_reader_refuses_bad_tables() {
    let good = pack(40);
    let problem = |b: &[u8]| match Pack::open(b) {
        Err(Error::Section { index, problem }) => (index, problem),
        other => panic!("{other:?}"),
    };
    // The control: rehashing alone leaves a valid pack.
    let mut same = good.clone();
    rehash(&mut same);
    assert!(Pack::open(&same).is_ok());
    // PLACES (entry 0) given a part.
    assert_eq!(
        problem(&set_part(&good, 0, 1)),
        (0, "a part of a kind that takes none")
    );
    // NAMES' base part (entry 1) moved to part 1: the kind starts without its base part.
    assert_eq!(
        problem(&set_part(&good, 1, 1)),
        (1, "a part comes without its kind's base part")
    );
    // NAMES' `ne` part (entry 2) made the base part again: a repeated key.
    assert_eq!(
        problem(&set_part(&good, 2, 0)),
        (2, "kinds and parts are not strictly increasing from kind 1")
    );
    // A reserved byte after the part.
    let mut b = good.clone();
    b[TABLE + 2 * ENTRY + 76] = 1;
    rehash(&mut b);
    assert_eq!(problem(&b), (2, "reserved flags or bytes are not zero"));
}
