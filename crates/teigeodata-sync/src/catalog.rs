//! The catalog's pack details: schema v1 of a target's `custom` object (D-160).
//!
//! The signed targets metadata names one target per pack, at its logical path
//! `address.component.tgd` (ADR-0010). TUF's `length` and `hashes` are the transport asset's,
//! the file a client downloads; `custom` says what the asset holds, so a client can plan an
//! install, show its size and check for updates before it fetches a byte:
//!
//! ```json
//! "np.places.tgd": {
//!   "length": 8227234,
//!   "hashes": { "sha256": "1a2b3c4d…" },
//!   "custom": {
//!     "schema": 1, "address": "np", "component": "places", "release": "2026.11.0",
//!     "file": "np.places.2026.11.0.1a2b3c4d.tgd.zst",
//!     "pack": { "length": 11748636, "head": "…32 hex…" },
//!     "transport": { "encoding": "zstd-seekable", "seek_table": 293, "head": [512, 1024] },
//!     "block_hashes": { "size": [6100, 6144], "for": ["timezone-index"] },
//!     "sizes": { "point": [522192, 601344], "names": [917602, 1302528] },
//!     "deps": [], "min_reader": 0,
//!     "bbox": [801000000, 263000000, 882000000, 304500000], "licence": "ODbL-1.0"
//!   }
//! }
//! ```
//!
//! - `file` is the asset's name on the host: release and the first 8 hex digits of the asset's
//!   SHA-256, so files are immutable and never overwritten (ADR-0010). It must agree with the
//!   target's own `sha256`, the one hash every TUF client knows (D-162).
//! - `pack.head` is the pack's 16-byte head hash (header bytes 112–127): equal heads mean equal
//!   packs, so an update with the installed head costs nothing.
//! - `sizes` gives, per component of the pack (D-135), its download bytes (the frames of its
//!   sections in the asset) and its disk bytes (the sections with their padding, D-172), so a
//!   whole install's parts add up to `pack.length`; unknown component names are
//!   skipped, for newer packs. Every install also fetches the head frame (`transport.head`) and
//!   the seek table, and `BLOCK_HASHES` (`block_hashes`, absent when the pack has none) when it
//!   holds one of the components that need it (a section of 1 MiB or more). [`Entry::size_of`]
//!   adds them as G-wire does (D-162).
//! - `parts` (optional, D-253) lists, per component, each part of a split kind but its base
//!   (ADR-0040) as `[key, wire, disk]`, or `[key, wire, disk, tolerance_m]` for a coarser
//!   outline part: `{"names": [[42, 185528, 361024], [25966, 493174, 1015552]]}`. The key is
//!   the `Part` number. A component's `sizes` holds every part, so its base is the rest;
//!   [`Entry::size_of`] counts only the parts a [`Selection`] holds. Unknown component names
//!   are skipped.
//! - `bbox` is `[west, south, east, north]` in units of 1e-7 degree: canonical JSON has no
//!   floats. East is below west when the box crosses the antimeridian.
//! - Unknown fields are ignored, so v1 may grow; a change that old clients would misread raises
//!   `schema`, and a client refuses a schema it does not know.
//!
//! The checked-in JSON schema is `schema/catalog-custom-v1.json`; this parser is the client's
//! validator and is held to the schema by the tests.

use alloc::borrow::ToOwned;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use serde_json::{Map, Value, json};
use teigeodata_core::Point;
use teigeodata_format::component::{Component, Components, Parts, Selection};
use teigeodata_format::{Bbox, FOOTER_SIZE, Part, name_like};

use crate::trust::{Target, lower_hex};

mod find;
pub use find::{Candidate, Catalog, Found, Resolver};

/// The schema this reader knows.
pub const SCHEMA: u64 = 1;

/// The transport encoding of schema v1.
pub const ENCODING: &str = "zstd-seekable";

/// Why a target's `custom` object was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// No `custom` object.
    Missing,
    /// A schema this reader does not know.
    Schema(u64),
    /// A field missing, of the wrong type or out of range.
    Field(&'static str),
    /// The path, `file` or `sha256` disagree with each other.
    Name(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("the target has no custom object"),
            Self::Schema(s) => write!(f, "catalog schema {s} is not known (this reader knows 1)"),
            Self::Field(k) => write!(f, "catalog field {k} is missing or invalid"),
            Self::Name(why) => write!(f, "catalog names disagree: {why}"),
        }
    }
}

impl core::error::Error for Error {}

