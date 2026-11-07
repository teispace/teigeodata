//! PLACES (kind 2): the core columns of every place, one row per place, rows in Hilbert order of
//! the point (D-074). A row number is the place's index in every other section.
//!
//! A [column table](teigeodata_core::table) with these columns:
//!
//! | ID | Column | Values | |
//! |---:|---|---|---|
//! | 1 | [`LAT`] | latitude on the 1e-6° grid, [biased](teigeodata_core::bitpack::bias) | required |
//! | 2 | [`LON`] | longitude, the same | required |
//! | 3 | [`KIND`] | [`PlaceKind`] code | required |
//! | 4 | [`ADMIN_LEVEL`] | admin level, 0 = country or none | optional, absent = all 0 |
//! | 5 | [`CLASS`] | class dictionary code, 0 = none | optional, absent = all 0 |
//! | 6 | [`FLAGS`] | flag bits, none defined yet | optional, absent = all 0 |
//!
//! [`Places::open`] checks the frame and every column's structure; [`Places::check`] also checks
//! every value (the forge and the tests run it). Reads never panic either way.

use teigeodata_core::bitpack::{Packed, unbias};
use teigeodata_core::table::Table;
use teigeodata_core::{E6, PlaceKind, Point, hilbert};

use crate::{Error, Kind, Section, columns};

/// Latitude column.
pub const LAT: u16 = 1;
/// Longitude column.
pub const LON: u16 = 2;
/// Kind column.
pub const KIND: u16 = 3;
/// Admin level column.
pub const ADMIN_LEVEL: u16 = 4;
/// Class column.
pub const CLASS: u16 = 5;
/// Flags column.
pub const FLAGS: u16 = 6;

const OPTIONAL: [u16; 3] = [ADMIN_LEVEL, CLASS, FLAGS];

/// The PLACES section of an open pack.
#[derive(Clone, Copy, Debug)]
pub struct Places<'a> {
    lat: Packed<'a>,
    lon: Packed<'a>,
    kind: Packed<'a>,
    optional: [Option<Packed<'a>>; 3],
}

fn fail(column: u16, problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::PLACES.0,
        column,
        problem,
    }
}

impl<'a> Places<'a> {
    /// Opens the section: its frame and the structure of every column, in O(columns + blocks).
    ///
    /// # Errors
    /// [`Error::Payload`] if the section is not a PLACES column table, a required column is
    /// missing, or a column is malformed or of the wrong length.
    pub fn open(section: &Section<'a>) -> Result<Self, Error> {
        let table: Table<'a> = columns::open(section, Kind::PLACES)?;
        let rows = usize::try_from(section.item_count())
            .map_err(|_| fail(0, "the row count does not fit this platform"))?;
        let req = |id| columns::required(&table, Kind::PLACES, id, Some(rows));
        let (lat, lon, kind) = (req(LAT)?, req(LON)?, req(KIND)?);
        let mut optional = [None; 3];
        for (slot, id) in optional.iter_mut().zip(OPTIONAL) {
            *slot = columns::optional(&table, Kind::PLACES, id, rows)?;
        }
        if table.ids().any(|id| id > FLAGS) {
            return Err(fail(0, "an unknown column"));
        }
        Ok(Self {
            lat,
            lon,
            kind,
            optional,
        })
    }

