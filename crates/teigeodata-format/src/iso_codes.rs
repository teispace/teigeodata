//! `ISO_CODES` (kind 21, component `admin`): the ISO 3166-1 alpha-2 code of a country row and
//! the ISO 3166-2 code of a subdivision row (D-197, [ISO 3166
//! codes](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/iso-3166-codes.md)).
//!
//! A [column table](teigeodata_core::table) with two columns, one value per coded row:
//!
//! | ID | Column | Values |
//! |---:|---|---|
//! | 1 | [`ROWS`] | the PLACES rows that have a code, strictly increasing |
//! | 2 | [`CODE`] | each one's code, packed ([`IsoCode::packed`]) |
//!
//! The item count is the number of coded rows. The full code is stored, alpha-2 included: the
//! ID namespace is the allocating country, not the ISO code (ADR-0008), and under the Nepal view
//! a unit's ISO prefix can differ from the country it hangs from. A pack with no codes has no
//! `ISO_CODES` section. A lookup by row is a binary search (`sorted::Sorted`); reads never
//! panic.

use teigeodata_core::IsoCode;
use teigeodata_core::bitpack::Packed;

use crate::sorted::Sorted;
use crate::{Error, Kind, Section, columns};

/// The coded PLACES rows.
pub const ROWS: u16 = 1;
/// Each coded row's packed code.
pub const CODE: u16 = 2;

/// The `ISO_CODES` section of an open pack.
#[derive(Clone, Copy, Debug)]
pub struct IsoCodes<'a> {
    rows: Sorted<'a>,
    codes: Packed<'a>,
}

const fn fail(column: u16, problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::ISO_CODES.0,
        column,
        problem,
    }
}

impl<'a> IsoCodes<'a> {
    /// Opens the section of a pack with `places` PLACES rows: its frame and both columns, in
    /// O(blocks).
    ///
    /// # Errors
    /// [`Error::Payload`] if the section is not an `ISO_CODES` column table with one row and
    /// one code per item, at least one item, and its last row below `places`.
    pub fn open(section: &Section<'a>, places: usize) -> Result<Self, Error> {
        let table = columns::open(section, Kind::ISO_CODES)?;
        let n = usize::try_from(section.item_count())
            .map_err(|_| fail(0, "the row count does not fit this platform"))?;
        let lo = columns::required(&table, Kind::ISO_CODES, ROWS, Some(n))?;
        let codes = columns::required(&table, Kind::ISO_CODES, CODE, Some(n))?;
        if table.ids().any(|id| id != ROWS && id != CODE) {
            return Err(fail(0, "an unknown column"));
        }
        let rows = Sorted { hi: None, lo };
        match rows.bounds() {
            None => Err(fail(ROWS, "no coded row: the section should be absent")),
            Some((_, last)) if usize::try_from(last).map_or(true, |l| l >= places) => {
                Err(fail(ROWS, "a row past the places"))
            }
            Some(_) => Ok(Self { rows, codes }),
        }
    }

    /// The number of coded rows.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.codes.len()
    }

    /// Whether there are none (never, for an opened section).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }

    /// The code of PLACES row `row`; `None` where it has none (or a corrupt value: `check`
    /// finds those). O(log blocks) and one block decoded.
    #[must_use]
    pub fn get(&self, row: u32) -> Option<IsoCode> {
        let i = self.rows.find(u64::from(row))?;
        IsoCode::from_packed(self.codes.get(i)?)
    }

    /// The `i`-th coded row and its code, in row order.
    #[must_use]
    pub fn entry(&self, i: usize) -> Option<(u32, IsoCode)> {
        Some((
            self.rows.lo.get(i)?,
            IsoCode::from_packed(self.codes.get(i)?)?,
        ))
    }

    /// The first row (in row order) whose code is `code`. A scan of the codes, O(coded rows):
    /// a few thousand values for the world baseline.
    #[must_use]
    pub fn row_of(&self, code: IsoCode) -> Option<u32> {
        let want = code.packed();
        let i = self.codes.values(0..self.len()).position(|v| v == want)?;
        self.rows.lo.get(i)
    }

    /// Every coded row and its code, in row order; a value that is not a code is skipped.
    pub fn iter(&self) -> impl Iterator<Item = (u32, IsoCode)> + '_ {
        let n = self.len();
        self.rows
            .lo
            .values(0..n)
            .zip(self.codes.values(0..n))
            .filter_map(|(r, c)| Some((r, IsoCode::from_packed(c)?)))
    }

    /// Checks every value: the rows strictly increase and every code is a valid ISO 3166 code.
    /// O(rows), no allocation.
    ///
    /// # Errors
    /// [`Error::Payload`] on the first bad value.
    pub fn check(&self) -> Result<(), Error> {
        if !self.rows.is_strictly_increasing() {
            return Err(fail(ROWS, "rows not strictly increasing"));
        }
        if self
            .codes
            .values(0..self.len())
            .any(|v| IsoCode::from_packed(v).is_none())
        {
            return Err(fail(CODE, "a value that is not an ISO 3166 code"));
        }
        Ok(())
    }
}

