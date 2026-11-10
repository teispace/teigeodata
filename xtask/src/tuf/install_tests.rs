//! The installer (D-167) against what a release publishes: the catalog entry and transport asset
//! `entry_of` makes from a pack, installed by component through `sync::install`.
#![allow(
    clippy::cast_possible_truncation,
    reason = "test data: bytes from small counters, offsets into buffers sized here"
)]

use std::ops::Range;

use teigeodata_format::component::{Component, Components};
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer};
use teigeodata_sync::Error;
use teigeodata_sync::install::Install;
use teigeodata_sync::transport::{SeekTable, encode_table};

/// A pack with a 2.5 MiB NAMES (three frames, covered by `BLOCK_HASHES`), small sections of
/// other components, and an empty one.
fn pack() -> Vec<u8> {
    let ids: Vec<u8> = (0..40_000_u32).map(|i| (i * 7 % 251) as u8).collect();
    let places: Vec<u8> = (0..30_000_u32).map(|i| (i % 13) as u8).collect();
    let names: Vec<u8> = (0..(5_u32 << 19))
        .map(|i| b"Lahachowk Kathmandu Pokhara "[(i % 28) as usize] ^ (i >> 16) as u8)
        .collect();
    let tz = vec![9_u8; 3_000];
    let mut w = Writer::new("zz", "places", "2026.11.0").unwrap();
    w.manifest(Tag::LICENCE, b"odbl").unwrap();
    w.pack(
        &[
            (Kind::IDMAP, Encoding::RAW, 1, &ids),
            (Kind::PLACES, Encoding::RAW, 1, &places),
            (Kind::HIERARCHY, Encoding::RAW, 0, &[]),
            (Kind::NAMES, Encoding::RAW, 1, &names),
            (Kind::PLACE_TZ, Encoding::RAW, 1, &tz),
        ],
        None,
    )
    .unwrap()
}

fn at(r: &Range<u64>) -> Range<usize> {
    usize::try_from(r.start).unwrap()..usize::try_from(r.end).unwrap()
}

/// Installs `set` from `asset`, every write recorded; the result and the staging file.
fn install(
    entry: &teigeodata_sync::catalog::Entry,
    asset: &[u8],
    pack_len: usize,
    set: Components,
) -> (Result<teigeodata_sync::install::Installed, Error>, Vec<u8>) {
    let mut staged = vec![0_u8; pack_len];
    let mut put = |off: u64, b: &[u8]| -> Result<(), Error> {
        let o = usize::try_from(off).unwrap();
        staged[o..o + b.len()].copy_from_slice(b);
        Ok(())
    };
    let mut inst = Install::new(entry, asset.len() as u64, set);
    let run = (|| {
        let [t, h] = inst.first()?;
        let fetches = inst
            .start(&asset[at(&t)], &asset[at(&h)], &mut put)?
            .to_vec();
        for (i, f) in fetches.iter().enumerate() {
            inst.feed(i, &asset[at(&f.compressed)], &mut put)?;
        }
        inst.finish()
    })();
    (run, staged)
}

#[test]
fn the_installer_rebuilds_the_pack_from_its_catalog_entry() {
    let pack = pack();
    let (entry, asset, _) = super::entry_of(&pack).unwrap();
    let p = Pack::open(&pack).unwrap();
    let sets = [
        Components::all(),
        Components::of(&[Component::Point]),
        Components::of(&[Component::Names]),
        Components::of(&[Component::Timezone]),
        Components::of(&[Component::Admin, Component::Timezone]),
    ];
    for set in sets {
        let (done, staged) = install(&entry, &asset, pack.len(), set);
        let done = done.unwrap_or_else(|e| panic!("{set:?}: {e}"));
        // The bytes fetched are the catalog's download size for the set (D-162).
        assert_eq!(done.fetched, entry.size_of(set).wire, "{set:?}");
        // The staged file opens, and holds exactly the pack's bytes for every installed section.
        let s = Pack::open_head(&staged).unwrap();
        for sec in p.sections() {
            if set.closed().holds(sec.kind()) {
                let got = s.section(sec.kind()).unwrap();
                assert_eq!(got.bytes(), sec.bytes(), "{set:?} {:?}", sec.kind());
                got.verify().unwrap();
            }
        }
        if set == Components::all() {
            assert!(staged == pack, "the whole pack, byte for byte");
            assert_eq!(done.written, pack.len() as u64);
        }
    }
}

/// Rebuilds `asset` with frame `i`'s decoded bytes changed by `edit` and recompressed validly,
/// so only the hashes can tell.
fn forge_frame(asset: &[u8], i: usize, edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let tail = SeekTable::tail_len(asset).unwrap();
    let table = SeekTable::parse(&asset[asset.len() - tail..]).unwrap();
    let mut entries: Vec<(u32, u32)> = table
        .entries()
        .map(|e| (e.compressed, e.decompressed))
        .collect();
    let start: usize = entries[..i].iter().map(|e| e.0 as usize).sum();
    let end = start + entries[i].0 as usize;
    let mut decoded = zstd::bulk::decompress(&asset[start..end], entries[i].1 as usize).unwrap();
    edit(&mut decoded);
    let frame = zstd::bulk::compress(&decoded, 19).unwrap();
    entries[i].0 = u32::try_from(frame.len()).unwrap();
    let mut out = asset[..start].to_vec();
    out.extend_from_slice(&frame);
    out.extend_from_slice(&asset[end..asset.len() - tail]);
    encode_table(entries.into_iter(), &mut out).unwrap();
    out
}

