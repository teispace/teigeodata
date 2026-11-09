//! The transport asset end to end: the writer's frames, the reference `zstd` CLI and the
//! `zeekstd` crate as oracles, the planner, and the refusals.
#![allow(
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    reason = "tests: a failure panics with its message; buffers sized here"
)]

use std::io::Read;
use std::process::Command;

use teigeodata_format::blocks::BLOCK;
use teigeodata_format::{Encoding, Kind, Pack, Writer};
use teigeodata_sync::Error;
use teigeodata_sync::transport::{
    Decoder, Frames, Layout, MAX_FRAME, SeekTable, compress_bound, encode, encode_table, store,
    verify,
};

/// A pack with small, empty and large sections: NAMES is 2.5 MiB, so it is cut into three
/// frames and gets `BLOCK_HASHES`.
fn pack() -> Vec<u8> {
    let idmap: Vec<u8> = (0..5000_u32).flat_map(u32::to_le_bytes).collect();
    let places: Vec<u8> = (0..70_000_u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    let names: Vec<u8> = (0..(5 << 19))
        .map(|i: u32| b"Lahachowk Kathmandu Pokhara "[(i % 28) as usize] ^ (i >> 16) as u8)
        .collect();
    let sections: [(Kind, Encoding, u64, &[u8]); 4] = [
        (Kind::IDMAP, Encoding::RAW, 5000, &idmap),
        (Kind::PLACES, Encoding::RAW, 7, &places),
        (Kind::HIERARCHY, Encoding::RAW, 0, &[]),
        (Kind::NAMES, Encoding::RAW, 9, &names),
    ];
    Writer::new("np", "places", "2026.11.0")
        .and_then(|w| w.pack(&sections, None))
        .unwrap()
}

fn zstd(level: i32) -> impl FnMut(&[u8]) -> Option<Vec<u8>> {
    move |b| zstd::bulk::compress(b, level).ok()
}

fn frames_of(asset: &[u8], pack: &[u8]) -> Result<Frames, Error> {
    let layout = Layout::of(&Pack::open(pack).unwrap())?;
    let tail = SeekTable::tail_len(asset)?;
    SeekTable::parse(&asset[asset.len() - tail..])?.check(asset.len() as u64, layout)
}

#[test]
fn frames_follow_the_pack() {
    let pack = pack();
    let asset = encode(&pack, zstd(3)).unwrap();
    let frames = frames_of(&asset, &pack).unwrap();
    let p = Pack::open(&pack).unwrap();
    let layout = frames.layout();
    // The head, IDMAP, PLACES, HIERARCHY (empty, so it shares NAMES's start), NAMES in three,
    // then BLOCK_HASHES with the footer.
    assert_eq!(p.sections().len(), 5);
    assert_eq!(frames.len(), 1 + 2 + 3 + 1);
    let first = layout.sections()[0].offset;
    assert_eq!(frames.frame(0).unwrap().decoded, 0..first);
    assert!(first <= 16 << 10);
    let names = layout
        .sections()
        .iter()
        .find(|s| s.kind == Kind::NAMES)
        .unwrap();
    let cuts: Vec<u64> = frames
        .iter()
        .map(|f| f.decoded.start)
        .filter(|&s| s > names.offset && s < names.offset + names.len)
        .collect();
    assert_eq!(
        cuts,
        [names.offset + MAX_FRAME, names.offset + 2 * MAX_FRAME]
    );
    // On the BLOCK_HASHES grid.
    assert!(cuts.iter().all(|c| (c - names.offset) % BLOCK as u64 == 0));
    for f in frames.iter() {
        assert!(f.decoded.end - f.decoded.start <= MAX_FRAME);
    }
    // Deterministic.
    assert_eq!(encode(&pack, zstd(3)).unwrap(), asset);
}

/// Writes `bytes` to a fresh file under the target directory's temp folder.
fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("tg-sync-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
#[allow(
    clippy::print_stderr,
    reason = "says why the CLI oracle was skipped on a machine without zstd"
)]
fn the_zstd_cli_reads_the_asset() {
    if Command::new("zstd").arg("-V").output().is_err() {
        assert!(
            std::env::var_os("CI").is_none(),
            "the zstd CLI is required in CI"
        );
        eprintln!("zstd CLI not found: oracle skipped");
        return;
    }
    let pack = pack();
    for (name, asset) in [
        ("l19.tgd.zst", encode(&pack, zstd(19)).unwrap()),
        ("stored.tgd.zst", encode(&pack, |b| Some(store(b))).unwrap()),
    ] {
        let path = temp_file(name, &asset);
        let out = Command::new("zstd")
            .args(["-d", "-c", "-q"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(out.status.success(), "zstd -d failed on {name}: {out:?}");
        assert!(
            out.stdout == pack,
            "zstd -d of {name} differs from the pack"
        );
        std::fs::remove_file(&path).unwrap();
    }
}

#[test]
fn zeekstd_reads_the_asset_and_its_seek_table() {
    let pack = pack();
    let asset = encode(&pack, zstd(9)).unwrap();
    let frames = frames_of(&asset, &pack).unwrap();

    let mut d = zeekstd::Decoder::new(zeekstd::BytesWrapper::new(&asset)).unwrap();
    let st = d.seek_table().clone();
    assert_eq!(st.num_frames() as usize, frames.len());
    for (i, f) in frames.iter().enumerate() {
        let i = u32::try_from(i).unwrap();
        assert_eq!(st.frame_start_comp(i).unwrap(), f.compressed.start);
        assert_eq!(st.frame_end_comp(i).unwrap(), f.compressed.end);
        assert_eq!(st.frame_start_decomp(i).unwrap(), f.decoded.start);
        assert_eq!(st.frame_end_decomp(i).unwrap(), f.decoded.end);
    }
    let mut all = Vec::new();
    d.read_to_end(&mut all).unwrap();
    assert!(all == pack, "zeekstd's decode differs from the pack");

    // A seek into the middle of NAMES.
    let names = frames
        .layout()
        .sections()
        .iter()
        .find(|s| s.kind == Kind::NAMES)
        .unwrap();
    let (from, to) = (
        names.offset + MAX_FRAME - 100,
        names.offset + MAX_FRAME + 100,
    );
    let mut d = zeekstd::Decoder::new(zeekstd::BytesWrapper::new(&asset)).unwrap();
    d.set_offset(from).unwrap();
    d.set_offset_limit(to).unwrap();
    let mut part = Vec::new();
    d.read_to_end(&mut part).unwrap();
    assert_eq!(part, pack[from as usize..to as usize]);
}

#[test]
fn a_zeekstd_asset_parses_but_is_not_ours() {
    let pack = pack();
    let mut asset = Vec::new();
    {
        let mut e = zeekstd::Encoder::new(&mut asset).unwrap();
        std::io::Write::write_all(&mut e, &pack).unwrap();
        e.finish().unwrap();
    }
    let tail = SeekTable::tail_len(&asset).unwrap();
    let table = SeekTable::parse(&asset[asset.len() - tail..]).unwrap();
    let st = zeekstd::SeekTable::from_seekable(&mut zeekstd::BytesWrapper::new(&asset)).unwrap();
    assert_eq!(table.len(), st.num_frames() as usize);
    let sum: u64 = table.entries().map(|e| u64::from(e.decompressed)).sum();
    assert_eq!(sum, pack.len() as u64);
    // Its frames are not cut on our sections, so the checked reader refuses it.
    assert!(frames_of(&asset, &pack).is_err());
}

#[test]
fn one_section_is_one_fetch() {
    let pack = pack();
    let asset = encode(&pack, zstd(3)).unwrap();
    let frames = frames_of(&asset, &pack).unwrap();
    let mut decoder = Decoder::new();
    let mut out = Vec::new();
    for s in frames.layout().sections().to_vec() {
        let plan = frames.plan_sections(&[s.kind]).unwrap();
        if s.len == 0 {
            assert_eq!(plan.len(), 0);
            continue;
        }
        assert_eq!(plan.len(), 1, "{:?}", s.kind);
        let fetch = &plan[0];
        let mut got = Vec::new();
        for i in fetch.frames.clone() {
            let f = frames.frame(i).unwrap();
            let src = &asset[f.compressed.start as usize..f.compressed.end as usize];
            decoder.frame(i, &f, src, &mut out).unwrap();
            got.extend_from_slice(&out);
        }
        let at = (s.offset - fetch.decoded.start) as usize;
        assert_eq!(
            got[at..at + s.len as usize],
            pack[s.offset as usize..(s.offset + s.len) as usize]
        );
    }
    // Two ranges in adjacent frames merge into one request; a byte in the head is frame 0.
    let all = frames.plan(&[0..1, 1..pack.len() as u64]).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].frames, 0..frames.len());
    assert_eq!(
        all[0].compressed,
        0..asset.len() as u64 - frames.table_len()
    );
    assert_eq!(
        frames.plan(&[0..1, 0..pack.len() as u64 + 1]),
        Err(Error::Range)
    );
}

