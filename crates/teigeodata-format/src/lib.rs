//! The `.tgd` pack container ([pack format](https://github.com/teispace/teigeodata/blob/main/docs/04-architecture/pack-format.md),
//! [ADR-0009](https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/adr/ADR-0009-pack-format.md)).
//!
//! - [`Pack::open`] validates a pack in place, without copying or allocating, and gives
//!   zero-copy views of its sections and manifest. It never panics, on any input.
//! - [`Writer`] (feature `write`) lays out a pack deterministically. It plans the layout from
//!   section lengths and hashes, so a large pack streams to disk with bounded memory.
//! - Feature `signature`: Ed25519 signatures over the head digest.
//! - Feature `zstd`: reading [BLOBS](blobs) (cold data in zstd frames, with `alloc`).
//! - Feature `spatial`: nearest places on the [place index](place_index) (pure-Rust `libm`).
//!
//! The encoding is canonical: [`Pack::open`] accepts a file only if it is exactly what the
//! writer produces for its content.
//!
//! ```
//! # #[cfg(feature = "write")] {
//! use teigeodata_format::{Encoding, Kind, Pack, Writer};
//!
//! let mut w = Writer::new("np", "places", "2026.11.0")?;
//! w.section_bytes(Kind::PLACES, Encoding::RAW, 3, b"abc")?;
//! let bytes = w.layout(false)?.assemble(&[b"abc"])?;
//!
//! let pack = Pack::open_verified(&bytes)?;
//! assert_eq!(pack.manifest().address(), "np");
//! assert_eq!(pack.section(Kind::PLACES).map(|s| s.bytes()), Some(&b"abc"[..]));
//! # }
//! # Ok::<(), teigeodata_format::Error>(())
//! ```

#![no_std]

#[cfg(any(feature = "alloc", feature = "zstd"))]
extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod arcs;
pub mod blobs;
pub mod blocks;
pub mod check;
mod columns;
pub mod component;
pub mod components;
pub mod dem;
pub mod elevation;
mod error;
#[cfg(feature = "write")]
pub mod extract;
#[cfg(feature = "fuzz")]
#[doc(hidden)]
pub mod fuzz;
pub mod hierarchy;
pub mod idmap;
pub mod iso_codes;
mod layout;
mod manifest;
pub mod names;
pub mod place_index;
pub mod place_tz;
pub mod places;
pub mod poly_index;
pub mod polygons;
pub mod postal;
mod read;
pub mod redirects;
pub mod region;
mod rtree;
pub mod search;
#[cfg(feature = "signature")]
mod signature;
mod sorted;
pub mod tz_rules;
#[cfg(feature = "write")]
mod write;

use core::fmt;

pub use error::Error;
pub use manifest::{Bbox, Entries, MAX_TEXT, Manifest, Tag, name_like, release_cmp};
pub use read::{FOOTER_SIZE, Pack, PackHead, Section, SectionEntry, footer_of};
#[cfg(all(feature = "signature", feature = "write"))]
pub use signature::signer;
#[cfg(feature = "signature")]
pub use signature::{SIGNATURE_DOMAIN, key_id};
#[cfg(feature = "write")]
pub use write::{Layout, Placed, Sign, Writer};

/// The format major version this crate writes. 0 until the format freezes (D-029).
pub const FORMAT_MAJOR: u16 = 0;
/// The format minor version this crate writes.
pub const FORMAT_MINOR: u16 = 1;
/// The newest `min_reader_major` this reader accepts.
pub const READER_MAJOR: u16 = 0;

/// A section kind. Unknown kinds are kept and skipped, so newer packs stay readable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Kind(pub u32);

