//! The manifest: our own canonical TLV (D-071, the [pack format](https://github.com/teispace/teigeodata/blob/main/docs/04-architecture/pack-format.md#manifest)).
//!
//! - An entry is a tag (u16 LE), a length (u32 LE) and that many value bytes.
//! - Tags are strictly increasing: one encoding per manifest.
//! - An unknown tag is skipped, unless its high bit (`0x8000`, "must understand") is set; then the
//!   pack is refused.
//! - The value of every known tag is checked when the manifest is parsed, so the typed getters
//!   never fail on a parsed manifest.

use core::fmt;

use teigeodata_core::{E7, Point};

use crate::Error;

/// A manifest tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tag(pub u16);

/// How a known tag's value is checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Value {
    /// 1–64 bytes of `a-z`, `0-9` and `-`, starting with a letter or digit: safe in file names.
    Ident,
    /// As [`Value::Ident`], and dots are allowed: `2026.11.0`.
    Release,
    /// A u32, little-endian.
    U32,
    /// UTF-8 text of 1 to [`MAX_TEXT`] bytes with no control character but the line feed.
    Text,
    /// Four i32 on the 1e-7° grid: min lat, min lon, max lat, max lon. `min lat ≤ max lat`; a
    /// min lon above the max lon means the box crosses the antimeridian.
    Bbox,
}

impl Tag {
    /// The pack's address: `np`, `ae-du`, `in-br-t07` (ADR-0010). Required.
    pub const ADDRESS: Self = Self(0x0001);
    /// The component: `places`, `boundary-lod1`. Required.
    pub const COMPONENT: Self = Self(0x0002);
    /// The release: `2026.11.0`. Required.
    pub const RELEASE: Self = Self(0x0003);
    /// The data schema version (u32).
    pub const SCHEMA: Self = Self(0x0004);
    /// The licence: `odbl` (main build) or `cc-by-4.0` (lite build).
    pub const LICENCE: Self = Self(0x0005);
    /// The bounding box of the pack's coverage.
    pub const BBOX: Self = Self(0x0006);
    /// The default perspective for disputed boundaries (ADR-0006): a country code.
    pub const PERSPECTIVE: Self = Self(0x0007);
    /// The notices of the sources the pack was built from (D-031, D-126): UTF-8 lines, one
    /// notice each, that an application shows wherever it shows the data.
    pub const ATTRIBUTION: Self = Self(0x0008);

    /// The "must understand" bit.
    pub const CRITICAL: u16 = 0x8000;

    /// Whether a reader that does not know this tag must refuse the pack.
    #[must_use]
    pub const fn is_critical(self) -> bool {
        self.0 & Self::CRITICAL != 0
    }

    /// This reader's name for the tag, if it knows it.
    #[must_use]
    pub fn name(self) -> Option<&'static str> {
        known(self).map(|(_, name, _)| *name)
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(n) => f.write_str(n),
            None => write!(f, "0x{:04x}", self.0),
        }
    }
}

/// Every tag this reader knows: the single list the reader, the writer and the docs test use.
pub(crate) const KNOWN: &[(Tag, &str, Value)] = &[
    (Tag::ADDRESS, "address", Value::Ident),
    (Tag::COMPONENT, "component", Value::Ident),
    (Tag::RELEASE, "release", Value::Release),
    (Tag::SCHEMA, "schema", Value::U32),
    (Tag::LICENCE, "licence", Value::Release),
    (Tag::BBOX, "bbox", Value::Bbox),
    (Tag::PERSPECTIVE, "perspective", Value::Ident),
    (Tag::ATTRIBUTION, "attribution", Value::Text),
];

/// The longest text value: half the head's 16 KiB, so the manifest keeps room for the table and
/// the signature.
pub const MAX_TEXT: usize = 8 * 1024;

