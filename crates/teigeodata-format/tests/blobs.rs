//! BLOBS through the writer and the reader: round trips with and without a dictionary, the
//! writer's refusals, and every forged section the reader must refuse, a decompression bomb
//! among them.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::many_single_char_names,
    clippy::too_many_lines,
    clippy::unnecessary_wraps,
    reason = "tests: small sizes, short names for forged spans, one table of forgeries, a compressor's signature"
)]

use teigeodata_core::bitpack::encode_direct;
use teigeodata_core::table::{Codec, encode_with};
use teigeodata_format::blobs::{
    self, BLOB_END, Blobs, DICT_END, FRAME_END, HEAP, MAX_FRAME, RAW_END,
};
use teigeodata_format::{Encoding, Error, Kind, Pack, Writer};

/// `ruzstd`'s own encoder (no dictionary): pure Rust, the default test compressor.
fn fast(raw: &[u8]) -> Option<Vec<u8>> {
    Some(ruzstd::encoding::compress_to_vec(
        raw,
        ruzstd::encoding::CompressionLevel::Fastest,
    ))
}

/// C zstd at level 19, as the forge writes: no dictionary ID, no checksum.
fn c19(dict: Option<&[u8]>) -> impl FnMut(&[u8]) -> Option<Vec<u8>> {
    let mut c = match dict {
        Some(d) => zstd::bulk::Compressor::with_dictionary(19, d).unwrap(),
        None => zstd::bulk::Compressor::new(19).unwrap(),
    };
    c.include_dictid(false).unwrap();
    c.include_checksum(false).unwrap();
    move |raw| c.compress(raw).ok()
}

/// Attribution-like text: short, repetitive records with numbers.
fn corpus(n: usize) -> Vec<Vec<u8>> {
    (0..n)
        .map(|i| {
            format!(
                "© OpenStreetMap contributors, ODbL 1.0; GeoNames CC-BY 4.0; record {i}, \
                 level {} of {}; {}",
                i % 7,
                i % 13,
                "x".repeat(i % 50)
            )
            .into_bytes()
        })
        .collect()
}

fn pack(body: &[u8], n: usize) -> Vec<u8> {
    let mut w = Writer::new("np", "blobs", "2026.11.0").unwrap();
    w.section_bytes(Kind::BLOBS, Encoding::TABLE, n as u64, body)
        .unwrap();
    w.layout(false).unwrap().assemble(&[body]).unwrap()
}

/// Opens `body` as BLOBS of `n` blobs, then runs both checks: the first error.
fn verdict(body: &[u8], n: usize) -> Result<(), Error> {
    let bytes = pack(body, n);
    let p = Pack::open(&bytes).unwrap();
    let b = Blobs::open(&p.section(Kind::BLOBS).unwrap())?;
    b.check()?;
    let mut d = b.decoder()?;
    d.check()?;
    for i in 0..b.len() {
        d.get(i)?;
    }
    Ok(())
}

/// Opens `body` and runs the structural check only, decoding nothing.
fn structure(body: &[u8], n: usize) -> Result<(), Error> {
    let bytes = pack(body, n);
    let p = Pack::open(&bytes).unwrap();
    Blobs::open(&p.section(Kind::BLOBS).unwrap())?.check()
}

fn read_all(body: &[u8], n: usize) -> Vec<Vec<u8>> {
    let bytes = pack(body, n);
    let p = Pack::open(&bytes).unwrap();
    let b = Blobs::open(&p.section(Kind::BLOBS).unwrap()).unwrap();
    b.check().unwrap();
    let mut d = b.decoder().unwrap();
    d.check().unwrap();
    // Out of order, so frames are decoded again and again.
    let mut out = vec![Vec::new(); b.len()];
    for i in (0..b.len()).rev().chain(0..b.len()) {
        out[i] = d.get(i).unwrap().to_vec();
    }
    assert!(d.get(b.len()).is_err());
    out
}

