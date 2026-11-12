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

    /// Whether the kind may be split into parts (ADR-0040): names, search and substring keys
    /// by language, outlines by admin level group and level of detail. A known kind that takes
    /// none has only its base part; a kind this reader does not know may have parts.
    #[must_use]
    pub fn takes_parts(self) -> bool {
        matches!(self, Self::NAMES | Self::SEARCH | Self::SUBSTRING)
            || self.is_outline()
            || self.name().is_none()
    }

    /// Whether this is an outline kind, whose parts are keyed [`Part::outline`] and read
    /// together from one part (ADR-0040).
    #[must_use]
    pub const fn is_outline(self) -> bool {
        matches!(
            self,
            Self::ARCS
                | Self::ARCS_OVERVIEW
                | Self::POLYGONS
                | Self::POLY_INDEX
                | Self::ADMIN_REGION
        )
    }

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

/// Which part of its kind a section is (ADR-0040). A kind may appear more than once in a pack,
/// each section a part with its own key; the base part, 0, is the only one of a kind that
/// takes no parts and comes first of its kind. What a key means is fixed per kind: a language
/// packed as NAMES' `lang` column packs it ([`Part::language`]) for NAMES, SEARCH and
/// SUBSTRING; the deepest admin level held and a level of detail ([`Part::outline`]) for the
/// outlines, each part a whole copy for those levels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Part(pub u32);

impl Part {
    /// The base part: today's single section of a kind.
    pub const BASE: Self = Self(0);
    /// The names of every language without a part of its own, keyed `*`.
    pub const REST: Self = Self(0x2a);

    /// The part of language `tag` (its primary subtag, 2 to 4 ASCII letters, folded to lower
    /// case and packed little-endian as NAMES' `lang` column); `None` for any other text.
    #[must_use]
    pub fn language(tag: &str) -> Option<Self> {
        let primary = tag.split(['-', '_']).next().unwrap_or_default();
        if !(2..=4).contains(&primary.len()) || !primary.bytes().all(|b| b.is_ascii_alphabetic()) {
            return None;
        }
        let mut v = 0_u32;
        for (i, b) in primary.bytes().enumerate() {
            v |= u32::from(b.to_ascii_lowercase()) << (8 * i);
        }
        Some(Self(v))
    }

    /// The deepest admin level an outline part may hold: its key's low byte stays below
    /// printable ASCII, so no outline key reads as [`Part::REST`] or a language.
    pub const MAX_OUTLINE_LEVEL: u8 = 30;

    /// The outline part that holds every admin unit of level `deepest` and coarser, whole, at
    /// level of detail `lod`: `(deepest + 1) | lod << 8`, never the base part, which holds every
    /// level at full detail. `None` past [`Part::MAX_OUTLINE_LEVEL`] or a lod past
    /// [`arcs::PINNED`]` - 1`.
    #[must_use]
    pub const fn outline(deepest: u8, lod: u8) -> Option<Self> {
        if deepest > Self::MAX_OUTLINE_LEVEL || lod >= arcs::PINNED {
            return None;
        }
        Some(Self((deepest as u32 + 1) | (lod as u32) << 8))
    }

    /// The deepest admin level and the level of detail of an outline part; `None` for the base
    /// part and any key [`Part::outline`] does not make.
    #[must_use]
    pub const fn outline_levels(self) -> Option<(u8, u8)> {
        let [low, lod, a, b] = self.0.to_le_bytes();
        if low == 0 || low > Self::MAX_OUTLINE_LEVEL + 1 || lod >= arcs::PINNED || a != 0 || b != 0
        {
            return None;
        }
        Some((low - 1, lod))
    }

    /// Whether this is the base part.
    #[must_use]
    pub const fn is_base(self) -> bool {
        self.0 == 0
    }
}

impl Part {
    /// The language of a language part, as its primary subtag (`ne`); `None` for the base
    /// part, [`Part::REST`] or any key that is not 2 to 4 packed ASCII lower-case letters.
    #[must_use]
    pub fn language_code(self) -> Option<[u8; 4]> {
        let bytes = self.0.to_le_bytes();
        let len = bytes.iter().position(|&b| b == 0).unwrap_or(4);
        let (word, pad) = bytes.split_at(len);
        (len >= 2 && word.iter().all(u8::is_ascii_lowercase) && pad.iter().all(|&b| b == 0))
            .then_some(bytes)
    }
}

/// A section's kind and part for people (reports, tables): `NAMES`, NAMES of the `ne` part,
/// `ARCS to level 2`, `ARCS to level 2 at lod 3`.
#[derive(Clone, Copy, Debug)]
pub struct SectionLabel(pub Kind, pub Part);

impl fmt::Display for SectionLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self(kind, part) = *self;
        match part.outline_levels().filter(|_| kind.is_outline()) {
            _ if part.is_base() => write!(f, "{kind}"),
            Some((deepest, 0)) => write!(f, "{kind} to level {deepest}"),
            Some((deepest, lod)) => write!(f, "{kind} to level {deepest} at lod {lod}"),
            None => write!(f, "{kind} `{part}`"),
        }
    }
}

/// `base`, `*` for [`Part::REST`], the language of a language part (`ne`), else the key in hex.
impl fmt::Display for Part {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_base() {
            return f.write_str("base");
        }
        if *self == Self::REST {
            return f.write_str("*");
        }
        match self.language_code() {
            Some(code) => {
                for b in code.iter().take_while(|&&b| b != 0) {
                    write!(f, "{}", char::from(*b))?;
                }
                Ok(())
            }
            None => write!(f, "{:#x}", self.0),
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
    use super::{KINDS, Kind, Part};

    #[test]
    fn a_part_names_its_language() {
        use std::string::ToString;
        assert_eq!(Part::BASE.to_string(), "base");
        assert_eq!(Part::REST.to_string(), "*");
        assert_eq!(Part::language("ne-Deva").unwrap().to_string(), "ne");
        assert_eq!(Part::language("MAI").unwrap().to_string(), "mai");
        let outline = |d, l| Part::outline(d, l).unwrap();
        assert_eq!(outline(3, 1).to_string(), "0x104");
        assert_eq!(outline(0, 0).outline_levels(), Some((0, 0)));
        assert_eq!(outline(30, 6).outline_levels(), Some((30, 6)));
        assert_ne!(outline(0, 0), Part::BASE);
        assert_eq!(Part::BASE.outline_levels(), None);
        // No outline key reads as another kind's: `*`, a language, a lod past the levels.
        assert_eq!((Part::outline(31, 0), Part::outline(0, 7)), (None, None));
        assert_eq!(Part::REST.outline_levels(), None);
        assert_eq!(Part::language("ne").unwrap().outline_levels(), None);
        assert_eq!(Part(0x0701).outline_levels(), None);
    }

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
