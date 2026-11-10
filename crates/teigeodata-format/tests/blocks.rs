//! `BLOCK_HASHES` through the writer and the reader: lazy verification of a large section, the
//! writer's coverage rule and content proof, the reader's structure checks on hand-assembled
//! packs, and a flipped byte in any block found by that block alone.
#![allow(
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    reason = "tests: a failure panics with its message; buffers sized here"
)]

use core::sync::atomic::AtomicUsize;

use teigeodata_format::blocks::{self, BLOCK};
use teigeodata_format::{Encoding, Error, Kind, Pack, Writer};

const BIG: Kind = Kind::DEM;
const OTHER: Kind = Kind::ARCS;

fn data(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8 ^ seed).collect()
}

fn writer(sections: &[(Kind, &[u8])]) -> Writer {
    let mut w = Writer::new("np", "dem", "2026.11.0").unwrap();
    for &(k, b) in sections {
        w.section_bytes(k, Encoding::RAW, b.len() as u64, b)
            .unwrap();
    }
    w
}

/// A pack of `sections` with the list `blocks::encode` makes, through the writer.
fn pack(sections: &[(Kind, &[u8])]) -> Vec<u8> {
    let list = blocks::encode(sections).unwrap();
    let mut all: Vec<(Kind, &[u8])> = sections.to_vec();
    if let Some(l) = &list {
        all.push((Kind::BLOCK_HASHES, l));
    }
    let mut w = writer(sections);
    if let Some(l) = &list {
        w.section_bytes(Kind::BLOCK_HASHES, Encoding::RAW, blocks::item_count(l), l)
            .unwrap();
    }
    let layout = w.layout(false).unwrap();
    let bodies: Vec<&[u8]> = layout
        .placed()
        .iter()
        .map(|p| all.iter().find(|s| s.0 == p.kind).unwrap().1)
        .collect();
    layout.assemble(&bodies).unwrap()
}

/// Assembles by hand, past the writer's content proof: what a forger could write.
fn forged(sections: &[(Kind, &[u8])], list: &[u8], cvs: u64) -> Vec<u8> {
    let mut w = writer(sections);
    w.section_bytes(Kind::BLOCK_HASHES, Encoding::RAW, cvs, list)
        .unwrap();
    let layout = w.layout(false).unwrap();
    let mut out = layout.head().unwrap().to_vec();
    for p in layout.placed() {
        let body = if p.kind == Kind::BLOCK_HASHES {
            list
        } else {
            sections.iter().find(|s| s.0 == p.kind).unwrap().1
        };
        out.resize(p.offset as usize, 0);
        out.extend_from_slice(body);
    }
    out.extend_from_slice(layout.footer());
    out
}

#[test]
fn lazy_verification_of_a_large_section() {
    // 3 MiB and a short last block, beside a small section.
    let big = data(3 * (1 << 20) + 777, 1);
    let small = data(5000, 2);
    let bytes = pack(&[(BIG, &big), (OTHER, &small)]);
    let p = Pack::open(&bytes).unwrap();
    p.verify_sections().unwrap();
    assert!(
        p.blocks(OTHER).unwrap().is_none(),
        "small sections are verified whole"
    );
    assert!(p.blocks(Kind::NAMES).unwrap().is_none());
    let b = p.blocks(BIG).unwrap().unwrap();
    assert_eq!(b.len(), big.len().div_ceil(BLOCK));
    for i in 0..b.len() {
        assert_eq!(
            b.verify(i).unwrap(),
            &big[i * BLOCK..((i + 1) * BLOCK).min(big.len())]
        );
    }
    assert_eq!(b.verify(b.len()), Err(Error::Length));
    for r in [
        0..1,
        BLOCK - 1..BLOCK + 1,
        5..5,
        100..3 * BLOCK + 9,
        big.len() - 3..big.len(),
    ] {
        assert_eq!(b.bytes(r.clone()).unwrap(), &big[r]);
    }
    assert_eq!(b.bytes(big.len()..big.len() + 1), Err(Error::Length));
    // With a memo: the bits are set once each block is verified.
    let memo: Vec<AtomicUsize> = (0..b.memo_words()).map(|_| AtomicUsize::new(0)).collect();
    let m = b.with_memo(&memo);
    m.verify(3).unwrap();
    m.verify(3).unwrap();
    assert_eq!(memo[0].load(core::sync::atomic::Ordering::Relaxed), 1 << 3);
}