impl Kind {
    /// Stable u64 IDs and ID → row.
    pub const IDMAP: Self = Self(1);
    /// Points, kind and class codes, flags.
    pub const PLACES: Self = Self(2);
    /// Parent rows, children lists, validity.
    pub const HIERARCHY: Self = Self(3);
    /// Name records and text.
    pub const NAMES: Self = Self(4);
    /// The text-search index.
    pub const SEARCH: Self = Self(5);
    /// The spatial index over points.
    pub const PLACE_INDEX: Self = Self(6);
    /// Shared polygon arcs.
    pub const ARCS: Self = Self(7);
    /// Ring and part tables.
    pub const POLYGONS: Self = Self(8);
    /// The spatial index over polygons.
    pub const POLY_INDEX: Self = Self(9);
    /// The exact region index.
    pub const REGION: Self = Self(10);
    /// Postal codes.
    pub const POSTAL: Self = Self(11);
    /// Optional column groups with presence bitmaps.
    pub const COMPONENTS: Self = Self(12);
    /// Elevation tiles.
    pub const DEM: Self = Self(13);
    /// Cold data: attribution text, long strings.
    pub const BLOBS: Self = Self(14);
    /// Per-block hashes of large sections.
    pub const BLOCK_HASHES: Self = Self(15);
    /// Retired IDs and their survivors.
    pub const REDIRECTS: Self = Self(16);
    /// The exact admin-unit index: REGION's layout on the ARCS vertices.
    pub const ADMIN_REGION: Self = Self(17);
    /// The coarse levels of ARCS alone, for fast coarse reads: ARCS's layout.
    pub const ARCS_OVERVIEW: Self = Self(18);
    /// Each place's height, one row per PLACES row (D-133).
    pub const ELEVATION: Self = Self(19);
    /// Each place's timezone area (D-134).
    pub const PLACE_TZ: Self = Self(20);
    /// The ISO 3166 code of each coded country and subdivision row (D-197).
    pub const ISO_CODES: Self = Self(21);
    /// The rules of the pack's timezones: transitions, types and footers (D-196).
    pub const TZ_RULES: Self = Self(22);
    /// Word-suffix keys of names to places, for substring search (D-199): the SEARCH layout
    /// with one map.
    pub const SUBSTRING: Self = Self(23);
    /// A coarse elevation grid for small installs (D-225): DEM's layout, about 15″.
    pub const DEM_COARSE: Self = Self(24);

    /// This reader's name for the kind, if it knows it.
    #[must_use]
    pub fn name(self) -> Option<&'static str> {
        KINDS.iter().find(|(k, _)| *k == self).map(|(_, n)| *n)
    }
}

/// Every kind this reader knows, with its name in the spec.
pub(crate) const KINDS: &[(Kind, &str)] = &[
    (Kind::IDMAP, "IDMAP"),
    (Kind::PLACES, "PLACES"),
    (Kind::HIERARCHY, "HIERARCHY"),
    (Kind::NAMES, "NAMES"),
    (Kind::SEARCH, "SEARCH"),
    (Kind::PLACE_INDEX, "PLACE_INDEX"),
    (Kind::ARCS, "ARCS"),
    (Kind::POLYGONS, "POLYGONS"),
    (Kind::POLY_INDEX, "POLY_INDEX"),
    (Kind::REGION, "REGION"),
    (Kind::POSTAL, "POSTAL"),
    (Kind::COMPONENTS, "COMPONENTS"),
    (Kind::DEM, "DEM"),
    (Kind::BLOBS, "BLOBS"),
    (Kind::BLOCK_HASHES, "BLOCK_HASHES"),
    (Kind::REDIRECTS, "REDIRECTS"),
    (Kind::ADMIN_REGION, "ADMIN_REGION"),
    (Kind::ARCS_OVERVIEW, "ARCS_OVERVIEW"),
    (Kind::ELEVATION, "ELEVATION"),
    (Kind::PLACE_TZ, "PLACE_TZ"),
    (Kind::ISO_CODES, "ISO_CODES"),
    (Kind::TZ_RULES, "TZ_RULES"),
    (Kind::SUBSTRING, "SUBSTRING"),
    (Kind::DEM_COARSE, "DEM_COARSE"),
];

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(n) => f.write_str(n),
            None => write!(f, "kind {}", self.0),
        }
    }
}

/// How a section's bytes are encoded as a whole. Column codecs inside a section are described
/// by its own header or the manifest's column descriptors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Encoding(pub u16);

impl Encoding {
    /// Stored as is.
    pub const RAW: Self = Self(0);
    /// A table of packed columns (`teigeodata_core::table`, D-074).
    pub const TABLE: Self = Self(1);
}

#[cfg(test)]
mod tests {
    use super::{KINDS, Kind};

    #[test]
    fn every_kind_is_documented_with_its_number() {
        let spec = include_str!("../../../docs/04-architecture/pack-format.md");
        // Numbers increase from 1; a number can be reserved for a kind still being built.
        let mut last = 0;
        for (k, name) in KINDS {
            assert!(k.0 > last, "{name}");
            last = k.0;
            let row = std::format!("| {} | {name} |", k.0);
            assert!(
                spec.contains(&row),
                "pack-format.md has no row starting {row}"
            );
        }
        assert_eq!(Kind(99).name(), None);
    }
}