#[test]
fn round_trip_without_a_dictionary() {
    let mut all = corpus(3_000);
    // Empty blobs anywhere, and one larger than a frame.
    all.insert(0, Vec::new());
    all.insert(1_500, Vec::new());
    all.push(vec![7; 200_000]);
    all.push(Vec::new());
    let refs: Vec<&[u8]> = all.iter().map(Vec::as_slice).collect();
    let body = blobs::encode(&refs, None, 16 << 10, fast).unwrap();
    assert_eq!(read_all(&body, all.len()), all);
    let bytes = pack(&body, all.len());
    let p = Pack::open(&bytes).unwrap();
    let b = Blobs::open(&p.section(Kind::BLOBS).unwrap()).unwrap();
    assert!(b.frames() > 10, "{} frames", b.frames());
    assert!(b.dictionary().is_none());
    assert_eq!(b.span(1), Some(0..all[1].len()));
    // Deterministic: the same input gives the same bytes.
    assert_eq!(blobs::encode(&refs, None, 16 << 10, fast).unwrap(), body);
}

#[test]
fn round_trip_with_a_dictionary_is_smaller() {
    let all = corpus(4_000);
    let refs: Vec<&[u8]> = all.iter().map(Vec::as_slice).collect();
    let dict = zstd::dict::from_samples(&all, 16 << 10).unwrap();
    let with = blobs::encode(&refs, Some(&dict), 1 << 10, c19(Some(&dict))).unwrap();
    let without = blobs::encode(&refs, None, 1 << 10, c19(None)).unwrap();
    assert_eq!(read_all(&with, all.len()), all);
    assert_eq!(read_all(&without, all.len()), all);
    assert!(
        with.len() < without.len(),
        "{} >= {}",
        with.len(),
        without.len()
    );
    // A frame written with a dictionary does not decode without it, or with another.
    let other = zstd::dict::from_samples(&corpus(500)[..], 4 << 10).unwrap();
    let wrong = blobs::encode(&refs, Some(&other), 4 << 10, c19(Some(&dict)));
    assert!(matches!(wrong, Err(Error::Write(_))));
}

#[test]
fn the_writer_refuses() {
    let all = corpus(10);
    let refs: Vec<&[u8]> = all.iter().map(Vec::as_slice).collect();
    let refused = |r: Result<Vec<u8>, Error>| matches!(r, Err(Error::Write(_)));
    let huge = vec![0; MAX_FRAME + 1];
    assert!(refused(blobs::encode(&[&huge], None, 1 << 16, fast)));
    assert!(blobs::encode(&[&huge[1..]], None, 1 << 16, fast).is_ok());
    assert!(refused(blobs::encode(&refs, Some(&[]), 1 << 16, fast)));
    assert!(refused(blobs::encode(&refs, None, 1 << 16, |_| None)));
    assert!(refused(blobs::encode(&refs, None, 1 << 16, |_| Some(
        Vec::new()
    ))));
    // A compressor fault: frames of other bytes, of the wrong length, or not zstd.
    assert!(refused(blobs::encode(&refs, None, 1 << 16, |r| fast(
        &r[1..]
    ))));
    assert!(refused(blobs::encode(&refs, None, 1 << 16, |r| {
        let mut v = r.to_vec();
        v[0] ^= 1;
        fast(&v)
    })));
    assert!(refused(blobs::encode(&refs, None, 1 << 16, |r| Some(
        r.to_vec()
    ))));
    // No blobs at all is a valid, empty section.
    let empty = blobs::encode(&[], None, 1 << 16, fast).unwrap();
    assert!(verdict(&empty, 0).is_ok());
}

/// A section body by hand: what a forger could write.
fn forge(
    frame_end: &[u32],
    raw_end: &[u32],
    blob_end: &[u32],
    dict_end: Option<u32>,
    heap: &[u8],
) -> Vec<u8> {
    let mut heap = heap.to_vec();
    heap.resize(heap.len().next_multiple_of(4), 0);
    let mut cols = vec![
        (FRAME_END, Codec::Bitpack, encode_direct(frame_end)),
        (RAW_END, Codec::Bitpack, encode_direct(raw_end)),
        (BLOB_END, Codec::Bitpack, encode_direct(blob_end)),
    ];
    if let Some(d) = dict_end {
        cols.push((DICT_END, Codec::Bitpack, encode_direct(&[d])));
    }
    cols.push((HEAP, Codec::Bytes, heap));
    let refs: Vec<(u16, Codec, &[u8])> = cols
        .iter()
        .map(|(i, c, b)| (*i, *c, b.as_slice()))
        .collect();
    encode_with(&refs).unwrap()
}