/// Tags every pack must have: they name it and derive its UUID.
pub(crate) const REQUIRED: [Tag; 3] = [Tag::ADDRESS, Tag::COMPONENT, Tag::RELEASE];

fn known(tag: Tag) -> Option<&'static (Tag, &'static str, Value)> {
    KNOWN.iter().find(|(t, _, _)| *t == tag)
}

/// Whether `v` is a name as the manifest holds an address or component (`dots` false) or a
/// release (`dots` true): 1–64 bytes of `a-z`, `0-9` and `-` (and `.`), starting with a letter
/// or digit.
#[must_use]
pub fn name_like(v: &[u8], dots: bool) -> bool {
    (1..=64).contains(&v.len())
        && v.first().is_some_and(u8::is_ascii_alphanumeric)
        && v.iter().all(|&c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || (dots && c == b'.')
        })
}

fn i32_at(v: &[u8], i: usize) -> Option<i32> {
    let b = v.get(i * 4..i * 4 + 4)?;
    Some(i32::from_le_bytes(b.try_into().ok()?))
}

/// Checks a value against its tag, if the tag is known. Shared by the reader and the writer.
#[allow(clippy::many_single_char_names, reason = "the four corners of a box")]
pub(crate) fn check(tag: Tag, v: &[u8]) -> Result<(), &'static str> {
    if tag.0 & !Tag::CRITICAL == 0 {
        return Err("tag 0 is reserved");
    }
    let Some((_, _, kind)) = known(tag) else {
        return if tag.is_critical() {
            Err("an unknown tag is marked critical: this reader cannot use the pack")
        } else {
            Ok(())
        };
    };
    let ok = match kind {
        Value::Ident => name_like(v, false),
        Value::Release => name_like(v, true),
        Value::U32 => v.len() == 4,
        Value::Text => {
            (1..=MAX_TEXT).contains(&v.len())
                && core::str::from_utf8(v)
                    .is_ok_and(|t| t.chars().all(|c| c == '\n' || !c.is_control()))
        }
        Value::Bbox => {
            v.len() == 16 && {
                let c = [0, 1, 2, 3].map(|i| i32_at(v, i));
                match c {
                    [Some(a), Some(b), Some(c), Some(d)] => {
                        Point::<E7>::from_units(a, b).is_some()
                            && Point::<E7>::from_units(c, d).is_some()
                            && a <= c
                    }
                    _ => false,
                }
            }
        }
    };
    if ok {
        Ok(())
    } else {
        Err(match kind {
            Value::Ident => "the value is not 1-64 characters of a-z, 0-9 and -",
            Value::Release => "the value is not 1-64 characters of a-z, 0-9, . and -",
            Value::U32 => "the value is not 4 bytes",
            Value::Text => "the text is not 1-8192 bytes of UTF-8 without control characters",
            Value::Bbox => "the bbox is not 16 bytes of an in-range box on the 1e-7 grid",
        })
    }
}

/// A coverage box on the 1e-7° grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bbox {
    /// South-west corner.
    pub min: Point<E7>,
    /// North-east corner. Its longitude is below `min`'s when the box crosses the antimeridian.
    pub max: Point<E7>,
}

impl Bbox {
    /// The 16-byte manifest value.
    #[must_use]
    pub fn to_bytes(self) -> [u8; 16] {
        let mut out = [0; 16];
        let parts = [
            self.min.lat(),
            self.min.lon(),
            self.max.lat(),
            self.max.lon(),
        ];
        for (chunk, v) in out.as_chunks_mut::<4>().0.iter_mut().zip(parts) {
            chunk.copy_from_slice(&v.to_le_bytes());
        }
        out
    }
}

/// A parsed, validated manifest. Borrows the pack's bytes.
#[derive(Clone, Copy, Debug)]
pub struct Manifest<'a> {
    bytes: &'a [u8],
}

/// An iterator over a manifest's entries, in tag order.
#[derive(Clone, Debug)]
pub struct Entries<'a> {
    rest: &'a [u8],
}

