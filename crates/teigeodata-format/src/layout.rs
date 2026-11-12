//! The fixed byte layouts: header, section-table entry, footer. Every field is little-endian and
//! unaligned, so a view never depends on the buffer's alignment or the host's byte order. The
//! derives check at compile time that no struct has padding.

use zerocopy::byteorder::little_endian::{U16, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// The first and the last eight bytes of every pack. PNG-style: a high byte, the name, a CR LF
/// pair, a DOS end-of-file and an LF, so text-mode transfers and truncation show at once.
pub(crate) const MAGIC: [u8; 8] = [0x89, b'T', b'G', b'D', b'\r', b'\n', 0x1a, b'\n'];

/// Sections start on 64-byte boundaries (cache lines, SIMD loads).
pub(crate) const ALIGN: u64 = 64;

/// The header, the section table, the manifest and the signature fit in this many bytes, so one
/// range request plans every later read.
pub(crate) const HEAD_MAX: u64 = 16 * 1024;

/// The bytes of the header that the head digest covers: everything before `head_hash`.
pub(crate) const HEADER_HASHED: usize = 112;

/// The signature block: an 8-byte key ID and a 64-byte Ed25519 signature.
pub(crate) const SIGNATURE_LEN: u32 = 72;

/// The 128-byte header at offset 0.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C)]
pub(crate) struct Header {
    pub(crate) magic: [u8; 8],
    pub(crate) format_major: U16,
    pub(crate) format_minor: U16,
    pub(crate) min_reader_major: U16,
    pub(crate) flags: U16,
    pub(crate) pack_uuid: [u8; 16],
    pub(crate) build_id: U64,
    pub(crate) created_unix: U64,
    pub(crate) table_off: U64,
    pub(crate) section_count: U32,
    pub(crate) manifest_len: U32,
    pub(crate) manifest_off: U64,
    pub(crate) signature_off: U64,
    pub(crate) signature_len: U32,
    pub(crate) file_len: U64,
    pub(crate) reserved: [u8; 20],
    /// The first 16 bytes of the head digest.
    pub(crate) head_hash: [u8; 16],
}

/// One 80-byte entry of the section table.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C)]
pub(crate) struct Entry {
    pub(crate) kind: U32,
    pub(crate) encoding: U16,
    pub(crate) flags: U16,
    pub(crate) offset: U64,
    pub(crate) length: U64,
    pub(crate) decoded_length: U64,
    pub(crate) item_count: U64,
    pub(crate) blake3: [u8; 32],
    /// The section's part of its kind (ADR-0040); 0 for the base part.
    pub(crate) part: U32,
    pub(crate) reserved: [u8; 4],
}

/// The 32-byte footer: the magic again, the file length and the head hash.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C)]
pub(crate) struct Footer {
    pub(crate) magic: [u8; 8],
    pub(crate) file_len: U64,
    pub(crate) head_hash: [u8; 16],
}

pub(crate) const HEADER_LEN: u64 = 128;
pub(crate) const ENTRY_LEN: u64 = 80;
pub(crate) const FOOTER_LEN: u64 = FOOTER_BYTES as u64;
pub(crate) const FOOTER_BYTES: usize = 32;

const _: () = assert!(size_of::<Header>() as u64 == HEADER_LEN);
const _: () = assert!(size_of::<Entry>() as u64 == ENTRY_LEN);
const _: () = assert!(size_of::<Footer>() == FOOTER_BYTES);
const _: () = assert!(core::mem::offset_of!(Header, head_hash) == HEADER_HASHED);

/// The domain-separated head digest: BLAKE3 over the hashed header bytes, the section table and
/// the manifest. It covers every section through the table's hashes, so it is what a signature
/// signs.
pub(crate) fn head_digest(header: &[u8], table: &[u8], manifest: &[u8]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(header);
    h.update(table);
    h.update(manifest);
    *h.finalize().as_bytes()
}

/// The pack UUID: derived, never random, so builds are reproducible (D-071).
pub(crate) fn pack_uuid(address: &str, component: &str, release: &str) -> [u8; 16] {
    let mut h = blake3::Hasher::new();
    h.update(b"teigeodata pack uuid\0");
    for part in [address, component, release] {
        h.update(part.as_bytes());
        // A separator: ("a", "bc") and ("ab", "c") must differ.
        h.update(&[0]);
    }
    let mut out = [0; 16];
    out.copy_from_slice(h.finalize().as_bytes().get(..16).unwrap_or(&[0; 16]));
    out
}

/// Rounds up to the next multiple of [`ALIGN`]; `None` on overflow.
pub(crate) const fn align_up(v: u64) -> Option<u64> {
    match v.checked_add(ALIGN - 1) {
        Some(x) => Some(x & !(ALIGN - 1)),
        None => None,
    }
}
