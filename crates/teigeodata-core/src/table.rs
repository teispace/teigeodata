//! A table of packed columns (D-074): the one payload layout of every section made of integer
//! columns (PLACES, HIERARCHY, IDMAP, REDIRECTS, and the dense columns of COMPONENTS). Each
//! column is a [`bitpack`](crate::bitpack) column with its own length, named by a 16-bit ID that
//! the section defines.
//!
//! Layout, all little-endian:
//! ```text
//! columns u16, reserved u16 (0)
//! directory [{ id u16, codec u8 (0 = bitpack), reserved u8 (0), len u32 }; columns]
//! bodies    each column's bytes in directory order, back to back
//! ```
//! IDs are strictly increasing and every length is a multiple of 4, so every body starts 4-byte
//! aligned and a table has exactly one encoding. [`Table::parse`] checks the frame in O(columns);
//! [`Table::column`] validates one column when a section opens it.

use core::fmt;

use crate::bitpack::{PackError, Packed};

const HEADER: usize = 4;
const ENTRY: usize = 8;
/// The only codec so far: a [`bitpack`](crate::bitpack) column.
const CODEC_BITPACK: u8 = 0;

/// Why bytes are not a table, or a column of it is missing or invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TableError {
    /// Shorter than the header or directory says.
    Truncated,
    /// Reserved bytes are not zero, a codec is unknown, IDs are not strictly increasing, a
    /// length is not a multiple of 4, or the lengths do not add up to the bodies.
    Directory,
    /// The section needs this column and the table has none.
    Missing(u16),
    /// This column is not a valid packed column.
    Column(u16, PackError),
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("the column table is truncated"),
            Self::Directory => f.write_str("the column table directory is invalid"),
            Self::Missing(id) => write!(f, "the column table has no column {id}"),
            Self::Column(id, e) => write!(f, "column {id}: {e}"),
        }
    }
}

impl core::error::Error for TableError {}

/// A table whose frame is validated, borrowing its bytes.
#[derive(Clone, Copy, Debug)]
pub struct Table<'a> {
    directory: &'a [[u8; ENTRY]],
    bodies: &'a [u8],
}

fn entry(e: [u8; ENTRY]) -> (u16, u8, u8, usize) {
    let [i0, i1, codec, reserved, l0, l1, l2, l3] = e;
    let len = usize::try_from(u32::from_le_bytes([l0, l1, l2, l3])).unwrap_or(usize::MAX);
    (u16::from_le_bytes([i0, i1]), codec, reserved, len)
}

impl<'a> Table<'a> {
    /// Validates the frame: header, directory and lengths. Columns are validated when opened.
    ///
    /// # Errors
    /// [`TableError::Truncated`] or [`TableError::Directory`].
    pub fn parse(bytes: &'a [u8]) -> Result<Self, TableError> {
        let (&[c0, c1, r0, r1], rest) = bytes
            .split_first_chunk::<HEADER>()
            .ok_or(TableError::Truncated)?;
        if (r0, r1) != (0, 0) {
            return Err(TableError::Directory);
        }
        let columns = usize::from(u16::from_le_bytes([c0, c1]));
        let (directory, bodies) = rest
            .split_at_checked(columns * ENTRY)
            .ok_or(TableError::Truncated)?;
        let directory = directory.as_chunks::<ENTRY>().0;
        let mut total: usize = 0;
        let mut last: Option<u16> = None;
        for &e in directory {
            let (id, codec, reserved, len) = entry(e);
            if codec != CODEC_BITPACK || reserved != 0 || len % 4 != 0 || last >= Some(id) {
                return Err(TableError::Directory);
            }
            last = Some(id);
            total = total.checked_add(len).ok_or(TableError::Directory)?;
        }
        if total != bodies.len() {
            return Err(if total > bodies.len() {
                TableError::Truncated
            } else {
                TableError::Directory
            });
        }
        Ok(Self { directory, bodies })
    }

