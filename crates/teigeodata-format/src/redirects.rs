//! REDIRECTS (kind 16): retired IDs, each to the current ID that replaced it, or to none for a
//! tombstone ([ADR-0008](https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/adr/ADR-0008-stable-ids.md)).
//!
//! A [column table](teigeodata_core::table) whose columns all have one value per retired ID
//! (the section's item count):
//!
//! | ID | Column | Values |
//! |---:|---|---|
//! | 1 | [`OLD_HI`] | high 32 bits of the retired IDs, in increasing order |
//! | 2 | [`OLD_LO`] | low 32 bits |
//! | 3 | [`NEW_HI`] | high 32 bits of the survivor; 0 with [`NEW_LO`] 0 for a tombstone |
//! | 4 | [`NEW_LO`] | low 32 bits |
//!
//! Chains are flattened by the forge: a survivor is always a current place of the pack, so one
//! lookup answers ([`resolve`]).

use teigeodata_core::PlaceId;
use teigeodata_core::bitpack::Packed;
use teigeodata_core::table::Table;

use crate::idmap::IdMap;
use crate::sorted::{Sorted, key};
use crate::{Error, Kind, Section, columns};

/// High halves of the retired IDs.
pub const OLD_HI: u16 = 1;
/// Low halves of the retired IDs.
pub const OLD_LO: u16 = 2;
/// High halves of the survivors.
pub const NEW_HI: u16 = 3;
/// Low halves of the survivors.
pub const NEW_LO: u16 = 4;

/// What became of a retired ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fate {
    /// Merged into this current ID.
    Redirect(PlaceId),
    /// Removed, with no successor.
    Tombstone,
}

/// The REDIRECTS section of an open pack.
#[derive(Clone, Copy, Debug)]
pub struct Redirects<'a> {
    old: Sorted<'a>,
    new_hi: Packed<'a>,
    new_lo: Packed<'a>,
}

fn fail(column: u16, problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::REDIRECTS.0,
        column,
        problem,
    }
}

impl<'a> Redirects<'a> {
    /// Opens the section: frame, column structure and lengths, in O(columns + blocks).
    ///
    /// # Errors
    /// [`Error::Payload`] if it is not a REDIRECTS column table, or a column is missing,
    /// malformed or of the wrong length.
    pub fn open(section: &Section<'a>) -> Result<Self, Error> {
        let table: Table<'a> = columns::open(section, Kind::REDIRECTS)?;
        let rows = usize::try_from(section.item_count())
            .map_err(|_| fail(0, "the row count does not fit this platform"))?;
        let col = |id| columns::required(&table, Kind::REDIRECTS, id, Some(rows));
        if table.ids().len() != 4 {
            return Err(fail(0, "an unknown column"));
        }
        Ok(Self {
            old: Sorted {
                hi: col(OLD_HI)?,
                lo: col(OLD_LO)?,
            },
            new_hi: col(NEW_HI)?,
            new_lo: col(NEW_LO)?,
        })
    }

    /// The number of retired IDs.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.new_lo.len()
    }

    /// Whether there are none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.new_lo.is_empty()
    }

    /// What became of `id`, if it is retired here.
    #[must_use]
    pub fn get(&self, id: PlaceId) -> Option<Fate> {
        let i = self.old.find(id.get())?;
        Some(self.fate(i))
    }

    fn fate(&self, i: usize) -> Fate {
        let new = key(
            self.new_hi.get(i).unwrap_or(0),
            self.new_lo.get(i).unwrap_or(0),
        );
        PlaceId::from_raw(new).map_or(Fate::Tombstone, Fate::Redirect)
    }

    /// Checks every value against the pack's current IDs: retired IDs valid, strictly
    /// increasing and not current; every survivor current. O(retired × log rows).
    ///
    /// # Errors
    /// [`Error::Payload`] naming the column.
    pub fn check(&self, current: &IdMap<'_>) -> Result<(), Error> {
        if !self.old.is_strictly_increasing() {
            return Err(fail(OLD_LO, "the retired IDs are not strictly increasing"));
        }
        for (i, old) in self.old.iter().enumerate() {
            let old = PlaceId::from_raw(old).ok_or_else(|| fail(OLD_LO, "an invalid ID"))?;
            if current.row(old).is_some() {
                return Err(fail(OLD_LO, "a retired ID is also current"));
            }
            if let Fate::Redirect(new) = self.fate(i)
                && current.row(new).is_none()
            {
                return Err(fail(NEW_LO, "a survivor is not a current place"));
            }
        }
        Ok(())
    }
}

/// Where an ID leads in a pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolved {
    /// A current place, at this row.
    Row(usize),
    /// Retired and merged: the survivor's ID and row.
    Moved(PlaceId, usize),
    /// Retired with no successor.
    Retired,
    /// Not known to this pack.
    Unknown,
}

/// Resolves an ID: current first, then retired (one hop: the forge flattens chains).
#[must_use]
pub fn resolve(ids: &IdMap<'_>, redirects: Option<&Redirects<'_>>, id: PlaceId) -> Resolved {
    if let Some(row) = ids.row(id) {
        return Resolved::Row(row);
    }
    match redirects.and_then(|r| r.get(id)) {
        Some(Fate::Redirect(new)) => ids
            .row(new)
            .map_or(Resolved::Unknown, |row| Resolved::Moved(new, row)),
        Some(Fate::Tombstone) => Resolved::Retired,
        None => Resolved::Unknown,
    }
}

/// Encodes a REDIRECTS payload from `(retired, survivor)` pairs, `None` for a tombstone (feature
/// `write`); its item count is the number of pairs. The pairs may come in any order.
///
/// # Errors
/// [`Error::Write`] if a retired ID repeats or is its own survivor.
#[cfg(feature = "write")]
pub fn encode(pairs: &[(PlaceId, Option<PlaceId>)]) -> Result<alloc::vec::Vec<u8>, Error> {
    use alloc::vec::Vec;
    use teigeodata_core::bitpack::encode_auto;

    let mut sorted: Vec<(u64, u64)> = pairs
        .iter()
        .map(|(old, new)| (old.get(), new.map_or(0, PlaceId::get)))
        .collect();
    sorted.sort_unstable();
    if sorted
        .windows(2)
        .any(|w| w.first().map(|p| p.0) == w.last().map(|p| p.0))
        || sorted.iter().any(|(old, new)| old == new)
    {
        return Err(Error::Write("a retired ID repeats or is its own survivor"));
    }
    let (old_hi, old_lo) = crate::sorted::split(sorted.iter().map(|p| p.0));
    let (new_hi, new_lo) = crate::sorted::split(sorted.iter().map(|p| p.1));
    let cols = [
        (OLD_HI, encode_auto(&old_hi)),
        (OLD_LO, encode_auto(&old_lo)),
        (NEW_HI, encode_auto(&new_hi)),
        (NEW_LO, encode_auto(&new_lo)),
    ];
    let refs: Vec<(u16, &[u8])> = cols.iter().map(|(id, b)| (*id, &b[..])).collect();
    teigeodata_core::table::encode(&refs).map_err(|_| Error::Write("the redirects table"))
}