/// Rewrites the seek table of `asset` with entries changed by `edit`.
fn with_table(asset: &[u8], edit: impl FnOnce(&mut Vec<(u32, u32)>)) -> Vec<u8> {
    let tail = SeekTable::tail_len(asset).unwrap();
    let table = SeekTable::parse(&asset[asset.len() - tail..]).unwrap();
    let mut entries: Vec<(u32, u32)> = table
        .entries()
        .map(|e| (e.compressed, e.decompressed))
        .collect();
    edit(&mut entries);
    let mut out = asset[..asset.len() - tail].to_vec();
    encode_table(entries.into_iter(), &mut out).unwrap();
    out
}

#[test]
fn lying_tables_are_refused() {
    let pack = pack();
    let asset = encode(&pack, zstd(3)).unwrap();
    assert!(frames_of(&asset, &pack).is_ok());
    // A frame that crosses a section boundary: 64 bytes moved from frame 2 to frame 1.
    let cross = with_table(&asset, |e| {
        e[1].1 += 64;
        e[2].1 -= 64;
    });
    assert!(matches!(
        frames_of(&cross, &pack),
        Err(Error::Frame { index: 1, .. })
    ));
    // One oversized frame for all of NAMES (2.5 MiB): too few frames.
    let merged = with_table(&asset, |e| {
        let (c, d) = (e[3].0 + e[4].0 + e[5].0, e[3].1 + e[4].1 + e[5].1);
        e.splice(3..6, [(c, d)]);
    });
    assert!(matches!(
        frames_of(&merged, &pack),
        Err(Error::SeekTable(_))
    ));
    // A compressed size above zstd's bound, and one below a frame's minimum.
    let big = with_table(&asset, |e| {
        e[0].0 = u32::try_from(compress_bound(u64::from(e[0].1)) + 1).unwrap();
    });
    assert!(matches!(
        frames_of(&big, &pack),
        Err(Error::Frame { index: 0, .. })
    ));
    let tiny = with_table(&asset, |e| e[3].0 = 4);
    assert!(frames_of(&tiny, &pack).is_err());
    // Compressed sizes shifted between frames (total kept): accepted by the table, refused by
    // the decoder, never installed.
    let shifted = with_table(&asset, |e| {
        e[1].0 -= 3;
        e[2].0 += 3;
    });
    assert!(frames_of(&shifted, &pack).is_ok());
    assert!(matches!(
        verify(&shifted, &pack),
        Err(Error::Frame { index: 1, .. })
    ));
    // Reserved descriptor bits.
    let mut reserved = asset.clone();
    let at = reserved.len() - 5;
    reserved[at] |= 0x04;
    assert!(matches!(
        SeekTable::tail_len(&reserved),
        Err(Error::SeekTable(_))
    ));
}