/// Splits one entry off the front: `(tag, value, rest)`.
fn split(b: &[u8]) -> Option<(Tag, &[u8], &[u8])> {
    let tag = u16::from_le_bytes(b.get(..2)?.try_into().ok()?);
    let len = u32::from_le_bytes(b.get(2..6)?.try_into().ok()?);
    let end = 6_usize.checked_add(usize::try_from(len).ok()?)?;
    Some((Tag(tag), b.get(6..end)?, b.get(end..)?))
}

impl<'a> Iterator for Entries<'a> {
    type Item = (Tag, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let (tag, value, rest) = split(self.rest)?;
        self.rest = rest;
        Some((tag, value))
    }
}

impl<'a> Manifest<'a> {
    /// Parses and validates a manifest.
    ///
    /// # Errors
    /// [`Error::Manifest`] for a truncated entry, tags out of order or repeated, a known tag
    /// with a bad value, an unknown critical tag, or a missing required tag.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        let err = |at: usize, problem| Error::Manifest {
            offset: u32::try_from(at).unwrap_or(u32::MAX),
            problem,
        };
        let mut rest = bytes;
        let mut prev: Option<Tag> = None;
        let mut required = 0;
        while !rest.is_empty() {
            let at = bytes.len() - rest.len();
            let (tag, value, next) = split(rest).ok_or_else(|| err(at, "truncated entry"))?;
            if prev.is_some_and(|p| p >= tag) {
                return Err(err(at, "tags are not strictly increasing"));
            }
            check(tag, value).map_err(|p| err(at, p))?;
            if REQUIRED.contains(&tag) {
                required += 1;
            }
            prev = Some(tag);
            rest = next;
        }
        if required != REQUIRED.len() {
            return Err(err(
                bytes.len(),
                "address, component and release are required",
            ));
        }
        Ok(Self { bytes })
    }

    /// The raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The entries in tag order.
    #[must_use]
    pub const fn entries(&self) -> Entries<'a> {
        Entries { rest: self.bytes }
    }

    /// A tag's raw value.
    #[must_use]
    pub fn get(&self, tag: Tag) -> Option<&'a [u8]> {
        // Tags are sorted, so the scan can stop early.
        self.entries()
            .take_while(|(t, _)| *t <= tag)
            .find(|(t, _)| *t == tag)
            .map(|(_, v)| v)
    }

    /// A tag's value as UTF-8 text.
    #[must_use]
    pub fn text(&self, tag: Tag) -> Option<&'a str> {
        self.get(tag).and_then(|v| core::str::from_utf8(v).ok())
    }

    /// The address. Always present in a parsed manifest.
    #[must_use]
    pub fn address(&self) -> &'a str {
        self.text(Tag::ADDRESS).unwrap_or_default()
    }

    /// The component. Always present in a parsed manifest.
    #[must_use]
    pub fn component(&self) -> &'a str {
        self.text(Tag::COMPONENT).unwrap_or_default()
    }

    /// The release. Always present in a parsed manifest.
    #[must_use]
    pub fn release(&self) -> &'a str {
        self.text(Tag::RELEASE).unwrap_or_default()
    }

    /// The sources' notices ([`Tag::ATTRIBUTION`]), if present.
    #[must_use]
    pub fn attribution(&self) -> Option<&'a str> {
        self.text(Tag::ATTRIBUTION)
    }

    /// A u32 value, for tags of that type.
    #[must_use]
    pub fn u32(&self, tag: Tag) -> Option<u32> {
        self.get(tag)
            .and_then(|v| v.try_into().ok())
            .map(u32::from_le_bytes)
    }

    /// The coverage box, if present.
    #[must_use]
    pub fn bbox(&self) -> Option<Bbox> {
        let v = self.get(Tag::BBOX)?;
        let min = Point::from_units(i32_at(v, 0)?, i32_at(v, 1)?)?;
        let max = Point::from_units(i32_at(v, 2)?, i32_at(v, 3)?)?;
        Some(Bbox { min, max })
    }
}

