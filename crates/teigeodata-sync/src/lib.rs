//! Downloads, updates and storage of teigeodata packs
//! ([ADR-0012](https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/adr/ADR-0012-offline-loading.md),
//! [transport](https://github.com/teispace/teigeodata/blob/main/docs/04-architecture/transport.md)).
//!
//! Built so far (Phase 4 step 4.4): the [`transport`] asset, a pack in the zstd seekable
//! format with frames on the pack's own structure:
//! - [`transport::SeekTable`] parses the seek table and checks it against the pack's layout;
//! - [`transport::Frames::plan`] turns wanted byte ranges of the pack into frame fetches;
//! - [`transport::Decoder`] (feature `zstd`) decodes one frame, its output capped at the
//!   frame's stated size;
//! - [`transport::write`] (feature `write`) writes the asset and checks it byte for byte.
//!
//! Step 4.3: `trust` (feature `trust`) verifies the signed catalog, a subset of TUF: the root
//! chain, timestamp, snapshot and targets, with length caps and the offline-first expiry rule.
//! [`catalog`] (feature `trust`) reads each target's pack details, schema v1.
//!
//! The reader side is `no_std` with `alloc` and sans-I/O: it never touches a file or a socket.
//!
//! ```
//! # #[cfg(feature = "write")] {
//! use teigeodata_format::{Encoding, Kind, Writer};
//! use teigeodata_sync::transport::{self, Layout, SeekTable};
//!
//! let mut w = Writer::new("np", "places", "2026.11.0")?;
//! w.section_bytes(Kind::PLACES, Encoding::RAW, 3, b"abc")?;
//! let pack = w.layout(false)?.assemble(&[b"abc"])?;
//!
//! // Any zstd compressor: this example stores the bytes in raw blocks.
//! let raw = |b: &[u8]| Some(transport::store(b));
//! let asset = transport::encode(&pack, raw)?;
//!
//! let layout = Layout::of(&teigeodata_format::Pack::open(&pack)?)?;
//! let tail = SeekTable::tail_len(&asset)?;
//! let table = SeekTable::parse(&asset[asset.len() - tail..])?;
//! let frames = table.check(asset.len() as u64, layout)?;
//! assert_eq!(frames.len(), 2); // the head, then PLACES with the footer
//! # }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

#[cfg(feature = "trust")]
pub mod catalog;
mod error;
#[cfg(feature = "fuzz")]
#[doc(hidden)]
pub mod fuzz;
pub mod transport;
#[cfg(feature = "trust")]
pub mod trust;

pub use error::Error;