/// One pack in the catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Its address: `np`, `in-br`, `in-br-t07`.
    pub address: String,
    /// The pack's component in the ADR-0010 sense: `places`, `dem`, `boundary-lod1`.
    pub component: String,
    /// Its release: `2026.11.0`.
    pub release: String,
    /// The asset's name on the host.
    pub file: String,
    /// The installed pack's length in bytes.
    pub pack_length: u64,
    /// The pack's head hash.
    pub head: [u8; 16],
    /// The seek table's length in bytes, at the asset's end.
    pub seek_table: u64,
    /// The head frame: its bytes in the asset and the head's bytes on disk.
    pub head_frame: Size,
    /// `BLOCK_HASHES`, when the pack has it.
    pub block_hashes: Option<BlockHashes>,
    /// Per component of the pack: download bytes and disk bytes.
    pub sizes: BTreeMap<Component, Size>,
    /// Each part of a split kind but its base (D-253), by component then part; empty for a
    /// pack without parts.
    pub parts: Vec<PartSize>,
    /// The logical paths of the packs this one needs.
    pub deps: Vec<String>,
    /// The lowest format major that reads it.
    pub min_reader: u16,
    /// Its bounding box, when it covers an area.
    pub bbox: Option<Bbox>,
    /// The SPDX identifier of its data licence.
    pub licence: String,
}

/// A component's bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    /// Bytes downloaded: its frames in the asset.
    pub wire: u64,
    /// Bytes on disk: its sections in the pack, each with its padding to the next (D-172).
    pub disk: u64,
}

/// One part of a split kind in a pack (ADR-0040, D-253): its component, its key, its bytes,
/// and for an outline part at a coarser level of detail that level's tolerance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartSize {
    /// The component of the part's sections.
    pub component: Component,
    /// The part's key.
    pub part: Part,
    /// Its download and disk bytes.
    pub size: Size,
    /// For a coarser outline part, its tolerance in metres; 0 otherwise.
    pub tolerance_m: u32,
}

/// The `BLOCK_HASHES` section: its bytes and the components that need it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockHashes {
    /// Its bytes.
    pub size: Size,
    /// The components with a section of 1 MiB or more.
    pub needed_by: Components,
}

/// The country of a pack's address: its two lower-case letters when the address is a country
/// (`np`) or a part of one (`ae-du`); `None` for any other address, whose packs belong to no
/// country. The catalog is delegated by it (D-175).
#[must_use]
pub fn country_of(address: &str) -> Option<[u8; 2]> {
    match address.as_bytes() {
        [a, b, rest @ ..]
            if a.is_ascii_lowercase()
                && b.is_ascii_lowercase()
                && (rest.is_empty() || rest.first() == Some(&b'-')) =>
        {
            Some([*a, *b])
        }
        _ => None,
    }
}

/// The logical path of a pack: `address.component.tgd`.
#[must_use]
pub fn path(address: &str, component: &str) -> String {
    alloc::format!("{address}.{component}.tgd")
}

/// The asset's name: `address.component.release.hash8.tgd.zst`.
#[must_use]
pub fn file_name(address: &str, component: &str, release: &str, asset_sha256: &[u8]) -> String {
    let hash8 = lower_hex(asset_sha256.get(..4).unwrap_or(asset_sha256));
    alloc::format!("{address}.{component}.{release}.{hash8}.tgd.zst")
}