#[test]
fn a_flipped_byte_is_found_by_its_block_only() {
    let big = data(2 * (1 << 20), 3);
    let good = pack(&[(BIG, &big)]);
    let p = Pack::open(&good).unwrap();
    let start = p.section(BIG).unwrap().bytes().as_ptr() as usize - good.as_ptr() as usize;
    for at in [0, BLOCK - 1, BLOCK, 50 * BLOCK + 3, big.len() - 1] {
        let mut bad = good.clone();
        bad[start + at] ^= 0x40;
        // The head does not cover section bytes: open is fine, the list proves fine.
        let p = Pack::open(&bad).unwrap();
        let b = p.blocks(BIG).unwrap().unwrap();
        let hit = at / BLOCK;
        assert_eq!(
            b.verify(hit),
            Err(Error::SectionHash { kind: BIG.0 }),
            "{at}"
        );
        assert!(b.verify((hit + 1) % b.len()).is_ok());
        // A range that ends in the bad block, and one that starts in it.
        assert!(b.bytes(at.saturating_sub(BLOCK)..at + 1).is_err());
        assert!(b.bytes(at..(at + 2 * BLOCK).min(big.len())).is_err());
        assert!(p.verify_sections().is_err());
    }
    // A flipped byte in a block hash: the proof fails before any block is read.
    let list_start = {
        let p = Pack::open(&good).unwrap();
        p.section(Kind::BLOCK_HASHES).unwrap().bytes().as_ptr() as usize - good.as_ptr() as usize
    };
    let mut bad = good.clone();
    bad[list_start + 16 + 16 + 32 * 7 + 5] ^= 1;
    let p = Pack::open(&bad).unwrap();
    assert!(matches!(p.blocks(BIG), Err(Error::SectionHash { kind }) if kind == BIG.0));
    // Located without the proof (D-142), the same list is found, and fails when proved.
    let located = p.locate_blocks(BIG).unwrap().unwrap();
    assert!(matches!(located.prove(), Err(Error::SectionHash { kind }) if kind == BIG.0));
    let good_pack = Pack::open(&good).unwrap();
    good_pack
        .locate_blocks(BIG)
        .unwrap()
        .unwrap()
        .prove()
        .unwrap();
}