    /// The number of places.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.lat.len()
    }

    /// Whether there are none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.lat.is_empty()
    }

    /// The point of `row`; `None` past the end, or if the stored value is off the grid (which
    /// [`Places::check`] rules out).
    #[must_use]
    pub fn point(&self, row: usize) -> Option<Point<E6>> {
        Point::from_units(unbias(self.lat.get(row)?), unbias(self.lon.get(row)?))
    }

    /// The kind of `row`; `None` past the end or for an unknown code.
    #[must_use]
    pub fn kind(&self, row: usize) -> Option<PlaceKind> {
        PlaceKind::from_code(self.kind.get(row)?)
    }

    fn optional(&self, slot: usize, row: usize) -> Option<u32> {
        if row >= self.len() {
            return None;
        }
        match self.optional.get(slot).copied().flatten() {
            Some(col) => col.get(row),
            None => Some(0),
        }
    }

    /// The admin level of `row` (0 for a country or a place that is not an admin unit).
    #[must_use]
    pub fn admin_level(&self, row: usize) -> Option<u8> {
        self.optional(0, row).and_then(|v| u8::try_from(v).ok())
    }

    /// The class dictionary code of `row`, 0 for none.
    #[must_use]
    pub fn class(&self, row: usize) -> Option<u32> {
        self.optional(1, row)
    }

    /// The flag bits of `row`.
    #[must_use]
    pub fn flags(&self, row: usize) -> Option<u32> {
        self.optional(2, row)
    }

    /// Checks every value: points on the grid and in Hilbert order, known kinds, admin levels
    /// that fit a byte and are set only on admin units, and no optional column that is all 0
    /// (the writer leaves those out). Streams the columns a block at a time; O(rows).
    ///
    /// # Errors
    /// [`Error::Payload`] naming the column.
    pub fn check(&self) -> Result<(), Error> {
        let n = self.len();
        let mut last = 0_u64;
        for (row, (lat, lon)) in self.lat.values(0..n).zip(self.lon.values(0..n)).enumerate() {
            let p = Point::<E6>::from_units(unbias(lat), unbias(lon))
                .ok_or_else(|| fail(LAT, "a point is off the grid"))?;
            let h = hilbert(p);
            if row > 0 && h < last {
                return Err(fail(LAT, "rows are not in Hilbert order"));
            }
            last = h;
        }
        let mut levels = self.optional[0].map(|c| c.values(0..n));
        for code in self.kind.values(0..n) {
            let kind = PlaceKind::from_code(code).ok_or_else(|| fail(KIND, "an unknown kind"))?;
            let level = levels.as_mut().and_then(Iterator::next).unwrap_or(0);
            if level > u32::from(u8::MAX) || (level != 0 && kind != PlaceKind::Admin) {
                return Err(fail(
                    ADMIN_LEVEL,
                    "an admin level that is too big or not on an admin unit",
                ));
            }
        }
        for (col, id) in self.optional.iter().zip(OPTIONAL) {
            if let Some(c) = col
                && c.values(0..n).all(|v| v == 0)
            {
                return Err(fail(id, "an optional column that is all 0 is stored"));
            }
        }
        Ok(())
    }
}

/// The columns of a PLACES section to write (feature `write`). Optional columns may be empty
/// (absent); all others have one value per place.
#[cfg(feature = "write")]
#[derive(Clone, Copy, Debug, Default)]
pub struct PlaceColumns<'c> {
    /// The points, in Hilbert order.
    pub points: &'c [Point<E6>],
    /// The kinds.
    pub kinds: &'c [PlaceKind],
    /// Admin levels, or empty.
    pub admin_levels: &'c [u8],
    /// Class codes, or empty.
    pub classes: &'c [u32],
    /// Flags, or empty.
    pub flags: &'c [u32],
}

/// Encodes a PLACES payload ([`Encoding::TABLE`](crate::Encoding::TABLE)); its item count is
/// the number of points.
///
/// # Errors
/// [`Error::Write`] if the columns differ in length or the points are not in Hilbert order.
#[cfg(feature = "write")]
pub fn encode(cols: &PlaceColumns<'_>) -> Result<alloc::vec::Vec<u8>, Error> {
    use alloc::vec::Vec;
    use teigeodata_core::bitpack::{bias, encode_auto};

    let n = cols.points.len();
    let optional_ok = |len: usize| len == 0 || len == n;
    if cols.kinds.len() != n
        || !optional_ok(cols.admin_levels.len())
        || !optional_ok(cols.classes.len())
        || !optional_ok(cols.flags.len())
    {
        return Err(Error::Write("the place columns differ in length"));
    }
    if cols.points.windows(2).any(|w| match w {
        [a, b] => hilbert(*a) > hilbert(*b),
        _ => false,
    }) {
        return Err(Error::Write("the places are not in Hilbert order"));
    }
    let lat: Vec<u32> = cols.points.iter().map(|p| bias(p.lat())).collect();
    let lon: Vec<u32> = cols.points.iter().map(|p| bias(p.lon())).collect();
    let kind: Vec<u32> = cols.kinds.iter().map(|k| u32::from(k.code())).collect();
    let level: Vec<u32> = cols.admin_levels.iter().map(|&v| u32::from(v)).collect();
    let mut encoded: Vec<(u16, Vec<u8>)> = Vec::with_capacity(6);
    for (id, values) in [
        (LAT, &lat[..]),
        (LON, &lon),
        (KIND, &kind),
        (ADMIN_LEVEL, &level),
        (CLASS, cols.classes),
        (FLAGS, cols.flags),
    ] {
        // An optional column that is empty or all 0 is left out: it reads as 0.
        if id <= KIND || values.iter().any(|&v| v != 0) {
            encoded.push((id, encode_auto(values)));
        }
    }
    let refs: Vec<(u16, &[u8])> = encoded.iter().map(|(id, b)| (*id, &b[..])).collect();
    teigeodata_core::table::encode(&refs).map_err(|_| Error::Write("the place table"))
}
