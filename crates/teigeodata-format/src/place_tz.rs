//! `PLACE_TZ` (kind 20): each place's timezone area, one row per PLACES row (D-134). A place's
//! timezone is fixed when the pack is built, so a client that wants it for its places needs a
//! few bits per place, not the REGION index (1.6 MB on Nepal), which is only for an arbitrary
//! coordinate ([components]).
//!
//! A [column table](teigeodata_core::table) with two columns:
//!
//! | ID | Column | Values |
//! |---:|---|---|
//! | 1 | [`ZONES`] | the PLACES rows of the timezone areas used, strictly increasing |
//! | 2 | [`ZONE`] | per place: 1 + its area's index in [`ZONES`]; 0 = none |
//!
//! The item count is the number of places, and must equal PLACES' (the engine checks it when it
//! opens a layer). A place's area is the one the pack's REGION holds at the place's stored
//! point, so `tz_of(place)` and `tz_at(place.point)` agree. A pack where no place has an area
//! has no `PLACE_TZ` section. Reads never panic.
//!
//! [components]: https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/components.md

use teigeodata_core::bitpack::Packed;

use crate::{Error, Kind, Section, columns};

/// The timezone areas' PLACES rows.
pub const ZONES: u16 = 1;
/// Each place's area, as 1 + an index into [`ZONES`].
pub const ZONE: u16 = 2;

/// The `PLACE_TZ` section of an open pack.
#[derive(Clone, Copy, Debug)]
pub struct PlaceTz<'a> {
    zones: Packed<'a>,
    zone: Packed<'a>,
}

const fn fail(column: u16, problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::PLACE_TZ.0,
        column,
        problem,
    }
}

impl<'a> PlaceTz<'a> {
    /// Opens the section: its frame and both columns, in O(blocks).
    ///
    /// # Errors
    /// [`Error::Payload`] if the section is not a `PLACE_TZ` column table with one [`ZONE`] value
    /// per item and at least one area.
    pub fn open(section: &Section<'a>) -> Result<Self, Error> {
        let table = columns::open(section, Kind::PLACE_TZ)?;
        let rows = usize::try_from(section.item_count())
            .map_err(|_| fail(0, "the row count does not fit this platform"))?;
        let zones = columns::required(&table, Kind::PLACE_TZ, ZONES, None)?;
        let zone = columns::required(&table, Kind::PLACE_TZ, ZONE, Some(rows))?;
        if zones.is_empty() || zones.len() > rows {
            return Err(fail(ZONES, "no areas, or more areas than places"));
        }
        if table.ids().any(|id| id != ZONES && id != ZONE) {
            return Err(fail(0, "an unknown column"));
        }
        Ok(Self { zones, zone })
    }

    /// The number of rows (places).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.zone.len()
    }

    /// Whether there are none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.zone.is_empty()
    }

    /// The number of timezone areas the places use.
    #[must_use]
    pub const fn zones(&self) -> usize {
        self.zones.len()
    }

    /// The PLACES row of the timezone area of place `row`; `None` past the end or where the
    /// place has none.
    #[must_use]
    pub fn get(&self, row: usize) -> Option<u32> {
        let v = self.zone.get(row)?;
        let i = usize::try_from(v.checked_sub(1)?).ok()?;
        self.zones.get(i)
    }

    /// Checks every value: the areas' rows increase and are rows of this pack, every place's
    /// value names an area, and at least one place has one. O(rows).
    ///
    /// # Errors
    /// [`Error::Payload`] on the first bad value.
    pub fn check(&self) -> Result<(), Error> {
        let n = self.len();
        let mut last = None;
        for z in self.zones.values(0..self.zones()) {
            if last.is_some_and(|l| z <= l) || usize::try_from(z).map_or(true, |z| z >= n) {
                return Err(fail(ZONES, "areas not increasing, or not a row"));
            }
            last = Some(z);
        }
        let k = u32::try_from(self.zones()).unwrap_or(u32::MAX);
        let mut known = false;
        for v in self.zone.values(0..n) {
            if v > k {
                return Err(fail(ZONE, "a value past the areas"));
            }
            known |= v != 0;
        }
        if known {
            Ok(())
        } else {
            Err(fail(
                ZONE,
                "no place has an area: the section should be absent",
            ))
        }
    }
}

