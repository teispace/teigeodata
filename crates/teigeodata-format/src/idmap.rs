//! IDMAP (kind 1): each row's stable ID, and the row of each ID (B5,
//! [ID to row](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/id-to-row.md)).
//!
//! A [column table](teigeodata_core::table) whose columns all have one value per row:
//!
//! | ID | Column | Values |
//! |---:|---|---|
//! | 1 | [`ID_HI`] | high 32 bits of each row's ID |
//! | 2 | [`ID_LO`] | low 32 bits |
//! | 3 | [`SORTED_HI`] | high 32 bits of the IDs in increasing order |
//! | 4 | [`SORTED_LO`] | low 32 bits, the same order |
//! | 5 | [`ORDER`] | the row of each sorted ID |
//!
//! Rows are in Hilbert order (PLACES), so ID → row needs the permutation [`ORDER`]; a lookup
//! searches the sorted IDs a block at a time and reads one row.

use teigeodata_core::PlaceId;
use teigeodata_core::bitpack::Packed;
use teigeodata_core::table::Table;

use crate::sorted::{Sorted, key};
use crate::{Error, Kind, Section, columns};

/// High halves of each row's ID.
pub const ID_HI: u16 = 1;
/// Low halves of each row's ID.
pub const ID_LO: u16 = 2;
/// High halves of the sorted IDs.
pub const SORTED_HI: u16 = 3;
/// Low halves of the sorted IDs.
pub const SORTED_LO: u16 = 4;
/// The row of each sorted ID.
pub const ORDER: u16 = 5;

/// The IDMAP section of an open pack.
#[derive(Clone, Copy, Debug)]
pub struct IdMap<'a> {
    hi: Packed<'a>,
    lo: Packed<'a>,
    sorted: Sorted<'a>,
    order: Packed<'a>,
}

fn fail(column: u16, problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::IDMAP.0,
        column,
        problem,
    }
}

impl<'a> IdMap<'a> {
    /// Opens the section: frame, column structure and lengths, in O(columns + blocks).
    ///
    /// # Errors
    /// [`Error::Payload`] if it is not an IDMAP column table, or a column is missing,
    /// malformed or of the wrong length.
    pub fn open(section: &Section<'a>) -> Result<Self, Error> {
        let table: Table<'a> = columns::open(section, Kind::IDMAP)?;
        let rows = usize::try_from(section.item_count())
            .map_err(|_| fail(0, "the row count does not fit this platform"))?;
        let col = |id| columns::required(&table, Kind::IDMAP, id, Some(rows));
        if table.ids().len() != 5 {
            return Err(fail(0, "an unknown column"));
        }
        Ok(Self {
            hi: col(ID_HI)?,
            lo: col(ID_LO)?,
            sorted: Sorted {
                hi: col(SORTED_HI)?,
                lo: col(SORTED_LO)?,
            },
            order: col(ORDER)?,
        })
    }

    /// The number of rows.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.lo.len()
    }

    /// Whether there are none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.lo.is_empty()
    }

    /// The ID of `row`; `None` past the end (or for a stored 0, which [`IdMap::check`] rules out).
    #[must_use]
    pub fn id(&self, row: usize) -> Option<PlaceId> {
        PlaceId::from_raw(key(self.hi.get(row)?, self.lo.get(row)?))
    }

    /// The row of `id`, if this pack has it as a current place. Retired IDs are in
    /// [`Redirects`](crate::redirects::Redirects).
    #[must_use]
    pub fn row(&self, id: PlaceId) -> Option<usize> {
        let k = self.sorted.find(id.get())?;
        let row = usize::try_from(self.order.get(k)?).ok()?;
        // The permutation is checked by `check`; confirming here keeps a bad pack from
        // answering with the wrong place.
        (self.id(row) == Some(id)).then_some(row)
    }

    /// Checks every value: each ID valid, the sorted IDs strictly increasing, and each sorted
    /// ID at the row [`ORDER`] gives (which makes [`ORDER`] a permutation). O(rows).
    ///
    /// # Errors
    /// [`Error::Payload`] naming the column.
    pub fn check(&self) -> Result<(), Error> {
        if !self.sorted.is_strictly_increasing() {
            return Err(fail(
                SORTED_LO,
                "the sorted IDs are not strictly increasing",
            ));
        }
        let n = self.len();
        for (id, row) in self.sorted.iter().zip(self.order.values(0..n)) {
            let place = PlaceId::from_raw(id).ok_or_else(|| fail(SORTED_LO, "an invalid ID"))?;
            let row = usize::try_from(row).unwrap_or(usize::MAX);
            if self.id(row) != Some(place) {
                return Err(fail(ORDER, "a sorted ID is not at the row its order names"));
            }
        }
        Ok(())
    }
}

/// Encodes an IDMAP payload from each row's ID (feature `write`); its item count is
/// `ids.len()`.
///
/// # Errors
/// [`Error::Write`] if an ID repeats or there are more than 2³² rows.
#[cfg(feature = "write")]
pub fn encode(ids: &[PlaceId]) -> Result<alloc::vec::Vec<u8>, Error> {
    use alloc::vec::Vec;
    use teigeodata_core::bitpack::encode_auto;

    let n = u32::try_from(ids.len()).map_err(|_| Error::Write("more than 2^32 rows"))?;
    let mut order: Vec<u32> = (0..n).collect();
    order.sort_unstable_by_key(|&r| ids.get(r as usize).map_or(0, |id| id.get()));
    let sorted: Vec<u64> = order
        .iter()
        .map(|&r| ids.get(r as usize).map_or(0, |id| id.get()))
        .collect();
    if sorted.windows(2).any(|w| w.first() == w.last()) {
        return Err(Error::Write("an ID repeats"));
    }
    let (hi, lo) = crate::sorted::split(ids.iter().map(|id| id.get()));
    let (sorted_hi, sorted_lo) = crate::sorted::split(sorted.into_iter());
    let cols = [
        (ID_HI, encode_auto(&hi)),
        (ID_LO, encode_auto(&lo)),
        (SORTED_HI, encode_auto(&sorted_hi)),
        (SORTED_LO, encode_auto(&sorted_lo)),
        (ORDER, encode_auto(&order)),
    ];
    let refs: Vec<(u16, &[u8])> = cols.iter().map(|(id, b)| (*id, &b[..])).collect();
    teigeodata_core::table::encode(&refs).map_err(|_| Error::Write("the ID map table"))
}