impl Entry {
    /// Reads the entry of the target at `path`, and checks it against the target.
    ///
    /// # Errors
    /// [`Error`] for a missing or malformed entry, an unknown schema, or names that disagree
    /// with the path or the target's SHA-256.
    pub fn from_target(target_path: &str, target: &Target) -> Result<Self, Error> {
        let c = target
            .custom
            .as_ref()
            .and_then(Value::as_object)
            .ok_or(Error::Missing)?;
        let schema = c
            .get("schema")
            .and_then(Value::as_u64)
            .ok_or(Error::Field("schema"))?;
        if schema != SCHEMA {
            return Err(Error::Schema(schema));
        }
        let address = ident(c, "address")?;
        let component = ident(c, "component")?;
        let release = release(c)?;
        if target_path != path(&address, &component) {
            return Err(Error::Name("the path is not address.component.tgd"));
        }
        let sha256 = target
            .hashes
            .get("sha256")
            .ok_or(Error::Name("the target has no sha256"))?;
        let file = text(c, "file")?;
        if file != file_name(&address, &component, &release, sha256) {
            return Err(Error::Name(
                "file is not address.component.release.hash8.tgd.zst",
            ));
        }
        let pack = obj(c, "pack")?;
        let transport = obj(c, "transport")?;
        if transport.get("encoding").and_then(Value::as_str) != Some(ENCODING) {
            return Err(Error::Field("transport.encoding"));
        }
        let entry = Self {
            pack_length: u(pack, "pack.length", "length")?,
            head: head(pack)?,
            seek_table: u(transport, "transport.seek_table", "seek_table")?,
            head_frame: size(transport.get("head")).ok_or(Error::Field("transport.head"))?,
            block_hashes: block_hashes(c.get("block_hashes"))?,
            sizes: sizes(obj(c, "sizes")?)?,
            parts: part_sizes(c.get("parts"))?,
            deps: deps(c, target_path)?,
            min_reader: c
                .get("min_reader")
                .and_then(Value::as_u64)
                .and_then(|v| u16::try_from(v).ok())
                .ok_or(Error::Field("min_reader"))?,
            bbox: bbox(c.get("bbox"))?,
            licence: licence(c)?,
            address,
            component,
            release,
            file,
        };
        if entry.seek_table == 0 || entry.seek_table >= target.length {
            return Err(Error::Field("transport.seek_table"));
        }
        let fixed = entry.head_frame.wire.saturating_add(entry.seek_table);
        if entry.head_frame.wire == 0
            || fixed > target.length
            || entry.head_frame.disk > entry.pack_length
        {
            return Err(Error::Field("transport.head"));
        }
        if entry
            .block_hashes
            .is_some_and(|b| b.size.wire > target.length || b.size.disk > entry.pack_length)
        {
            return Err(Error::Field("block_hashes"));
        }
        if entry.sizes.is_empty()
            || entry
                .sizes
                .values()
                .any(|s| s.wire > target.length || s.disk > entry.pack_length)
        {
            return Err(Error::Field("sizes"));
        }
        // The parts of a whole install cannot outgrow the pack (equal unless a newer pack has
        // components this reader skips).
        if entry.size_of(Components::all()).disk > entry.pack_length {
            return Err(Error::Field("sizes"));
        }
        Ok(entry)
    }

    /// The entry's logical path.
    #[must_use]
    pub fn path(&self) -> String {
        path(&self.address, &self.component)
    }

    /// The `custom` object, as release tooling writes it.
    #[must_use]
    pub fn to_custom(&self) -> Value {
        let sizes: Map<String, Value> = self
            .sizes
            .iter()
            .map(|(c, s)| (c.name().to_owned(), json!([s.wire, s.disk])))
            .collect();
        let mut v = json!({
            "schema": SCHEMA,
            "address": self.address,
            "component": self.component,
            "release": self.release,
            "file": self.file,
            "pack": {"length": self.pack_length, "head": lower_hex(&self.head)},
            "transport": {
                "encoding": ENCODING,
                "seek_table": self.seek_table,
                "head": [self.head_frame.wire, self.head_frame.disk],
            },
            "sizes": sizes,
            "deps": self.deps,
            "min_reader": self.min_reader,
            "licence": self.licence,
        });
        if let (Some(b), Value::Object(m)) = (self.block_hashes, &mut v) {
            let names: Vec<&str> = b.needed_by.iter().map(Component::name).collect();
            m.insert(
                "block_hashes".to_owned(),
                json!({"size": [b.size.wire, b.size.disk], "for": names}),
            );
        }
        if let (false, Value::Object(m)) = (self.parts.is_empty(), &mut v) {
            let mut by: Map<String, Value> = Map::new();
            for p in &self.parts {
                let mut row = alloc::vec![
                    Value::from(p.part.0),
                    p.size.wire.into(),
                    p.size.disk.into()
                ];
                if p.tolerance_m > 0 {
                    row.push(p.tolerance_m.into());
                }
                if let Value::Array(list) = by
                    .entry(p.component.name())
                    .or_insert_with(|| Value::Array(Vec::new()))
                {
                    list.push(Value::Array(row));
                }
            }
            m.insert("parts".to_owned(), Value::Object(by));
        }
        if let (Some(b), Value::Object(m)) = (self.bbox, &mut v) {
            m.insert(
                "bbox".to_owned(),
                json!([b.min.lon(), b.min.lat(), b.max.lon(), b.max.lat()]),
            );
        }
        v
    }