/// Orders releases as dotted numbers (`2026.9.0 < 2026.11.0`): parts split at `.` and `-`,
/// numeric parts compared as numbers and before text parts, text parts as text.
#[must_use]
pub fn release_cmp(left: &str, right: &str) -> core::cmp::Ordering {
    let (mut pa, mut pb) = (left.split(['.', '-']), right.split(['.', '-']));
    loop {
        let (x, y) = match (pa.next(), pb.next()) {
            (Some(x), Some(y)) => (x, y),
            (None, None) => return core::cmp::Ordering::Equal,
            (None, Some(_)) => return core::cmp::Ordering::Less,
            (Some(_), None) => return core::cmp::Ordering::Greater,
        };
        let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(nx), Ok(ny)) => nx.cmp(&ny).then_with(|| x.len().cmp(&y.len())),
            (Ok(_), Err(_)) => core::cmp::Ordering::Less,
            (Err(_), Ok(_)) => core::cmp::Ordering::Greater,
            (Err(_), Err(_)) => x.cmp(y),
        };
        if ord != core::cmp::Ordering::Equal {
            return ord;
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::large_digit_groups,
    reason = "1e-7 grid values read as degrees, then seven digits: 26_0000000 is 26°"
)]
mod tests {
    use super::{KNOWN, Manifest, Tag, check};
    use crate::Error;
    use std::vec::Vec;

