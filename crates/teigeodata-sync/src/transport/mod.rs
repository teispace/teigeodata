//! The transport asset: a pack in the zstd seekable format, cut on the pack's own structure
//! ([transport](https://github.com/teispace/teigeodata/blob/main/docs/04-architecture/transport.md),
//! D-112).
//!
//! The asset is ordinary independent zstd frames, then one skippable frame holding the seek
//! table. Any zstd decoder reads it whole and gets the `.tgd` back byte for byte. The frames:
//! - frame 0 is the head (header, section table, manifest, signature) and its padding;
//! - then each section with its padding, the last one with the footer;
//! - a section of 1 MiB or more is cut every [`MAX_FRAME`] bytes from its start, which falls on
//!   the 16 KiB grid of `BLOCK_HASHES` (D-078), so a frame never crosses a section boundary and a
//!   decoded frame is whole blocks.
//!
//! These cuts are canonical: [`Layout`] derives them from the section table, and
//! [`SeekTable::check`] accepts a seek table only if its decoded sizes are exactly these cuts.
//! The seek table is not signed and need not be. It only says where to read; each frame's
//! output is capped at the decoded size the signed head implies, and the bytes are then checked
//! against BLAKE3 block by block. A lying table can waste a request, not install wrong bytes.

#[cfg(feature = "zstd")]
mod decode;
mod layout;
mod table;
#[cfg(feature = "write")]
mod write;

#[cfg(feature = "zstd")]
pub use decode::Decoder;
pub use layout::{Fetch, Frame, Frames, Layout, SectionSpan};
pub use table::{Entry, SeekTable, encode_table};
#[cfg(feature = "write")]
pub use write::{Written, encode, store, verify, write};

/// The most decoded bytes in one frame: 1 MiB, 64 `BLOCK_HASHES` blocks. It bounds the bytes
/// lost to an interrupted download and a decoder's memory.
pub const MAX_FRAME: u64 = 1 << 20;
/// The most decoded bytes in the head frame: the 16 KiB head, plus the 32-byte footer of a pack
/// without sections.
pub const MAX_HEAD_FRAME: u64 = (16 << 10) + 32;
/// The most frames in one asset (a 4 TiB pack at 1 MiB per frame).
pub const MAX_FRAMES: usize = 1 << 22;
/// The smallest zstd frame: magic, frame header descriptor, one header byte, a block header.
pub const MIN_FRAME_LEN: u64 = 9;
/// The magic number of a zstd frame.
pub const ZSTD_MAGIC: u32 = 0xFD2F_B528;
/// The magic number of the skippable frame that holds the seek table.
pub const SKIPPABLE_MAGIC: u32 = 0x184D_2A5E;
/// The magic number that ends a seekable file.
pub const SEEKABLE_MAGIC: u32 = 0x8F92_EAB1;
/// The seek-table footer: frame count u32, descriptor u8, [`SEEKABLE_MAGIC`].
pub const FOOTER_LEN: usize = 9;

/// The largest compressed size of a frame of `n` decoded bytes: zstd's `ZSTD_COMPRESSBOUND`.
/// A seek-table entry above it is a lie.
#[must_use]
pub const fn compress_bound(n: u64) -> u64 {
    let small = if n < (128 << 10) {
        ((128 << 10) - n) >> 11
    } else {
        0
    };
    n + (n >> 8) + small
}
