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
//!   "hashes": { "blake3": "1a2b3c4d…", "sha256": "…" },
//!   "custom": {
//!     "schema": 1, "address": "np", "component": "places", "release": "2026.11.0",
//!     "file": "np.places.2026.11.0.1a2b3c4d.tgd.zst",
//!     "pack": { "length": 11748636, "head": "…32 hex…" },
//!     "transport": { "encoding": "zstd-seekable", "seek_table": 293 },
//!     "sizes": { "point": [522192, 601344], "names": [917602, 1302528] },
//!     "deps": [], "min_reader": 0,
//!     "bbox": [801000000, 263000000, 882000000, 304500000], "licence": "ODbL-1.0"
//!   }
//! }
//! ```
//!
//! - `file` is the asset's name on the host: release and the first 8 hex digits of the asset's
//!   BLAKE3, so files are immutable and never overwritten (ADR-0010). It must agree with the
//!   target's own `blake3`.
//! - `pack.head` is the pack's 16-byte head hash (header bytes 112–127): equal heads mean equal
//!   packs, so an update with the installed head costs nothing.
//! - `sizes` gives, per component of the pack (D-135), its download bytes (the frames of its
//!   sections in the asset) and its disk bytes (the sections). A plan adds the components a set
//!   needs and the head; unknown component names are skipped, for newer packs.
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
use teigeodata_format::component::{Component, Components};
use teigeodata_format::{Bbox, name_like};

use crate::trust::{Target, lower_hex};

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
    /// The path, `file` or `blake3` disagree with each other.
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
    /// Per component of the pack: download bytes and disk bytes.
    pub sizes: BTreeMap<Component, Size>,
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
    /// Bytes on disk: its sections in the pack.
    pub disk: u64,
}

/// The logical path of a pack: `address.component.tgd`.
#[must_use]
pub fn path(address: &str, component: &str) -> String {
    alloc::format!("{address}.{component}.tgd")
}

/// The asset's name: `address.component.release.hash8.tgd.zst`.
#[must_use]
pub fn file_name(address: &str, component: &str, release: &str, asset_blake3: &[u8]) -> String {
    let hash8 = lower_hex(asset_blake3.get(..4).unwrap_or(asset_blake3));
    alloc::format!("{address}.{component}.{release}.{hash8}.tgd.zst")
}

impl Entry {
    /// Reads the entry of the target at `path`, and checks it against the target.
    ///
    /// # Errors
    /// [`Error`] for a missing or malformed entry, an unknown schema, or names that disagree
    /// with the path or the target's BLAKE3.
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
        let blake3 = target
            .hashes
            .get("blake3")
            .ok_or(Error::Name("the target has no blake3"))?;
        let file = text(c, "file")?;
        if file != file_name(&address, &component, &release, blake3) {
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
            sizes: sizes(obj(c, "sizes")?)?,
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
        if entry.sizes.is_empty()
            || entry
                .sizes
                .values()
                .any(|s| s.wire > target.length || s.disk > entry.pack_length)
        {
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
            "transport": {"encoding": ENCODING, "seek_table": self.seek_table},
            "sizes": sizes,
            "deps": self.deps,
            "min_reader": self.min_reader,
            "licence": self.licence,
        });
        if let (Some(b), Value::Object(m)) = (self.bbox, &mut v) {
            m.insert(
                "bbox".to_owned(),
                json!([b.min.lon(), b.min.lat(), b.max.lon(), b.max.lat()]),
            );
        }
        v
    }

    /// Download and disk bytes of an install with `set` (closed by what each needs).
    /// Components the pack lacks count nothing.
    #[must_use]
    pub fn size_of(&self, set: Components) -> Size {
        self.sizes
            .iter()
            .filter(|(c, _)| set.closed().contains(**c))
            .fold(Size { wire: 0, disk: 0 }, |a, (_, s)| Size {
                wire: a.wire + s.wire,
                disk: a.disk + s.disk,
            })
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

fn sizes(m: &Map<String, Value>) -> Result<BTreeMap<Component, Size>, Error> {
    let mut out = BTreeMap::new();
    for (name, v) in m {
        let pair = v
            .as_array()
            .and_then(|a| match a.as_slice() {
                [w, d] => Some(Size {
                    wire: w.as_u64()?,
                    disk: d.as_u64()?,
                }),
                _ => None,
            })
            .ok_or(Error::Field("sizes"))?;
        if let Some(c) = Component::from_name(name) {
            out.insert(c, pair);
        }
    }
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