    fn tlv(entries: &[(u16, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (t, v) in entries {
            out.extend_from_slice(&t.to_le_bytes());
            out.extend_from_slice(&u32::try_from(v.len()).unwrap_or(0).to_le_bytes());
            out.extend_from_slice(v);
        }
        out
    }

    const ID: [(u16, &[u8]); 3] = [(1, b"np"), (2, b"places"), (3, b"2026.11.0")];

    fn with(extra: &[(u16, &[u8])]) -> Vec<u8> {
        let mut all: Vec<(u16, &[u8])> = ID.to_vec();
        all.extend_from_slice(extra);
        all.sort_by_key(|e| e.0);
        tlv(&all)
    }

    fn problem(b: &[u8]) -> Option<&'static str> {
        match Manifest::parse(b) {
            Err(Error::Manifest { problem, .. }) => Some(problem),
            _ => None,
        }
    }

    #[test]
    fn reads_known_values() {
        let bbox = [26_0000000_i32, 80_0000000, 30_5000000, 88_2000000]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>();
        let b = with(&[(4, &7_u32.to_le_bytes()), (5, b"odbl"), (6, &bbox)]);
        let m = Manifest::parse(&b);
        assert!(m.is_ok(), "{m:?}");
        if let Ok(m) = m {
            assert_eq!(
                (m.address(), m.component(), m.release()),
                ("np", "places", "2026.11.0")
            );
            assert_eq!(m.u32(Tag::SCHEMA), Some(7));
            assert_eq!(m.text(Tag::LICENCE), Some("odbl"));
            assert_eq!(m.bbox().map(|b| b.max.lon()), Some(88_2000000));
            assert_eq!(
                m.bbox().map(super::Bbox::to_bytes).as_ref().map(|b| &b[..]),
                Some(&bbox[..])
            );
            assert_eq!(m.get(Tag::PERSPECTIVE), None);
            assert_eq!(m.attribution(), None);
            assert_eq!(m.entries().count(), 6);
        }
        // A licence name has dots (D-126).
        let lite = with(&[(5, b"cc-by-4.0")]);
        assert_eq!(
            Manifest::parse(&lite)
                .ok()
                .and_then(|m| m.text(Tag::LICENCE)),
            Some("cc-by-4.0")
        );
        let notices = "© OpenStreetMap contributors (ODbL)\nGeoNames (CC BY 4.0)";
        let b = with(&[(8, notices.as_bytes())]);
        assert_eq!(
            Manifest::parse(&b).ok().and_then(|m| m.attribution()),
            Some(notices)
        );
    }

    #[test]
    fn attribution_is_bounded_text() {
        let long = "a".repeat(super::MAX_TEXT + 1);
        for bad in [&b""[..], b"\xff", b"tab\there", b"nul\0", long.as_bytes()] {
            assert_eq!(
                problem(&with(&[(8, bad)])),
                Some("the text is not 1-8192 bytes of UTF-8 without control characters"),
                "{bad:?}"
            );
        }
        let longest = "é".repeat(super::MAX_TEXT / 2);
        assert!(Manifest::parse(&with(&[(8, longest.as_bytes())])).is_ok());
    }

    #[test]
    fn unknown_tags_skip_unless_critical() {
        assert!(Manifest::parse(&with(&[(0x0100, b"\xff\x00anything")])).is_ok());
        assert_eq!(
            problem(&with(&[(0x8100, b"")])),
            Some("an unknown tag is marked critical: this reader cannot use the pack")
        );
    }

    #[test]
    fn rejects() {
        assert_eq!(
            problem(&tlv(&ID[..2])),
            Some("address, component and release are required")
        );
        assert_eq!(
            problem(&tlv(&[ID[1], ID[0], ID[2]])),
            Some("tags are not strictly increasing")
        );
        assert_eq!(
            problem(&tlv(&[ID[0], ID[0], ID[1], ID[2]])),
            Some("tags are not strictly increasing")
        );
        let mut cut = with(&[]);
        cut.pop();
        assert_eq!(problem(&cut), Some("truncated entry"));
        assert_eq!(problem(&with(&[])[..5]), Some("truncated entry"));
        for bad in [&b""[..], b"-np", b"NP", b"n p", b"n/p", b"n.p", &[b'a'; 65]] {
            assert!(
                problem(&tlv(&[(1, bad), ID[1], ID[2]])).is_some(),
                "{bad:?}"
            );
        }
        assert!(Manifest::parse(&tlv(&[ID[0], ID[1], (3, b"2026.11.0-rc.1")])).is_ok());
        assert!(problem(&with(&[(4, b"\x01\x02\x03")])).is_some());
        let inverted = [30_0000000_i32, 0, 20_0000000, 0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>();
        assert!(problem(&with(&[(6, &inverted)])).is_some());
        let off_grid = [0, 1_800_000_001_i32, 0, 0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>();
        assert!(problem(&with(&[(6, &off_grid)])).is_some());
        // Crossing the antimeridian is allowed: min lon above max lon.
        let fiji = [-21_0000000_i32, 177_0000000, -12_0000000, -178_0000000]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>();
        assert!(Manifest::parse(&with(&[(6, &fiji)])).is_ok());
    }

    #[test]
    fn known_tags_are_unique_and_not_critical() {
        for (i, (t, name, _)) in KNOWN.iter().enumerate() {
            assert!(!t.is_critical(), "{name}");
            assert!(
                KNOWN
                    .iter()
                    .skip(i + 1)
                    .all(|(u, n, _)| u != t && n != name)
            );
        }
        assert_eq!(check(Tag(0x7fff), b"x"), Ok(()));
        assert_eq!(check(Tag(0), b""), Err("tag 0 is reserved"));
        assert_eq!(check(Tag(0x8000), b""), Err("tag 0 is reserved"));
    }

    #[test]
    fn every_known_tag_is_documented() {
        let spec = include_str!("../../../docs/04-architecture/pack-format.md");
        for (t, name, _) in KNOWN {
            let row = std::format!("| `0x{:04x}` | `{name}` |", t.0);
            assert!(
                spec.contains(&row),
                "pack-format.md has no row starting {row}"
            );
        }
    }
}