/// Encodes an `ISO_CODES` payload ([`Encoding::TABLE`](crate::Encoding::TABLE)) from coded rows
/// in strictly increasing row order; its item count is `codes.len()`. Returns `None` for no
/// codes: the pack then has no `ISO_CODES` section.
///
/// # Errors
/// [`Error::Write`] if the rows are not strictly increasing.
#[cfg(feature = "write")]
pub fn encode(codes: &[(u32, IsoCode)]) -> Result<Option<alloc::vec::Vec<u8>>, Error> {
    use alloc::vec::Vec;
    if codes.is_empty() {
        return Ok(None);
    }
    if codes.windows(2).any(|w| matches!(w, [a, b] if a.0 >= b.0)) {
        return Err(Error::Write("ISO codes' rows not strictly increasing"));
    }
    let rows: Vec<u32> = codes.iter().map(|c| c.0).collect();
    let values: Vec<u32> = codes.iter().map(|c| c.1.packed()).collect();
    let a = teigeodata_core::bitpack::encode_auto(&rows);
    let b = teigeodata_core::bitpack::encode_auto(&values);
    teigeodata_core::table::encode(&[(ROWS, &a[..]), (CODE, &b[..])])
        .map(Some)
        .map_err(|_| Error::Write("the ISO code table"))
}

#[cfg(all(test, feature = "write"))]
mod tests {
    use super::{CODE, IsoCodes, ROWS, encode};
    use crate::{Kind, Section};
    use alloc::vec::Vec;
    use teigeodata_core::IsoCode;

    #[allow(clippy::unwrap_used, reason = "test inputs are valid codes")]
    fn code(s: &str) -> IsoCode {
        s.parse().unwrap()
    }

    #[test]
    fn codes_round_trip_and_none_is_absent() {
        let codes = [
            (3, code("NP-P3")),
            (7, code("NP")),
            (8, code("NP-P1")),
            (200, code("IN-TN")),
            (201, code("PK-GB")),
        ];
        let bytes = encode(&codes).unwrap().unwrap();
        let s = Section::unpacked(Kind::ISO_CODES, codes.len() as u64, &bytes);
        let t = IsoCodes::open(&s, 202).unwrap();
        t.check().unwrap();
        assert_eq!(t.len(), 5);
        for &(r, c) in &codes {
            assert_eq!(t.get(r), Some(c));
            assert_eq!(t.row_of(c), Some(r));
        }
        for r in [0, 4, 9, 199, 202, u32::MAX] {
            assert_eq!(t.get(r), None, "row {r}");
        }
        assert_eq!(t.row_of(code("IN")), None);
        assert!(t.iter().eq(codes.iter().copied()));
        assert_eq!(t.entry(1), Some((7, code("NP"))));
        assert_eq!(t.entry(5), None);
        assert_eq!(encode(&[]).unwrap(), None);
        // Rows out of order, a row past the places, a wrong item count and another kind are
        // refused.
        assert!(encode(&[(2, code("NP")), (2, code("IN"))]).is_err());
        assert!(IsoCodes::open(&s, 201).is_err(), "row 201 of 201 places");
        let wrong = Section::unpacked(Kind::ISO_CODES, 4, &bytes);
        assert!(IsoCodes::open(&wrong, 202).is_err());
        let other = Section::unpacked(Kind::PLACE_TZ, 5, &bytes);
        assert!(IsoCodes::open(&other, 202).is_err());
    }

    /// Planted faults: rows out of order, a value that is not a code, a user-assigned code,
    /// an unknown column, and no rows.
    #[test]
    fn the_check_catches_bad_rows_and_codes() {
        let col = |v: &[u32]| teigeodata_core::bitpack::encode_auto(v);
        let np = code("NP").packed();
        let p3 = code("NP-P3").packed();
        let check = |rows: &[u32], codes: &[u32]| {
            let b =
                teigeodata_core::table::encode(&[(ROWS, &col(rows)[..]), (CODE, &col(codes)[..])])
                    .unwrap();
            let s = Section::unpacked(Kind::ISO_CODES, rows.len() as u64, &b);
            IsoCodes::open(&s, 10).and_then(|t| t.check())
        };
        assert!(check(&[1, 4], &[np, p3]).is_ok());
        assert!(check(&[4, 1], &[np, p3]).is_err(), "not increasing");
        assert!(check(&[1, 1], &[np, p3]).is_err(), "a row twice");
        assert!(check(&[1, 4], &[np, 0]).is_err(), "0 is not a code");
        assert!(check(&[1, 4], &[np, p3 | 1 << 28]).is_err(), "bits past 27");
        let xk = (24 << 23) | (11 << 18);
        assert!(check(&[1, 4], &[np, xk]).is_err(), "user-assigned");
        assert!(check(&[], &[]).is_err(), "no rows");
        assert!(check(&[1, 10], &[np, p3]).is_err(), "row 10 of 10 places");
        let extra: Vec<u8> = teigeodata_core::table::encode(&[
            (ROWS, &col(&[1])[..]),
            (CODE, &col(&[np])[..]),
            (3, &col(&[1])[..]),
        ])
        .unwrap();
        let s = Section::unpacked(Kind::ISO_CODES, 1, &extra);
        assert!(IsoCodes::open(&s, 10).is_err(), "an unknown column");
    }
}