    /// The column IDs, in increasing order.
    pub fn ids(&self) -> impl ExactSizeIterator<Item = u16> + '_ {
        self.directory.iter().map(|&e| entry(e).0)
    }

    /// The bytes of column `id`, or `None` if the table has no such column.
    #[must_use]
    pub fn column_bytes(&self, id: u16) -> Option<&'a [u8]> {
        let mut start = 0;
        for &e in self.directory {
            let (eid, _, _, len) = entry(e);
            if eid == id {
                return self.bodies.get(start..start + len);
            }
            start += len;
        }
        None
    }

    /// Column `id`, validated.
    ///
    /// # Errors
    /// [`TableError::Missing`] if there is no such column; [`TableError::Column`] if it is
    /// invalid.
    pub fn column(&self, id: u16) -> Result<Packed<'a>, TableError> {
        let bytes = self.column_bytes(id).ok_or(TableError::Missing(id))?;
        Packed::parse(bytes).map_err(|e| TableError::Column(id, e))
    }

    /// Column `id` if the table has it (an optional column).
    ///
    /// # Errors
    /// [`TableError::Column`] if it is present and invalid.
    pub fn optional(&self, id: u16) -> Result<Option<Packed<'a>>, TableError> {
        match self.column(id) {
            Ok(p) => Ok(Some(p)),
            Err(TableError::Missing(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Builds a table from encoded columns (feature `alloc`): `(id, bytes)` pairs in strictly
/// increasing ID order, each from [`bitpack::encode`](crate::bitpack::encode).
///
/// # Errors
/// [`TableError::Directory`] if the IDs are not strictly increasing, there are more than 65,535
/// columns, or a column is not a multiple of 4 bytes or above 4 GiB.
#[cfg(feature = "alloc")]
pub fn encode(columns: &[(u16, &[u8])]) -> Result<alloc::vec::Vec<u8>, TableError> {
    let count = u16::try_from(columns.len()).map_err(|_| TableError::Directory)?;
    let body: usize = columns.iter().map(|c| c.1.len()).sum();
    let mut out = alloc::vec::Vec::with_capacity(HEADER + columns.len() * ENTRY + body);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    let mut last: Option<u16> = None;
    for &(id, bytes) in columns {
        let len = u32::try_from(bytes.len()).map_err(|_| TableError::Directory)?;
        if last >= Some(id) || len % 4 != 0 {
            return Err(TableError::Directory);
        }
        last = Some(id);
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&[CODEC_BITPACK, 0]);
        out.extend_from_slice(&len.to_le_bytes());
    }
    for &(_, bytes) in columns {
        out.extend_from_slice(bytes);
    }
    Ok(out)
}

#[cfg(test)]
#[allow(
    clippy::cast_possible_truncation,
    reason = "test inputs are cut from a 64-bit generator on purpose"
)]
mod tests {
    use super::{Table, TableError, encode};
    use crate::bitpack::{self, Coding, PackError};
    use std::vec::Vec;

    fn sample() -> (Vec<u32>, Vec<u32>, Vec<u8>) {
        let a: Vec<u32> = (0..1000).map(|i| i * 3).collect();
        let b: Vec<u32> = (0..77).map(|i| i % 5).collect();
        let (ea, eb) = (
            bitpack::encode(&a, Coding::Delta),
            bitpack::encode(&b, Coding::For),
        );
        let t = encode(&[(1, &ea), (9, &eb)]).unwrap_or_default();
        (a, b, t)
    }

    #[test]
    fn round_trips() {
        let (a, b, bytes) = sample();
        let t = Table::parse(&bytes).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(t.ids().collect::<Vec<_>>(), [1, 9]);
        for (id, want) in [(1, &a), (9, &b)] {
            let col = t.column(id).unwrap_or_else(|e| panic!("{e}"));
            let mut out = std::vec![0; want.len()];
            assert_eq!(col.decode_all(&mut out), want.len());
            assert_eq!(&out, want);
        }
        assert_eq!(t.column(2).err(), Some(TableError::Missing(2)));
        assert!(t.optional(2).is_ok_and(|c| c.is_none()));
        assert!(t.optional(9).is_ok_and(|c| c.is_some()));
        let empty = encode(&[]).unwrap_or_default();
        assert_eq!(Table::parse(&empty).map(|t| t.ids().len()), Ok(0));
    }

    #[test]
    fn rejects() {
        let (_, _, good) = sample();
        let col = bitpack::encode(&[1, 2, 3], Coding::For);
        assert_eq!(
            encode(&[(2, &col), (2, &col)]).err(),
            Some(TableError::Directory)
        );
        assert_eq!(
            encode(&[(3, &col), (2, &col)]).err(),
            Some(TableError::Directory)
        );
        assert_eq!(encode(&[(1, &col[..5])]).err(), Some(TableError::Directory));
        assert_eq!(Table::parse(&good[..3]).err(), Some(TableError::Truncated));
        assert_eq!(
            Table::parse(&good[..good.len() - 4]).err(),
            Some(TableError::Truncated)
        );
        let mut extra = good.clone();
        extra.extend_from_slice(&[0; 4]);
        assert_eq!(Table::parse(&extra).err(), Some(TableError::Directory));
        // Header reserved, then the first entry's codec, reserved, ID order and length.
        for (at, v) in [(2, 1), (6, 1), (7, 1), (12, 1), (8, 3)] {
            let mut b = good.clone();
            b[at] = v;
            assert!(Table::parse(&b).is_err(), "byte {at} = {v}");
        }
        // A valid frame around an invalid column: found when the column is opened.
        let mut b = good;
        b[4 + 16 + 4] = 9; // the first column's lanes byte
        let t = Table::parse(&b).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            t.column(1).err(),
            Some(TableError::Column(1, PackError::Header))
        );
    }

    #[test]
    fn never_panics() {
        let (_, _, base) = sample();
        let mut s: u64 = 7;
        let mut out = std::vec![0; 2048];
        for _ in 0..20_000 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let mut b = base.clone();
            let at = s as usize % b.len();
            b[at] = (s >> 32) as u8;
            let cut = (s >> 16) as usize % (b.len() + 1);
            if let Ok(t) = Table::parse(&b[..cut]) {
                for id in t.ids().collect::<Vec<_>>() {
                    if let Ok(c) = t.column(id) {
                        let _ = c.decode_all(&mut out);
                        let _ = c.get(at % (c.len() + 1));
                    }
                }
            }
        }
    }
}