#[test]
fn the_writer_enforces_coverage_and_content() {
    let big = data(1 << 20, 4);
    let small = data(1000, 5);
    // Exactly 1 MiB is covered; one byte less is not.
    assert!(blocks::encode(&[(BIG, &big)]).unwrap().is_some());
    assert!(
        blocks::encode(&[(BIG, &big[..big.len() - 1])])
            .unwrap()
            .is_none()
    );
    assert!(Pack::open(&pack(&[(BIG, &big[..big.len() - 1])])).is_ok());
    // A large section without a list; a list with no large section.
    assert!(matches!(
        writer(&[(BIG, &big)]).layout(false),
        Err(Error::Write(_))
    ));
    let mut w = writer(&[(OTHER, &small)]);
    w.section_bytes(
        Kind::BLOCK_HASHES,
        Encoding::RAW,
        0,
        &[14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    )
    .unwrap();
    assert!(matches!(w.layout(false), Err(Error::Write(_))));
    // A list of the right shape but the wrong content: refused when assembled.
    let other = data(1 << 20, 6);
    let wrong = blocks::encode(&[(BIG, &other)]).unwrap().unwrap();
    let mut w = writer(&[(BIG, &big)]);
    w.section_bytes(Kind::BLOCK_HASHES, Encoding::RAW, 64, &wrong)
        .unwrap();
    let layout = w.layout(false).unwrap();
    let bodies: Vec<&[u8]> = layout
        .placed()
        .iter()
        .map(|p| if p.kind == BIG { &big[..] } else { &wrong[..] })
        .collect();
    assert!(matches!(layout.assemble(&bodies), Err(Error::Write(_))));
    assert!(matches!(
        blocks::encode(&[(BIG, &big), (BIG, &big)]),
        Err(Error::Write(_))
    ));
    // The right list with 32 bytes too many: refused by its length.
    let mut longer = blocks::encode(&[(BIG, &big)]).unwrap().unwrap();
    longer.extend_from_slice(&[0; 32]);
    let mut w = writer(&[(BIG, &big)]);
    w.section_bytes(Kind::BLOCK_HASHES, Encoding::RAW, 64, &longer)
        .unwrap();
    assert!(matches!(w.layout(false), Err(Error::Write(_))));
}

/// Forged counts the writer never writes: the list's item count in the section table (behind
/// the head hash, so resealed) and its own header counts.
#[test]
fn the_reader_refuses_forged_counts() {
    let big = data(1 << 20, 9);
    let good = pack(&[(BIG, &big)]);
    let p = Pack::open(&good).unwrap();
    let list = p.section(Kind::BLOCK_HASHES).unwrap();
    let list_at = list.bytes().as_ptr() as usize - good.as_ptr() as usize;
    // The table entry of the list: found by its hash, which sits 40 bytes into the entry.
    let hash_at = good.windows(32).position(|w| w == list.hash()).unwrap();
    let items_at = hash_at - 40 + 32;
    let set = |b: &mut Vec<u8>, at: usize, v: u64| b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    let refused = |b: &[u8]| matches!(Pack::open(b), Err(Error::Payload { kind: 15, .. }));
    // The item count differs from the header's count of block hashes.
    let mut b = good.clone();
    set(&mut b, items_at, 65);
    teigeodata_format::fuzz::reseal_head(&mut b);
    assert!(refused(&b));
    // Both say 65, but the list holds 64.
    set(&mut b, list_at + 8, 65);
    assert!(refused(&b));
    // Two more entries and one hash fewer: the same length, the counts agree, the coverage
    // does not.
    let mut b = good.clone();
    b[list_at + 4..list_at + 8].copy_from_slice(&3_u32.to_le_bytes());
    set(&mut b, list_at + 8, 63);
    set(&mut b, items_at, 63);
    teigeodata_format::fuzz::reseal_head(&mut b);
    assert!(refused(&b));
    // One entry more, and the list (the last section) 16 bytes longer to hold it: every
    // length agrees, and an entry covers no section.
    let mut b = good.clone();
    b[list_at + 4..list_at + 8].copy_from_slice(&2_u32.to_le_bytes());
    let mut fake = [0_u8; 16];
    fake[..4].copy_from_slice(&99_u32.to_le_bytes());
    fake[8..].copy_from_slice(&64_u64.to_le_bytes());
    b.splice(list_at + 32..list_at + 32, fake);
    let grow = |b: &mut Vec<u8>, at: usize| {
        let v = u64::from_le_bytes(b[at..at + 8].try_into().unwrap());
        b[at..at + 8].copy_from_slice(&(v + 16).to_le_bytes());
    };
    for at in [hash_at - 40 + 16, hash_at - 40 + 24, 84] {
        grow(&mut b, at);
    }
    let footer_len_at = b.len() - 24;
    grow(&mut b, footer_len_at);
    teigeodata_format::fuzz::reseal_head(&mut b);
    assert!(refused(&b), "{:?}", Pack::open(&b).err());
}

/// A list when no section is large: an empty list written as kind 14, relabelled 15.
#[test]
fn the_reader_refuses_a_list_with_nothing_to_cover() {
    let small = data(1000, 1);
    let empty_list = [14_u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut w = writer(&[(OTHER, &small)]);
    w.section_bytes(Kind::BLOBS, Encoding::RAW, 0, &empty_list)
        .unwrap();
    let layout = w.layout(false).unwrap();
    let bodies: Vec<&[u8]> = layout
        .placed()
        .iter()
        .map(|p| {
            if p.kind == OTHER {
                &small[..]
            } else {
                &empty_list[..]
            }
        })
        .collect();
    let mut b = layout.assemble(&bodies).unwrap();
    assert!(Pack::open(&b).is_ok());
    let hash = *blake3::hash(&empty_list).as_bytes();
    let kind_at = b.windows(32).position(|w| w == hash).unwrap() - 40;
    b[kind_at..kind_at + 4].copy_from_slice(&Kind::BLOCK_HASHES.0.to_le_bytes());
    teigeodata_format::fuzz::reseal_head(&mut b);
    assert!(matches!(
        Pack::open(&b),
        Err(Error::Payload { kind: 15, .. })
    ));
}

#[test]
fn the_reader_refuses_forged_lists() {
    let (a, b) = (data(1 << 20, 7), data((1 << 20) + 1, 8));
    let sections: [(Kind, &[u8]); 2] = [(BIG, &a), (OTHER, &b)];
    let good = blocks::encode(&sections).unwrap().unwrap();
    let cvs = 64 + 65;
    assert!(Pack::open(&forged(&sections, &good, cvs)).is_ok());
    // Header: block size, reserved; directory: kind order, reserved, first.
    // Entries are sorted by kind: OTHER (ARCS, 7) then BIG (DEM, 13).
    let dir = 16;
    let edits: [(usize, u8); 6] = [
        (0, 15),
        (1, 1),
        (dir, 13),
        (dir + 4, 1),
        (dir + 8, 1),
        (dir + 16 + 8, 1),
    ];
    for (at, v) in edits {
        let mut l = good.clone();
        l[at] = v;
        let bytes = forged(&sections, &l, cvs);
        assert!(
            matches!(Pack::open(&bytes), Err(Error::Payload { kind: 15, .. })),
            "byte {at} = {v}"
        );
    }
    // A CV moved within its section: the structure is fine, the proof fails.
    let mut l = good.clone();
    let first = 16 + 32;
    l[first..first + 32].copy_from_slice(&good[first + 32 * 64..first + 32 * 65]);
    let bytes = forged(&sections, &l, cvs);
    let p = Pack::open(&bytes).unwrap();
    // OTHER (kind 7) comes first, 65 blocks; its first CV now holds its last one.
    assert!(matches!(p.blocks(OTHER), Err(Error::SectionHash { kind }) if kind == OTHER.0));
    assert!(p.blocks(BIG).unwrap().is_some());
}

/// `proven_cvs` (D-167): from the head's entries and the list's bytes alone, each covered
/// section's block hashes, proven, equal the leaves of its blocks; a changed list, a wrong
/// entry and a forged list are refused.
#[test]
fn block_hashes_from_the_head_alone() {
    let big = data(2 * (1 << 20) + 5, 3);
    let other = data(1 << 20, 4);
    let small = data(900, 5);
    let bytes = pack(&[(BIG, &big), (OTHER, &other), (Kind::NAMES, &small)]);
    let p = Pack::open(&bytes).unwrap();
    let head = p.head();
    let list_entry = head.section(Kind::BLOCK_HASHES).unwrap();
    let list = p.section(Kind::BLOCK_HASHES).unwrap().bytes();
    for (kind, body) in [(BIG, &big), (OTHER, &other)] {
        let e = head.section(kind).unwrap();
        let cvs = blocks::proven_cvs(&list_entry, list, &e).unwrap().unwrap();
        assert_eq!(cvs.len(), body.len().div_ceil(BLOCK));
        for (i, chunk) in body.chunks(BLOCK).enumerate() {
            assert_eq!(cvs[i], blocks::leaf(i as u64, chunk));
        }
    }
    let small_entry = head.section(Kind::NAMES).unwrap();
    assert_eq!(
        blocks::proven_cvs(&list_entry, list, &small_entry),
        Ok(None)
    );
    // The list changed in one byte: its own hash fails.
    let e = head.section(BIG).unwrap();
    let mut changed = list.to_vec();
    let end = changed.len() - 1;
    changed[end] ^= 1;
    assert!(matches!(
        blocks::proven_cvs(&list_entry, &changed, &e),
        Err(Error::SectionHash { .. })
    ));
    // Another section's entry given as the list.
    assert!(blocks::proven_cvs(&e, list, &e).is_err());
    // A section entry whose hash is not what the list proves.
    let mut wrong = e;
    let other_hash = *head.section(OTHER).unwrap().hash;
    wrong.hash = &other_hash;
    assert!(matches!(
        blocks::proven_cvs(&list_entry, list, &wrong),
        Err(Error::SectionHash { kind }) if kind == BIG.0
    ));
}