#[test]
fn truncated_and_changed_assets_are_refused() {
    let pack = pack();
    let asset = encode(&pack, zstd(3)).unwrap();
    assert_eq!(verify(&asset, &pack), Ok(()));
    for cut in [1, 9, 17, asset.len() / 2, asset.len() - 1] {
        assert!(
            verify(&asset[..asset.len() - cut], &pack).is_err(),
            "cut {cut}"
        );
    }
    let mut longer = asset.clone();
    longer.insert(0, 0);
    assert!(verify(&longer, &pack).is_err());
    // A changed byte inside a stored frame decodes, to bytes the pack's hashes refuse.
    let stored = encode(&pack, |b| Some(store(b))).unwrap();
    let frames = frames_of(&stored, &pack).unwrap();
    let f = frames.frame(2).unwrap();
    let mut bad = stored.clone();
    bad[(f.compressed.end - 10) as usize] ^= 0x20;
    assert!(matches!(verify(&bad, &pack), Err(Error::Write(_))));
}

#[test]
fn the_self_check_refuses_a_faulty_compressor() {
    let pack = pack();
    // Compresses the wrong bytes.
    let wrong = encode(&pack, |b| zstd::bulk::compress(&b[..b.len() / 2], 3).ok());
    assert!(matches!(wrong, Err(Error::Write(_))), "{wrong:?}");
    // A frame with a flipped byte that still decodes (a stored frame).
    let flipped = encode(&pack, |b| {
        let mut f = store(b);
        let last = f.len() - 1;
        f[last] ^= 1;
        Some(f)
    });
    assert_eq!(
        flipped,
        Err(Error::Write("a frame does not decode to its bytes"))
    );
    // Two frames for one call, and a failure.
    let two = encode(&pack, |b| {
        let mut f = store(b);
        f.extend(store(&[]));
        Some(f)
    });
    assert!(matches!(two, Err(Error::Write(_))));
    assert_eq!(
        encode(&pack, |_| None),
        Err(Error::Write("the compressor failed"))
    );
    // Not a pack.
    assert!(matches!(
        encode(b"not a pack", zstd(3)),
        Err(Error::Pack(_))
    ));
}