/// Encodes a `PLACE_TZ` payload ([`Encoding::TABLE`](crate::Encoding::TABLE)) from each place's
/// timezone area's PLACES row, `None` where it has none; its item count is `zone_of.len()`.
/// Returns `None` when no place has an area: the pack then has no `PLACE_TZ` section.
///
/// # Errors
/// [`Error::Write`] if an area's row is not below `zone_of.len()`.
#[cfg(feature = "write")]
pub fn encode(zone_of: &[Option<u32>]) -> Result<Option<alloc::vec::Vec<u8>>, Error> {
    use alloc::vec::Vec;
    let n = zone_of.len();
    let mut zones: Vec<u32> = zone_of.iter().flatten().copied().collect();
    if zones.is_empty() {
        return Ok(None);
    }
    zones.sort_unstable();
    zones.dedup();
    if zones
        .last()
        .is_some_and(|&z| usize::try_from(z).map_or(true, |z| z >= n))
    {
        return Err(Error::Write("a timezone area that is not a row"));
    }
    let zone: Vec<u32> = zone_of
        .iter()
        .map(|z| {
            z.and_then(|z| zones.binary_search(&z).ok())
                .map_or(0, |i| u32::try_from(i + 1).unwrap_or(u32::MAX))
        })
        .collect();
    let a = teigeodata_core::bitpack::encode_auto(&zones);
    let b = teigeodata_core::bitpack::encode_auto(&zone);
    teigeodata_core::table::encode(&[(ZONES, &a[..]), (ZONE, &b[..])])
        .map(Some)
        .map_err(|_| Error::Write("the timezone table"))
}

/// Encodes `PLACE_TZ` for places at `points` (PLACES' stored points, in row order): each
/// place's area is the one the linked REGION `zones` holds at its point, as
/// [`encode`] returns it.
///
/// # Errors
/// As [`encode`].
#[cfg(feature = "write")]
pub fn encode_from(
    zones: &crate::region::Region<'_>,
    points: &[teigeodata_core::Point<teigeodata_core::E6>],
) -> Result<Option<alloc::vec::Vec<u8>>, Error> {
    let zone_of: alloc::vec::Vec<Option<u32>> = points
        .iter()
        .map(|p| zones.region(p.to_e7()).and_then(|z| zones.place(z)))
        .collect();
    encode(&zone_of)
}

#[cfg(all(test, feature = "write"))]
mod tests {
    use super::{PlaceTz, encode};
    use crate::{Kind, Section};

    #[test]
    fn areas_round_trip_and_none_is_absent() {
        let zone_of = [Some(5), None, Some(2), Some(5), Some(2), None, Some(0)];
        let bytes = encode(&zone_of).unwrap().unwrap();
        let s = Section::unpacked(Kind::PLACE_TZ, zone_of.len() as u64, &bytes);
        let t = PlaceTz::open(&s).unwrap();
        t.check().unwrap();
        assert_eq!((t.len(), t.zones()), (7, 3));
        let back: alloc::vec::Vec<Option<u32>> = (0..7).map(|r| t.get(r)).collect();
        assert_eq!(back, zone_of);
        assert_eq!(t.get(7), None);
        assert_eq!(encode(&[None, None]).unwrap(), None);
        assert!(
            encode(&[Some(2), None]).is_err(),
            "an area that is not a row"
        );
        // A wrong item count, or another kind, is refused.
        let wrong = Section::unpacked(Kind::PLACE_TZ, 6, &bytes);
        assert!(PlaceTz::open(&wrong).is_err());
        let other = Section::unpacked(Kind::PLACES, 7, &bytes);
        assert!(PlaceTz::open(&other).is_err());
    }

    #[test]
    fn the_check_catches_values_past_the_areas_and_rows_out_of_range() {
        let col = |v: &[u32]| teigeodata_core::bitpack::encode_auto(v);
        let table = |zones: &[u32], zone: &[u32]| {
            teigeodata_core::table::encode(&[
                (super::ZONES, &col(zones)[..]),
                (super::ZONE, &col(zone)[..]),
            ])
            .unwrap()
        };
        let check = |zones: &[u32], zone: &[u32]| {
            let b = table(zones, zone);
            let s = Section::unpacked(Kind::PLACE_TZ, zone.len() as u64, &b);
            PlaceTz::open(&s).and_then(|t| t.check())
        };
        assert!(check(&[1], &[1, 0, 1]).is_ok());
        assert!(check(&[1], &[2, 0, 1]).is_err(), "past the areas");
        assert!(check(&[3], &[1, 0, 1]).is_err(), "not a row");
        assert!(check(&[2, 1], &[1, 2, 1]).is_err(), "not increasing");
        assert!(check(&[1], &[0, 0, 0]).is_err(), "no place has one");
    }
}