    /// Download and disk bytes of an install of `want`, its components closed by what each
    /// needs: the head frame and the seek table, the frames of the parts it holds of those
    /// components (D-253), and `BLOCK_HASHES` when one of them needs it; on disk the head, the
    /// sections, `BLOCK_HASHES` and the footer. Components the pack lacks count nothing. A
    /// [`Components`] holds every part.
    #[must_use]
    pub fn size_of(&self, want: impl Into<Selection>) -> Size {
        let want = want.into().closed();
        let parts = self.resolve(want.parts);
        let fixed = Size {
            wire: self.head_frame.wire.saturating_add(self.seek_table),
            disk: self.head_frame.disk.saturating_add(FOOTER_SIZE as u64),
        };
        let own = self
            .sizes
            .iter()
            .filter(|(c, _)| want.set.contains(**c))
            .fold(fixed, |a, (&c, &s)| a.plus(self.held(c, s, parts)));
        match self.block_hashes {
            Some(b) if want.set.iter().any(|c| b.needed_by.contains(c)) => own.plus(b.size),
            _ => own,
        }
    }

    /// `parts` on this pack: each language asked for as the part that holds it.
    #[must_use]
    pub fn resolve(&self, parts: Parts) -> Parts {
        parts.resolve(|p| {
            self.parts
                .iter()
                .any(|q| q.component == Component::Names && q.part == p)
        })
    }

    /// Of component `c`, `total` bytes with every part, the bytes of the parts `parts` holds:
    /// its base (the total less the listed parts) when held, and each listed part held.
    fn held(&self, c: Component, total: Size, parts: Parts) -> Size {
        let Some(&kind) = c.kinds().first() else {
            return total;
        };
        let listed = self.parts.iter().filter(|p| p.component == c);
        let all = listed.clone().fold(Size::ZERO, |a, p| a.plus(p.size));
        let base = Size {
            wire: total.wire.saturating_sub(all.wire),
            disk: total.disk.saturating_sub(all.disk),
        };
        let start = if parts.holds(kind, Part::BASE) {
            base
        } else {
            Size::ZERO
        };
        listed
            .filter(|p| parts.holds(kind, p.part))
            .fold(start, |a, p| a.plus(p.size))
    }
}

impl Size {
    /// No bytes.
    pub const ZERO: Self = Self { wire: 0, disk: 0 };

    /// Both sums, saturating.
    #[must_use]
    pub const fn plus(self, o: Self) -> Self {
        Self {
            wire: self.wire.saturating_add(o.wire),
            disk: self.disk.saturating_add(o.disk),
        }
    }
}

fn obj<'a>(m: &'a Map<String, Value>, k: &'static str) -> Result<&'a Map<String, Value>, Error> {
    m.get(k).and_then(Value::as_object).ok_or(Error::Field(k))
}

fn text(m: &Map<String, Value>, k: &'static str) -> Result<String, Error> {
    m.get(k)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or(Error::Field(k))
}

fn u(m: &Map<String, Value>, name: &'static str, k: &str) -> Result<u64, Error> {
    m.get(k).and_then(Value::as_u64).ok_or(Error::Field(name))
}

fn ident(m: &Map<String, Value>, k: &'static str) -> Result<String, Error> {
    let s = text(m, k)?;
    if name_like(s.as_bytes(), false) {
        Ok(s)
    } else {
        Err(Error::Field(k))
    }
}

fn release(m: &Map<String, Value>) -> Result<String, Error> {
    let s = text(m, "release")?;
    if name_like(s.as_bytes(), true) {
        Ok(s)
    } else {
        Err(Error::Field("release"))
    }
}

fn head(pack: &Map<String, Value>) -> Result<[u8; 16], Error> {
    let s = pack
        .get("head")
        .and_then(Value::as_str)
        .ok_or(Error::Field("pack.head"))?;
    let bad = Error::Field("pack.head");
    if s.len() != 32 {
        return Err(bad);
    }
    let mut out = [0_u8; 16];
    for (o, pair) in out.iter_mut().zip(s.as_bytes().as_chunks::<2>().0) {
        let nib = |c: u8| match c {
            b'0'..=b'9' => Ok(c - b'0'),
            b'a'..=b'f' => Ok(c - b'a' + 10),
            _ => Err(bad),
        };
        *o = (nib(pair[0])? << 4) | nib(pair[1])?;
    }
    Ok(out)
}

/// `[wire, disk]`.
fn size(v: Option<&Value>) -> Option<Size> {
    match v?.as_array()?.as_slice() {
        [w, d] => Some(Size {
            wire: w.as_u64()?,
            disk: d.as_u64()?,
        }),
        _ => None,
    }
}

