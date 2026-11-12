//! Fuzz and property harnesses (feature `fuzz`, D-072).
//!
//! Each harness takes raw bytes and panics if a property fails. The same functions run under
//! `cargo test` (a seeded random driver in `tests/fuzz.rs`) and under libFuzzer (`fuzz/`), so
//! there is one harness per property and no copy.
//!
//! Properties:
//! - [`open`]: `Pack::open` never panics; any pack it accepts is exactly what the writer
//!   produces for its content (the encoding is canonical).
//! - [`round_trip`]: a model written and read back is unchanged; every single-byte change to the
//!   file and every truncation is detected.
//! - [`mutate`]: a valid pack with edited structure and a recomputed head hash, so the checks
//!   behind the hash are reached, then [`open`].
//! - [`manifest`]: `Manifest::parse` never panics and accepts only canonical encodings.
//! - [`sections`]: PLACES, HIERARCHY, IDMAP, REDIRECTS and BLOBS, each in a pack from the writer.
//! - [`bitpack`]: `Packed::parse`; every accessor agrees with a full decode.
//! - [`table`]: `Table::parse`; an accepted table is canonical (it re-encodes to its bytes).
//! - [`bits`]: `Bits::parse` and `check`; ranks agree with bits; a checked bitmap is canonical.

#![allow(
    clippy::missing_panics_doc,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    reason = "test harnesses: a panic is how a property failure is reported, and inputs are bounded"
)]

mod payloads;

pub use payloads::{bitpack, bits, sections, table};

use alloc::vec::Vec;

use ed25519_dalek::SigningKey;
use zerocopy::FromBytes;

use crate::layout::{FOOTER_LEN, HEADER_HASHED, HEADER_LEN, Header, head_digest};
use crate::{Encoding, Error, FORMAT_MAJOR, FORMAT_MINOR, Kind, Manifest, Pack, Tag, Writer};

/// A size limit of a harness: `native` normally, `miri` under Miri, where the nightly job runs
/// one input per harness and each costs up to a million times its native time. The Miri limits
/// still span more than one 128-value block; libFuzzer covers the native limits.
pub(crate) const fn limit(native: usize, miri: usize) -> usize {
    if cfg!(miri) { miri } else { native }
}

/// Reads structured values from fuzz bytes; runs out into zeros.
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn u8(&mut self) -> u8 {
        let (v, rest) = self.0.split_first().map_or((0, &[][..]), |(v, r)| (*v, r));
        self.0 = rest;
        v
    }
    fn u16(&mut self) -> u16 {
        u16::from_le_bytes([self.u8(), self.u8()])
    }
    fn u64(&mut self) -> u64 {
        u64::from_le_bytes([0; 8].map(|_| self.u8()))
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes([0; 4].map(|_| self.u8()))
    }
    /// A value below `n` (0 if `n` is 0).
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            usize::from(self.u16()) % n
        }
    }
    /// Every byte left.
    fn rest(&mut self) -> &'a [u8] {
        core::mem::take(&mut self.0)
    }
    fn bytes(&mut self, max: usize) -> &'a [u8] {
        let n = usize::from(self.u16()) % (max + 1);
        let (v, rest) = self.0.split_at(n.min(self.0.len()));
        self.0 = rest;
        v
    }
}

const ADDRESSES: [&str; 4] = ["np", "in", "ae-du", "in-br-t07"];
const KEY: [u8; 32] = [7; 32];

fn key() -> SigningKey {
    SigningKey::from_bytes(&KEY)
}