fn column(r: Result<(), Error>) -> Option<u16> {
    match r {
        Err(Error::Payload {
            kind: 14, column, ..
        }) => Some(column),
        _ => None,
    }
}

#[test]
fn the_reader_refuses_forged_sections() {
    let a = b"first blob ".repeat(20);
    let b = b"second".to_vec();
    let raw = [a.clone(), b.clone()].concat();
    let z = fast(&raw).unwrap();
    let (zl, al, rl) = (z.len() as u32, a.len() as u32, raw.len() as u32);
    // The honest one.
    assert_eq!(
        verdict(&forge(&[zl], &[rl], &[al, rl], None, &z), 2),
        Ok(())
    );
    // Frames: empty, outside the heap, past the padding, garbage after the last.
    assert_eq!(
        column(verdict(&forge(&[0], &[rl], &[al, rl], None, &z), 2)),
        Some(FRAME_END)
    );
    assert_eq!(
        column(verdict(&forge(&[zl + 8], &[rl], &[al, rl], None, &z), 2)),
        Some(FRAME_END)
    );
    let mut trailing = z.clone();
    trailing.extend_from_slice(&[0; 4]);
    assert_eq!(
        column(verdict(&forge(&[zl], &[rl], &[al, rl], None, &trailing), 2)),
        Some(HEAP)
    );
    let mut junk = z.clone();
    junk.push(1);
    assert_eq!(
        column(verdict(&forge(&[zl], &[rl], &[al, rl], None, &junk), 2)),
        Some(HEAP)
    );
    // Raw spans: empty, over 8 MiB, the blobs ending short of the frame or past it.
    assert_eq!(
        column(verdict(&forge(&[zl], &[0], &[0, 0], None, &z), 2)),
        Some(RAW_END)
    );
    let over = MAX_FRAME as u32 + 1;
    assert_eq!(
        column(verdict(&forge(&[zl], &[over], &[al, over], None, &z), 2)),
        Some(RAW_END)
    );
    assert_eq!(
        column(verdict(&forge(&[zl], &[rl], &[al, rl - 1], None, &z), 2)),
        Some(BLOB_END)
    );
    assert_eq!(
        column(verdict(&forge(&[zl], &[rl], &[al, rl + 1], None, &z), 2)),
        Some(BLOB_END)
    );
    assert_eq!(
        column(verdict(&forge(&[zl], &[rl], &[rl, al], None, &z), 2)),
        Some(BLOB_END)
    );
    // The frame's stated length: shorter or longer than it decodes to.
    assert_eq!(
        column(verdict(
            &forge(&[zl], &[rl - 1], &[al, rl - 1], None, &z),
            2
        )),
        Some(HEAP)
    );
    assert_eq!(
        column(verdict(
            &forge(&[zl], &[rl + 1], &[al, rl + 1], None, &z),
            2
        )),
        Some(HEAP)
    );
    // A frame cut short, or its span cutting into the next frame.
    let z2 = [z.clone(), z.clone()].concat();
    let r2 = [rl, 2 * rl];
    assert_eq!(
        column(verdict(
            &forge(&[zl - 1, 2 * zl], &r2, &[rl, 2 * rl], None, &z2),
            2
        )),
        Some(HEAP)
    );
    assert_eq!(
        verdict(&forge(&[zl, 2 * zl], &r2, &[rl, 2 * rl], None, &z2), 2),
        Ok(())
    );
    // Each structural rule on its own, without the decoder's checks behind it: a frame over
    // 8 MiB, a frame end inside a blob, blobs going backwards, a frame left after the blobs.
    let s = |fe: &[u32], re: &[u32], be: &[u32], heap: &[u8], n| {
        column(structure(&forge(fe, re, be, None, heap), n))
    };
    assert_eq!(s(&[zl], &[over], &[al, over], &z, 2), Some(RAW_END));
    assert_eq!(
        s(&[zl, 2 * zl], &r2, &[al, rl + 1, 2 * rl], &z2, 3),
        Some(BLOB_END)
    );
    assert_eq!(s(&[zl], &[rl], &[al, 1, rl], &z, 3), Some(BLOB_END));
    assert_eq!(s(&[zl, 2 * zl], &r2, &[al, rl], &z2, 2), Some(BLOB_END));
    assert_eq!(s(&[zl, 2 * zl], &r2, &[al, rl, 2 * rl], &z2, 3), None);
    // Reads without the deep check refuse a backwards span too, never return it empty.
    let bytes = pack(&forge(&[zl], &[rl], &[al, 1, rl], None, &z), 3);
    let p = Pack::open(&bytes).unwrap();
    let b = Blobs::open(&p.section(Kind::BLOBS).unwrap()).unwrap();
    let mut d = b.decoder().unwrap();
    assert!(d.get(1).is_err());
    assert_eq!(d.get(0).unwrap(), &raw[..a.len()]);
    // Bytes after a whole zstd frame, inside its span.
    let mut tail = z.clone();
    tail.extend_from_slice(&[0; 4]);
    assert_eq!(
        column(verdict(&forge(&[zl + 4], &[rl], &[al, rl], None, &tail), 2)),
        Some(HEAP)
    );
    // A blob crossing a frame end.
    assert_eq!(
        verdict(&forge(&[zl, 2 * zl], &r2, &[al, rl, 2 * rl], None, &z2), 3),
        Ok(())
    );
    assert_eq!(
        column(verdict(
            &forge(&[zl, 2 * zl], &r2, &[al, rl + 1, 2 * rl], None, &z2),
            3
        )),
        Some(BLOB_END)
    );
    // Dictionaries: empty, not one, outside the heap.
    assert_eq!(
        column(verdict(&forge(&[zl], &[rl], &[al, rl], Some(0), &z), 2)),
        Some(DICT_END)
    );
    let notdict = [vec![9; 8], z.clone()].concat();
    assert_eq!(
        column(verdict(
            &forge(&[zl + 8], &[rl], &[al, rl], Some(8), &notdict),
            2
        )),
        Some(DICT_END)
    );
    assert_eq!(
        column(verdict(
            &forge(&[zl], &[rl], &[al, rl], Some(zl + 64), &z),
            2
        )),
        Some(DICT_END)
    );
}