fn block_hashes(v: Option<&Value>) -> Result<Option<BlockHashes>, Error> {
    let Some(v) = v else { return Ok(None) };
    let bad = Error::Field("block_hashes");
    let m = v.as_object().ok_or(bad)?;
    let size = size(m.get("size")).ok_or(bad)?;
    let list = m.get("for").and_then(Value::as_array).ok_or(bad)?;
    if list.is_empty() {
        return Err(bad);
    }
    let mut needed_by = Components::NONE;
    for name in list {
        // Unknown names are skipped, as in `sizes`.
        if let Some(c) = Component::from_name(name.as_str().ok_or(bad)?) {
            needed_by = needed_by.union(Components::of(&[c]));
        }
    }
    Ok(Some(BlockHashes { size, needed_by }))
}

fn sizes(m: &Map<String, Value>) -> Result<BTreeMap<Component, Size>, Error> {
    let mut out = BTreeMap::new();
    for (name, v) in m {
        let pair = size(Some(v)).ok_or(Error::Field("sizes"))?;
        if let Some(c) = Component::from_name(name) {
            out.insert(c, pair);
        }
    }
    Ok(out)
}

/// The optional `parts` object; a component this reader does not know is skipped.
fn part_sizes(v: Option<&Value>) -> Result<Vec<PartSize>, Error> {
    let Some(v) = v else {
        return Ok(Vec::new());
    };
    let bad = Error::Field("parts");
    let mut out = Vec::new();
    for (name, rows) in v.as_object().ok_or(bad)? {
        let rows = rows.as_array().ok_or(bad)?;
        let Some(component) = Component::from_name(name) else {
            continue;
        };
        for row in rows {
            let n = |i: usize| row.get(i).and_then(Value::as_u64);
            let (Some(key), Some(wire), Some(disk)) = (n(0), n(1), n(2)) else {
                return Err(bad);
            };
            let len = row.as_array().map_or(0, Vec::len);
            let tolerance_m = match (len, n(3)) {
                (3, _) => 0,
                (4, Some(t)) => u32::try_from(t).ok().filter(|&t| t > 0).ok_or(bad)?,
                _ => return Err(bad),
            };
            out.push(PartSize {
                component,
                part: Part(u32::try_from(key).ok().filter(|&k| k != 0).ok_or(bad)?),
                size: Size { wire, disk },
                tolerance_m,
            });
        }
    }
    // By component, then part: the order the measure gives.
    out.sort_unstable_by_key(|p| (p.component, p.part));
    Ok(out)
}

fn deps(m: &Map<String, Value>, own: &str) -> Result<Vec<String>, Error> {
    let list = m
        .get("deps")
        .and_then(Value::as_array)
        .ok_or(Error::Field("deps"))?;
    let mut out: Vec<String> = Vec::with_capacity(list.len());
    for d in list {
        let s = d.as_str().ok_or(Error::Field("deps"))?;
        let ok = s
            .strip_suffix(".tgd")
            .and_then(|r| r.split_once('.'))
            .is_some_and(|(a, c)| name_like(a.as_bytes(), false) && name_like(c.as_bytes(), false));
        if !ok || s == own || out.iter().any(|o| o == s) {
            return Err(Error::Field("deps"));
        }
        out.push(s.to_owned());
    }
    Ok(out)
}

fn bbox(v: Option<&Value>) -> Result<Option<Bbox>, Error> {
    let Some(v) = v else { return Ok(None) };
    let bad = Error::Field("bbox");
    let n: Vec<i32> = v
        .as_array()
        .ok_or(bad)?
        .iter()
        .map(|x| x.as_i64().and_then(|i| i32::try_from(i).ok()).ok_or(bad))
        .collect::<Result<_, _>>()?;
    let &[west, south, east, north] = n.as_slice() else {
        return Err(bad);
    };
    let min = Point::from_units(south, west).ok_or(bad)?;
    let max = Point::from_units(north, east).ok_or(bad)?;
    if south > north {
        return Err(bad);
    }
    Ok(Some(Bbox { min, max }))
}

/// An SPDX licence identifier: letters, digits, `.`, `-` and `+`.
fn licence(m: &Map<String, Value>) -> Result<String, Error> {
    let s = text(m, "licence")?;
    let ok = (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'));
    if ok {
        Ok(s)
    } else {
        Err(Error::Field("licence"))
    }
}

#[cfg(test)]
mod tests;