/// Rebuilds a pack from what the reader returned, without re-hashing its sections.
fn rebuild(p: &Pack<'_>) -> Vec<u8> {
    let m = p.manifest();
    let mut w = Writer::new(m.address(), m.component(), m.release())
        .unwrap_or_else(|e| panic!("an accepted identity is rejected by the writer: {e}"));
    w.build_id(p.build_id()).created_unix(p.created_unix());
    for (tag, v) in m.entries() {
        if ![Tag::ADDRESS, Tag::COMPONENT, Tag::RELEASE].contains(&tag) {
            w.manifest(tag, v)
                .unwrap_or_else(|e| panic!("an accepted manifest entry is rejected: {e}"));
        }
    }
    for s in p.sections() {
        let len = u64::try_from(s.bytes().len()).unwrap_or(u64::MAX);
        w.section(
            s.kind(),
            s.encoding(),
            s.item_count(),
            s.decoded_length(),
            len,
            *s.hash(),
        )
        .unwrap_or_else(|e| panic!("an accepted section is rejected: {e}"));
    }
    let mut layout = w
        .layout(p.signature().is_some())
        .unwrap_or_else(|e| panic!("an accepted pack cannot be laid out: {e}"));
    if let Some((id, sig)) = p.signature() {
        let mut block = [0; 72];
        block[..8].copy_from_slice(&id);
        block[8..].copy_from_slice(&sig);
        layout
            .set_signature(&block)
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let mut out = layout.head().unwrap_or_else(|e| panic!("{e}")).to_vec();
    for (placed, s) in layout.placed().iter().zip(p.sections()) {
        out.resize(usize::try_from(placed.offset).unwrap_or(usize::MAX), 0);
        out.extend_from_slice(s.bytes());
    }
    out.extend_from_slice(layout.footer());
    out
}

/// `Pack::open`, `Pack::open_head` and `footer_of` never panic; any pack `open` accepts is
/// canonical, `open_head` accepts it too, and its footer is the one `footer_of` rebuilds.
pub fn open(data: &[u8]) {
    let footer = crate::footer_of(data);
    if let Ok(h) = Pack::open_head(data) {
        // A partial install's reads: block lists are checked when asked for, never panic.
        for s in h.sections() {
            let _ = h.part_blocks(s.kind(), s.part());
        }
    }
    let Ok(p) = Pack::open(data) else { return };
    assert!(
        Pack::open_head(data).is_ok(),
        "open accepts what open_head refuses"
    );
    let tail = data.len() - crate::FOOTER_SIZE;
    assert_eq!(footer.ok().as_ref().map(|f| &f[..]), data.get(tail..));
    // Every accessor works on an accepted pack.
    for s in p.sections() {
        assert_eq!(
            p.part(s.kind(), s.part()).map(|t| t.bytes()),
            Some(s.bytes())
        );
        let _ = s.verify();
    }
    let _ = (
        p.manifest().entries().count(),
        p.manifest().bbox(),
        p.uuid(),
    );
    let _ = p.verify_sections();
    let _ = p.verify_signature(&[key().verifying_key()]);
    let writer_version =
        p.format_version() == (FORMAT_MAJOR, FORMAT_MINOR) && p.min_reader_major() == FORMAT_MAJOR;
    if writer_version {
        assert_eq!(rebuild(&p), data, "an accepted pack is not canonical");
    }
}

/// A section kind for a model: any of 0 to 39 except `BLOCK_HASHES`, which is derived from the
/// other sections (`blocks::encode`); model sections are under 1 MiB, so a pack has none.
fn model_kind(b: u8) -> Kind {
    let k = u32::from(b) % 39;
    Kind(k + u32::from(k >= Kind::BLOCK_HASHES.0))
}

/// A model from fuzz bytes: written, read back unchanged, every change detected.
pub fn round_trip(data: &[u8]) {
    let mut c = Cursor(data);
    let address = ADDRESSES[usize::from(c.u8()) % ADDRESSES.len()];
    let Ok(mut w) = Writer::new(address, "places", "2026.11.0") else {
        panic!("a fixed identity is rejected")
    };
    w.build_id(c.u64()).created_unix(c.u64());
    let mut tags = Vec::new();
    for _ in 0..c.u8() % 6 {
        let tag = Tag(c.u16() & 0x7fff);
        let v = c.bytes(64);
        if w.manifest(tag, v).is_ok() {
            tags.push((tag, v));
        }
    }
    let mut sections = Vec::new();
    for _ in 0..c.u8() % 12 {
        let kind = model_kind(c.u8());
        let encoding = Encoding(u16::from(c.u8() % 3));
        let bytes = c.bytes(limit(300, 64));
        let items = c.u64();
        let len = bytes.len() as u64;
        let decoded = if encoding == Encoding::RAW {
            len
        } else {
            c.u64()
        };
        let hash = *blake3::hash(bytes).as_bytes();
        if w.section(kind, encoding, items, decoded, len, hash).is_ok() {
            sections.push((kind, encoding, items, decoded, bytes));
        }
    }
    sections.sort_by_key(|s| s.0);
    tags.sort_by_key(|t| t.0);
    let signed = c.u8() & 1 == 1;
    let Ok(mut layout) = w.layout(signed) else {
        panic!("a small model cannot be laid out")
    };
    if signed {
        let block = crate::signer(&key())(layout.digest());
        layout
            .set_signature(&block)
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let bodies: Vec<&[u8]> = sections.iter().map(|s| s.4).collect();
    let bytes = layout.assemble(&bodies).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(bytes.len() as u64, layout.file_len());

    let p =
        Pack::open_verified(&bytes).unwrap_or_else(|e| panic!("a written pack is refused: {e}"));
    assert_eq!(p.manifest().address(), address);
    let read: Vec<_> = p
        .sections()
        .map(|s| {
            (
                s.kind(),
                s.encoding(),
                s.item_count(),
                s.decoded_length(),
                s.bytes(),
            )
        })
        .collect();
    assert_eq!(read, sections);
    let extra: Vec<_> = p.manifest().entries().filter(|(t, _)| t.0 > 3).collect();
    assert_eq!(extra, tags);
    let trusted = [key().verifying_key()];
    let other = [SigningKey::from_bytes(&[8; 32]).verifying_key()];
    if signed {
        assert_eq!(p.verify_signature(&trusted), Ok(0));
        assert_eq!(
            p.verify_signature(&other),
            Err(Error::Signature("signed by a key that is not trusted"))
        );
    } else {
        assert_eq!(
            p.verify_signature(&trusted),
            Err(Error::Signature("the pack is not signed"))
        );
    }
    assert_eq!(rebuild(&p), bytes);

    // Any one changed byte is detected; so is any truncation.
    let at = usize::from(c.u16()) % bytes.len();
    let mut bad = bytes.clone();
    bad[at] ^= 1 << (c.u8() % 8);
    let detected = Pack::open_verified(&bad)
        .and_then(|p| {
            if signed {
                p.verify_signature(&trusted).map(|_| ())
            } else {
                Ok(())
            }
        })
        .is_err();
    assert!(
        detected,
        "a flipped bit at byte {at} of {} went unnoticed",
        bytes.len()
    );
    assert!(
        Pack::open(&bytes[..at]).is_err(),
        "a truncation to {at} bytes was accepted"
    );
}

/// Recomputes the head hash after an edit of the head, for tests that forge a field the
/// writer always sets right.
pub fn reseal_head(bytes: &mut [u8]) {
    reseal(bytes);
}

/// Recomputes the head hash after an edit, so the reader's later checks are reached.
fn reseal(bytes: &mut [u8]) {
    let Ok((h, _)) = Header::ref_from_prefix(bytes) else {
        return;
    };
    let table_end = usize::try_from(h.manifest_off.get()).unwrap_or(usize::MAX);
    let manifest_end = table_end.saturating_add(h.manifest_len.get() as usize);
    let (Some(header), Some(table), Some(manifest)) = (
        bytes.get(..HEADER_HASHED),
        bytes.get(HEADER_LEN as usize..table_end),
        bytes.get(table_end..manifest_end),
    ) else {
        return;
    };
    let digest = head_digest(header, table, manifest);
    let len = bytes.len();
    if let Some(hash) = bytes.get_mut(HEADER_HASHED..HEADER_HASHED + 16) {
        hash.copy_from_slice(&digest[..16]);
    }
    if let Some(hash) = bytes.get_mut(len - 16..) {
        hash.copy_from_slice(&digest[..16]);
    }
}

/// Header fields as (offset, size): every field the reader checks.
const HEADER_FIELDS: [(usize, usize); 15] = [
    (8, 2),
    (10, 2),
    (12, 2),
    (14, 2),
    (16, 1),
    (32, 8),
    (40, 8),
    (48, 8),
    (56, 4),
    (60, 4),
    (64, 8),
    (72, 8),
    (80, 4),
    (84, 8),
    (92, 1),
];
/// Section-table entry fields as (offset in the entry, size).
const ENTRY_FIELDS: [(usize, usize); 9] = [
    (0, 4),
    (4, 2),
    (6, 2),
    (8, 8),
    (16, 8),
    (24, 8),
    (32, 8),
    (40, 1),
    (72, 1),
];

fn read_le(b: &[u8], at: usize, size: usize) -> u64 {
    b.get(at..at + size).map_or(0, |f| {
        f.iter().rev().fold(0, |v, &x| (v << 8) | u64::from(x))
    })
}

fn write_le(b: &mut [u8], at: usize, size: usize, v: u64) {
    if let Some(f) = b.get_mut(at..at + size) {
        for (i, x) in f.iter_mut().enumerate() {
            *x = (v >> (8 * i)) as u8;
        }
    }
}

/// Sets the file length in the header and the footer to the real length.
fn fix_lengths(b: &mut [u8]) {
    let len = b.len();
    if len >= (HEADER_LEN + FOOTER_LEN) as usize {
        write_le(b, 84, 8, len as u64);
        write_le(b, len - 24, 8, len as u64);
    }
}

/// A valid pack, edited (bytes, named fields with boundary values, inserted or deleted bytes),
/// usually resealed so the checks behind the head hash are reached, then [`open`].
pub fn mutate(data: &[u8]) {
    let mut c = Cursor(data);
    let mut w = Writer::new(ADDRESSES[usize::from(c.u8()) % 4], "places", "2026.11.0")
        .unwrap_or_else(|e| panic!("{e}"));
    let _ = w.manifest(Tag::SCHEMA, &1_u32.to_le_bytes());
    let bodies: [&[u8]; 3] = [b"first section", &[0xab; 100], b""];
    for (kind, b) in [Kind::IDMAP, Kind::PLACES, Kind::REDIRECTS]
        .into_iter()
        .zip(bodies)
    {
        let _ = w.section_bytes(kind, Encoding::RAW, 1, b);
    }
    let signed = c.u8() & 1 == 1;
    let Ok(mut layout) = w.layout(signed) else {
        return;
    };
    if signed {
        let _ = layout.set_signature(&crate::signer(&key())(layout.digest()));
    }
    let Ok(mut bytes) = layout.assemble(&bodies) else {
        return;
    };
    let head = layout
        .placed()
        .first()
        .map_or(bytes.len(), |p| p.offset as usize);
    let entries = layout.placed().len();
    for _ in 0..=(c.u8() % 3) {
        match c.u8() % 4 {
            // A random byte, mostly in the head.
            0 => {
                let range = if c.u8() < 200 { head } else { bytes.len() };
                let at = usize::from(c.u16()) % range;
                bytes[at] = c.u8();
            }
            // A named field set to a boundary value.
            1 | 2 => {
                let (at, size) = if c.u8() & 1 == 0 {
                    HEADER_FIELDS[usize::from(c.u8()) % HEADER_FIELDS.len()]
                } else {
                    let (o, s) = ENTRY_FIELDS[usize::from(c.u8()) % ENTRY_FIELDS.len()];
                    (128 + 80 * (usize::from(c.u8()) % entries.max(1)) + o, s)
                };
                let old = read_le(&bytes, at, size);
                let other = {
                    let (o, s) = HEADER_FIELDS[usize::from(c.u8()) % HEADER_FIELDS.len()];
                    read_le(&bytes, o, s.min(size))
                };
                let v = match c.u8() % 8 {
                    0 => 0,
                    1 => 1,
                    2 => old.wrapping_add(1),
                    3 => old.wrapping_sub(1),
                    4 => old.wrapping_add(64),
                    5 => other,
                    6 => u64::MAX,
                    _ => c.u64(),
                };
                write_le(&mut bytes, at, size, v);
            }
            // Bytes inserted or deleted, with the length fields fixed.
            _ => {
                let at = usize::from(c.u16()) % bytes.len();
                let n = 1 + usize::from(c.u8() % 70);
                if c.u8() & 1 == 0 {
                    let fill = c.u8() & 1;
                    bytes.splice(at..at, core::iter::repeat_n(fill, n));
                } else {
                    bytes.drain(at..(at + n).min(bytes.len()));
                }
                fix_lengths(&mut bytes);
            }
        }
    }
    if c.u8() % 4 != 0 {
        reseal(&mut bytes);
    }
    open(&bytes);
}

/// `Manifest::parse` never panics, and accepts only canonical encodings.
pub fn manifest(data: &[u8]) {
    let Ok(m) = Manifest::parse(data) else { return };
    let mut again = Vec::new();
    let mut prev = None;
    for (tag, v) in m.entries() {
        assert!(prev < Some(tag));
        prev = Some(tag);
        assert_eq!(m.get(tag), Some(v));
        again.extend_from_slice(&tag.0.to_le_bytes());
        again.extend_from_slice(&(v.len() as u32).to_le_bytes());
        again.extend_from_slice(v);
    }
    assert_eq!(again, data, "an accepted manifest is not canonical");
}