#[test]
fn nothing_unchecked_is_written() {
    let pack = pack();
    let (entry, asset, _) = super::entry_of(&pack).unwrap();
    let all = Components::all();
    let tail = SeekTable::tail_len(&asset).unwrap();
    let frames = SeekTable::parse(&asset[asset.len() - tail..])
        .unwrap()
        .entries()
        .len();
    // Each frame, with one byte changed and validly recompressed: the install fails, and the
    // staging file never holds the changed byte. (The asset's length changes with the frame,
    // as a host serving other bytes would; the catalog entry stays the signed one.)
    for i in 0..frames {
        let forged = forge_frame(&asset, i, |d| {
            let mid = d.len() / 2;
            d[mid] ^= 0x40;
        });
        let mut e = entry.clone();
        e.head_frame.wire = u64::from(
            SeekTable::parse(&forged[forged.len() - tail..])
                .unwrap()
                .entry(0)
                .unwrap()
                .compressed,
        );
        let (done, staged) = install(&e, &forged, pack.len(), all);
        assert!(done.is_err(), "frame {i}");
        let wrong = staged
            .iter()
            .zip(&pack)
            .position(|(a, b)| a != b && *a != 0);
        assert_eq!(wrong, None, "frame {i}: a changed byte was written");
    }
    // A padding byte after each section that has padding, and the footer's last byte.
    let starts: Vec<u64> = SeekTable::parse(&asset[asset.len() - tail..])
        .unwrap()
        .entries()
        .scan(0_u64, |at, e| {
            let s = *at;
            *at += u64::from(e.decompressed);
            Some(s)
        })
        .collect();
    let p = Pack::open(&pack).unwrap();
    let mut targets: Vec<u64> = p
        .head()
        .sections()
        .map(|s| s.offset + s.length)
        .filter(|end| end % 64 != 0)
        .collect();
    targets.push(pack.len() as u64 - 1);
    assert!(targets.len() >= 3);
    for t in targets {
        let i = starts.iter().rposition(|&s| s <= t).unwrap();
        let k = usize::try_from(t - starts[i]).unwrap();
        let forged = forge_frame(&asset, i, |d| d[k] ^= 1);
        let mut e = entry.clone();
        e.head_frame.wire = u64::from(
            SeekTable::parse(&forged[forged.len() - tail..])
                .unwrap()
                .entry(0)
                .unwrap()
                .compressed,
        );
        let (done, _) = install(&e, &forged, pack.len(), all);
        assert!(done.is_err(), "byte {t} of frame {i}");
    }
    // A catalog entry for another pack: refused at the head.
    let mut other = entry.clone();
    other.head[0] ^= 1;
    let (done, staged) = install(&other, &asset, pack.len(), all);
    assert_eq!(
        done,
        Err(Error::Install("the head hash is not the catalog's"))
    );
    assert!(staged.iter().all(|&b| b == 0), "nothing written");
    let mut renamed = entry.clone();
    renamed.release = "2026.12.0".into();
    let (done, _) = install(&renamed, &asset, pack.len(), all);
    assert!(done.is_err());
}

#[test]
fn the_caller_is_held_to_the_order() {
    let pack = pack();
    let (entry, asset, _) = super::entry_of(&pack).unwrap();
    let mut put = |_: u64, _: &[u8]| -> Result<(), Error> { Ok(()) };
    let mut inst = Install::new(&entry, asset.len() as u64, Components::all());
    assert!(inst.feed(0, &[], &mut put).is_err(), "fed before start");
    assert!(inst.finish().is_err(), "never started");
    let [t, h] = inst.first().unwrap();
    // A host that returns a short head frame.
    let short = &asset[at(&h)][1..];
    assert!(inst.start(&asset[at(&t)], short, &mut put).is_err());
    let mut inst = Install::new(&entry, asset.len() as u64, Components::all());
    let fetches = inst
        .start(&asset[at(&t)], &asset[at(&h)], &mut put)
        .unwrap()
        .to_vec();
    // BLOCK_HASHES first, alone; NAMES (covered) before it is refused.
    let names = fetches
        .iter()
        .position(|f| f.frames.len() >= 3)
        .expect("NAMES's three frames, merged with their neighbours");
    assert_ne!(names, 0);
    assert_eq!(
        inst.feed(names, &asset[at(&fetches[names].compressed)], &mut put),
        Err(Error::Install("a large section before its block hashes"))
    );
    inst.feed(0, &asset[at(&fetches[0].compressed)], &mut put)
        .unwrap();
    assert!(
        inst.feed(0, &asset[at(&fetches[0].compressed)], &mut put)
            .is_err(),
        "fed twice"
    );
    assert!(inst.finish().is_err(), "not every fetch fed");
    // A write that fails stops the install with its error.
    let mut full = |_: u64, _: &[u8]| -> Result<(), Error> { Err(Error::Install("disk full")) };
    assert_eq!(
        inst.feed(1, &asset[at(&fetches[1].compressed)], &mut full),
        Err(Error::Install("disk full"))
    );
}