#[test]
fn a_decompression_bomb_stops_at_its_size() {
    let mut d = Decoder::new();
    let mut out = Vec::new();
    let bomb = zstd::bulk::compress(&vec![0; 1 << 20], 19).unwrap();
    assert!(bomb.len() < 200);
    assert!(d.decode(&bomb, 4096, &mut out).is_err());
    assert_eq!(out.len(), 4096);
    assert!(d.decode(&bomb, 1 << 20, &mut out).is_ok());
    // Over the frame limit, refused before decoding.
    let over = zstd::bulk::compress(&vec![0; (1 << 20) + 1], 19).unwrap();
    assert!(d.decode(&over, (1 << 20) + 1, &mut out).is_err());
}

#[test]
fn the_bound_is_zstds() {
    for n in [
        0,
        1,
        255,
        256,
        4096,
        131_071,
        131_072,
        131_073,
        1 << 20,
        (1 << 20) + 7,
    ] {
        assert_eq!(
            compress_bound(n as u64),
            zstd::zstd_safe::compress_bound(n) as u64,
            "{n}"
        );
    }
}

/// Installing by component (D-133, D-136): the plan fetches the head and the chosen sections
/// (with what they need, and `BLOCK_HASHES` only for a section of 1 MiB or more); those frames
/// decoded into a zeroed file, with the footer rebuilt from the head, open as the pack, the
/// installed sections verify and the others do not.
#[test]
fn a_component_install_fetches_only_its_frames_and_opens() {
    use teigeodata_format::component::{Component, Components};
    let pack = pack();
    let asset = encode(&pack, zstd(3)).unwrap();
    let frames = frames_of(&asset, &pack).unwrap();
    let mut decoder = Decoder::new();
    let mut out = Vec::new();
    let install = |set: Components, decoder: &mut Decoder, out: &mut Vec<u8>| {
        let plan = frames.plan_components(set).unwrap();
        let mut file = vec![0_u8; pack.len()];
        let mut fetched = 0;
        for fetch in &plan {
            fetched += fetch.compressed.end - fetch.compressed.start;
            for i in fetch.frames.clone() {
                let f = frames.frame(i).unwrap();
                let src = &asset[f.compressed.start as usize..f.compressed.end as usize];
                decoder.frame(i, &f, src, out).unwrap();
                file[f.decoded.start as usize..f.decoded.end as usize].copy_from_slice(out);
            }
        }
        let footer = teigeodata_format::footer_of(&file).unwrap();
        let at = file.len() - footer.len();
        file[at..].copy_from_slice(&footer);
        (plan, file, fetched)
    };
    let kinds = |file: &[u8]| -> Vec<(Kind, bool)> {
        let p = Pack::open_head(file).unwrap();
        p.sections()
            .filter(|s| !s.bytes().is_empty())
            .map(|s| (s.kind(), s.verify().is_ok()))
            .collect()
    };

    // Points only: the head, IDMAP and PLACES, one request; no NAMES, no BLOCK_HASHES.
    let (plan, file, points) = install(Components::of(&[Component::Point]), &mut decoder, &mut out);
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].decoded.start, 0);
    assert_eq!(
        kinds(&file),
        [
            (Kind::IDMAP, true),
            (Kind::PLACES, true),
            (Kind::NAMES, false),
            (Kind::BLOCK_HASHES, false),
        ]
    );
    // Names: its 2.5 MiB section brings BLOCK_HASHES and, by its needs, the points.
    let (_, file, names) = install(Components::of(&[Component::Names]), &mut decoder, &mut out);
    assert!(kinds(&file).iter().all(|&(_, ok)| ok));
    assert!(names > points);
    // Everything: the whole asset but the seek table.
    let (_, file, all) = install(Components::all(), &mut decoder, &mut out);
    assert_eq!(file, pack);
    assert_eq!(all, asset.len() as u64 - frames.table_len());
    // A head that is not a pack has no footer.
    assert!(teigeodata_format::footer_of(&[0; 200]).is_err());
}
