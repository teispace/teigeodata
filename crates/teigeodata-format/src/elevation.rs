//! ELEVATION (kind 19): each place's height, one row per PLACES row (D-133). Its own section,
//! so a client that wants coordinates only never downloads it ([components]).
//!
//! A [column table](teigeodata_core::table) with one column:
//!
//! | ID | Column | Values |
//! |---:|---|---|
//! | 1 | [`HEIGHT`] | orthometric metres above EGM2008, + [`BIAS`]; 0 = unknown |
//!
//! The item count is the number of places, and must equal PLACES' (the engine checks it when it
//! opens a layer). A pack where no place has a height has no ELEVATION section. Reads never panic.
//!
//! [components]: https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/components.md

use teigeodata_core::bitpack::Packed;

use crate::{Error, Kind, Section, columns};

/// The height column.
pub const HEIGHT: u16 = 1;
/// What [`HEIGHT`] adds to a height so that every `i16` but `i16::MIN` is stored above 0.
pub const BIAS: i32 = 32_768;

/// The ELEVATION section of an open pack.
#[derive(Clone, Copy, Debug)]
pub struct Elevations<'a> {
    height: Packed<'a>,
}

fn fail(problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::ELEVATION.0,
        column: HEIGHT,
        problem,
    }
}

impl<'a> Elevations<'a> {
    /// Opens the section: its frame and its one column, in O(blocks).
    ///
    /// # Errors
    /// [`Error::Payload`] if the section is not an ELEVATION column table of one column with one
    /// value per item.
    pub fn open(section: &Section<'a>) -> Result<Self, Error> {
        let table = columns::open(section, Kind::ELEVATION)?;
        let rows = usize::try_from(section.item_count())
            .map_err(|_| fail("the row count does not fit this platform"))?;
        let height = columns::required(&table, Kind::ELEVATION, HEIGHT, Some(rows))?;
        if table.ids().any(|id| id != HEIGHT) {
            return Err(fail("an unknown column"));
        }
        Ok(Self { height })
    }

    /// The number of rows.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.height.len()
    }

    /// Whether there are none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.height.is_empty()
    }

    /// The height of `row` in whole metres above EGM2008; `None` past the end or where it is
    /// unknown.
    #[must_use]
    pub fn get(&self, row: usize) -> Option<i16> {
        let v = self.height.get(row)?;
        if v == 0 {
            return None;
        }
        i32::try_from(v)
            .ok()
            .and_then(|v| i16::try_from(v - BIAS).ok())
    }

    /// Checks every value: each fits an `i16` once unbiased, and at least one is known (the
    /// writer leaves the section out otherwise). O(rows).
    ///
    /// # Errors
    /// [`Error::Payload`] on the first bad value.
    pub fn check(&self) -> Result<(), Error> {
        let n = self.len();
        let mut known = false;
        for v in self.height.values(0..n) {
            if v > u32::from(u16::MAX) {
                return Err(fail("a height outside i16"));
            }
            known |= v != 0;
        }
        if known {
            Ok(())
        } else {
            Err(fail(
                "every height is unknown: the section should be absent",
            ))
        }
    }
}

/// Encodes an ELEVATION payload ([`Encoding::TABLE`](crate::Encoding::TABLE)) of one height per
/// place, `None` where unknown; its item count is `heights.len()`. Returns `None` when no height
/// is known: the pack then has no ELEVATION section.
///
/// # Errors
/// [`Error::Write`] for `i16::MIN`, the stored "unknown".
#[cfg(feature = "write")]
pub fn encode(heights: &[Option<i16>]) -> Result<Option<alloc::vec::Vec<u8>>, Error> {
    use alloc::vec::Vec;
    if heights.contains(&Some(i16::MIN)) {
        return Err(Error::Write("an elevation of i16::MIN"));
    }
    if heights.iter().all(Option::is_none) {
        return Ok(None);
    }
    let values: Vec<u32> = heights
        .iter()
        .map(|h| h.map_or(0, |m| (i32::from(m) + BIAS).unsigned_abs()))
        .collect();
    let col = teigeodata_core::bitpack::encode_auto(&values);
    teigeodata_core::table::encode(&[(HEIGHT, &col[..])])
        .map(Some)
        .map_err(|_| Error::Write("the elevation table"))
}

#[cfg(all(test, feature = "write"))]
mod tests {
    use super::{Elevations, encode};
    use crate::{Kind, Section};

    #[test]
    fn heights_round_trip_and_unknown_is_absent() {
        let heights = [
            Some(48),
            None,
            Some(8_848),
            Some(-400),
            Some(i16::MAX),
            None,
        ];
        let bytes = encode(&heights).unwrap().unwrap();
        let s = Section::unpacked(Kind::ELEVATION, heights.len() as u64, &bytes);
        let e = Elevations::open(&s).unwrap();
        e.check().unwrap();
        assert_eq!(e.len(), 6);
        let back: alloc::vec::Vec<Option<i16>> = (0..6).map(|r| e.get(r)).collect();
        assert_eq!(back, heights);
        assert_eq!(e.get(6), None);
        assert_eq!(encode(&[None, None]).unwrap(), None);
        assert!(encode(&[Some(i16::MIN)]).is_err());
        // A wrong item count, or another kind, is refused.
        let wrong = Section::unpacked(Kind::ELEVATION, 5, &bytes);
        assert!(Elevations::open(&wrong).is_err());
        let other = Section::unpacked(Kind::PLACES, 6, &bytes);
        assert!(Elevations::open(&other).is_err());
    }
}