#[test]
fn a_decompression_bomb_stops_at_its_stated_length() {
    // 8 MiB of zeros compresses to a few hundred bytes; the section claims 100 bytes.
    let zeros = vec![0; MAX_FRAME];
    let z = fast(&zeros).unwrap();
    assert!(z.len() < 4_096, "{}", z.len());
    let zl = z.len() as u32;
    assert_eq!(
        column(verdict(&forge(&[zl], &[100], &[100], None, &z), 1)),
        Some(HEAP)
    );
    // And honestly stated, it reads back: the limit is inclusive.
    let m = MAX_FRAME as u32;
    assert_eq!(verdict(&forge(&[zl], &[m], &[m], None, &z), 1), Ok(()));
}

#[test]
fn the_reader_refuses_unknown_columns_and_codecs() {
    let z = fast(b"abc").unwrap();
    let zl = z.len() as u32;
    let good = forge(&[zl], &[3], &[3], None, &z);
    assert_eq!(verdict(&good, 1), Ok(()));
    // The blob count is the section's item count.
    assert!(column(verdict(&good, 2)).is_some());
    let mut heap = z.clone();
    heap.resize(heap.len().next_multiple_of(4), 0);
    let (fe, re, be) = (
        encode_direct(&[zl]),
        encode_direct(&[3]),
        encode_direct(&[3]),
    );
    let extra = encode_direct(&[0]);
    let body = encode_with(&[
        (FRAME_END, Codec::Bitpack, &fe),
        (RAW_END, Codec::Bitpack, &re),
        (BLOB_END, Codec::Bitpack, &be),
        (HEAP, Codec::Bytes, &heap),
        (6, Codec::Bitpack, &extra),
    ])
    .unwrap();
    assert_eq!(column(verdict(&body, 1)), Some(0));
    let body = encode_with(&[
        (FRAME_END, Codec::Bitpack, &fe),
        (RAW_END, Codec::Bitpack, &re),
        (BLOB_END, Codec::Bitpack, &be),
        (DICT_END, Codec::Bytes, &[0, 0, 0, 0]),
        (HEAP, Codec::Bytes, &heap),
    ])
    .unwrap();
    assert_eq!(column(verdict(&body, 1)), Some(DICT_END));
}
