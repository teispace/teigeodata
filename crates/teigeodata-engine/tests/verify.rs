//! Lazy verification (D-078) and mapped files (D-113): a corrupt section fails the first query
//! that reads it and only those; a large section is checked a 16 KiB block at a time; a
//! truncated file is refused at open; a replaced file is noticed and the old mapping still
//! answers.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "tests: failures panic with their message"
)]

mod common;

use std::fs;
use std::path::PathBuf;

use common::synthetic;
use teigeodata_core::{E6, Point};
use teigeodata_engine::{Engine, Error, Kind, Layer, LayerOptions, Load, Scratch, Verify};
use teigeodata_format::{Encoding, Pack, Writer};

fn opts(verify: Verify) -> LayerOptions {
    LayerOptions {
        verify,
        ..LayerOptions::default()
    }
}

/// The pack with one byte of section `kind` flipped, at `offset` into it.
fn flipped(bytes: &[u8], kind: Kind, offset: usize) -> Vec<u8> {
    let pack = Pack::open(bytes).unwrap();
    let s = pack.section(kind).unwrap();
    let start = s.bytes().as_ptr() as usize - bytes.as_ptr() as usize;
    let mut out = bytes.to_vec();
    out[start + offset] ^= 0x01;
    out
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("teigeodata-engine-{name}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_corrupt_section_fails_only_the_queries_that_read_it() {
    let model = synthetic(400, 5);
    let good = model.encode();
    let s_len = Pack::open(&good)
        .unwrap()
        .section(Kind::SEARCH)
        .unwrap()
        .bytes()
        .len();
    // A byte in the middle of SEARCH: the structure still opens, the hash does not match.
    let bad = flipped(&good, Kind::SEARCH, s_len - 9);
    let mut e = Engine::new();
    e.add(Layer::from_bytes(bad.clone(), opts(Verify::Lazy)).unwrap());
    let (mut s, mut hits, mut near) = (Scratch::default(), Vec::new(), Vec::new());
    let id = model.ids[3];
    assert_eq!(e.get(id).unwrap().unwrap().id, id);
    let at = Point::<E6>::from_degrees(28.0, 84.0).unwrap();
    e.reverse(at, 3, &mut s, &mut near).unwrap();
    assert_eq!(near.len(), 3);
    for _ in 0..2 {
        // Every time, not only the first.
        let err = e.search("kama", 5, &mut s, &mut hits).unwrap_err();
        assert!(
            matches!(&err, Error::Integrity { kind, .. } if *kind == Kind::SEARCH),
            "{err}"
        );
    }
    // Eager verification refuses the pack at open; trusted bytes are not checked.
    assert!(matches!(
        Layer::from_bytes(bad.clone(), opts(Verify::Eager)),
        Err(Error::Format(teigeodata_format::Error::SectionHash { .. }))
    ));
    let mut t = Engine::new();
    t.add(Layer::from_bytes(bad, opts(Verify::Trusted)).unwrap());
    assert!(t.search("kama", 5, &mut s, &mut hits).is_ok());
    // The good pack passes all three ways.
    for v in [Verify::Lazy, Verify::Eager, Verify::Trusted] {
        let mut g = Engine::new();
        g.add(Layer::from_bytes(good.clone(), opts(v)).unwrap());
        g.search("kama", 5, &mut s, &mut hits).unwrap();
    }
}

#[test]
fn a_large_section_is_checked_a_block_at_a_time() {
    // A 2.5 MiB section is covered by BLOCK_HASHES (D-078); corrupt block 70 of 160.
    let blob: Vec<u8> = (0..(5 << 19) as u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    let w = Writer::new("np", "blobs", "2026.11.0").unwrap();
    let good = w
        .pack(&[(Kind::BLOBS, Encoding::RAW, 1, &blob)], None)
        .unwrap();
    let bad = flipped(&good, Kind::BLOBS, 70 * 16_384 + 5);
    let layer = Layer::from_bytes(bad, opts(Verify::Lazy)).unwrap();
    let block = 16_384;
    // Blocks before and after the bad one read; any range touching it fails, every time.
    assert_eq!(
        layer.section_bytes(Kind::BLOBS, 0..block).unwrap(),
        &blob[..block]
    );
    let tail = 100 * block..blob.len();
    assert_eq!(
        layer.section_bytes(Kind::BLOBS, tail.clone()).unwrap(),
        &blob[tail]
    );
    for range in [
        70 * block..70 * block + 1,
        69 * block..71 * block,
        0..blob.len(),
    ] {
        assert!(matches!(
            layer.section_bytes(Kind::BLOBS, range),
            Err(Error::Integrity { .. })
        ));
    }
    assert!(matches!(
        layer.section_bytes(Kind::BLOBS, 0..blob.len() + 1),
        Err(Error::Range)
    ));
    assert!(matches!(
        layer.section_bytes(Kind::NAMES, 0..1),
        Err(Error::Range)
    ));
    // A flipped block hash: the list no longer proves (D-142). Lazy opens in O(head) and fails
    // every read of the section, even of a good block; Eager fails at open.
    let list = Pack::open(&good)
        .unwrap()
        .section(Kind::BLOCK_HASHES)
        .unwrap()
        .bytes()
        .len();
    let bad_list = flipped(&good, Kind::BLOCK_HASHES, list - 1);
    let layer = Layer::from_bytes(bad_list.clone(), opts(Verify::Lazy)).unwrap();
    for range in [0..1, 100 * block..100 * block + 7, 0..blob.len()] {
        for _ in 0..2 {
            assert!(matches!(
                layer.section_bytes(Kind::BLOBS, range.clone()),
                Err(Error::Integrity { .. })
            ));
        }
    }
    assert!(Layer::from_bytes(bad_list, opts(Verify::Eager)).is_err());
    // The good pack proves on first use and reads every block.
    let layer = Layer::from_bytes(good, opts(Verify::Lazy)).unwrap();
    assert_eq!(
        layer.section_bytes(Kind::BLOBS, 0..blob.len()).unwrap(),
        &blob[..]
    );
}

#[test]
fn mapped_files_truncated_or_replaced() {
    let dir = scratch_dir("map");
    let model = synthetic(300, 8);
    let bytes = model.encode();
    let path = dir.join("np.places.2026.11.0.tgd");
    fs::write(&path, &bytes).unwrap();

    for load in [Load::Map, Load::Read] {
        let layer = Layer::open(
            &path,
            LayerOptions {
                load,
                ..LayerOptions::default()
            },
        )
        .unwrap();
        assert_eq!(layer.is_mapped(), load == Load::Map);
        assert_eq!(layer.len(), bytes.len());
        assert!(layer.unchanged());
    }

    // Truncated before opening: refused by the length checks, never read past the end.
    let short = dir.join("short.tgd");
    fs::write(&short, &bytes[..bytes.len() - 100]).unwrap();
    assert!(matches!(
        Layer::open(&short, LayerOptions::default()),
        Err(Error::Format(_))
    ));
    fs::write(&short, b"").unwrap();
    assert!(Layer::open(&short, LayerOptions::default()).is_err());
    assert!(matches!(
        Layer::open(dir.join("missing.tgd"), LayerOptions::default()),
        Err(Error::Io(_))
    ));

    // Replaced by rename (the install rule): the old mapping keeps the old file and answers;
    // `unchanged` tells the caller to reopen.
    let mut e = Engine::new();
    e.add(Layer::open(&path, LayerOptions::default()).unwrap());
    let other = synthetic(50, 9).encode();
    let tmp = dir.join("np.tmp");
    fs::write(&tmp, &other).unwrap();
    fs::rename(&tmp, &path).unwrap();
    let layer = e.layers().next().unwrap();
    assert!(!layer.unchanged());
    let id = model.ids[7];
    assert_eq!(e.get(id).unwrap().unwrap().id, id);
    // Removed: the mapping still holds the inode.
    fs::remove_file(&path).unwrap();
    assert_eq!(e.get(id).unwrap().unwrap().id, id);
    assert!(!e.layers().next().unwrap().unchanged());
    fs::remove_dir_all(&dir).unwrap();
}

/// ELEVATION is one height per PLACES row (D-133): a layer whose ELEVATION has another row
/// count is refused at open, never answered with another row's height.
#[test]
fn heights_that_do_not_match_the_places_are_refused() {
    let model = synthetic(300, 8);
    let good = model.encode();
    let pack = Pack::open(&good).unwrap();
    let short = teigeodata_format::elevation::encode(&model.elevations[1..])
        .unwrap()
        .unwrap();
    let rebuilt = |heights: &[u8], items: u64| {
        let mut sections: Vec<(Kind, Encoding, u64, &[u8])> = pack
            .sections()
            .filter(|s| s.kind() != Kind::ELEVATION)
            .map(|s| (s.kind(), Encoding::TABLE, s.item_count(), s.bytes()))
            .collect();
        sections.push((Kind::ELEVATION, Encoding::TABLE, items, heights));
        let m = pack.manifest();
        let w = Writer::new(m.address(), m.component(), m.release()).unwrap();
        w.pack(&sections, None).unwrap()
    };
    let same = pack.section(Kind::ELEVATION).unwrap();
    let ok = rebuilt(same.bytes(), same.item_count());
    assert!(Layer::from_bytes(ok, opts(Verify::Eager)).is_ok());
    let bad = rebuilt(&short, model.elevations.len() as u64 - 1);
    assert!(matches!(
        Layer::from_bytes(bad, opts(Verify::Eager)),
        Err(Error::Format(teigeodata_format::Error::Payload {
            kind: 19,
            ..
        }))
    ));
}
