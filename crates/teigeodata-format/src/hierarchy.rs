//! HIERARCHY (kind 3): each place's parent row and each place's children, by row number in
//! PLACES (D-074).
//!
//! A [column table](teigeodata_core::table) with three columns, all required:
//!
//! | ID | Column | Values |
//! |---:|---|---|
//! | 1 | [`PARENT`] | parent row + 1; 0 for a root (a country, or a place with no parent) |
//! | 2 | [`CHILD_END`] | for each row, the end of its children in [`CHILDREN`] (a running count) |
//! | 3 | [`CHILDREN`] | child rows, grouped by parent row, ascending within a group |
//!
//! The children of row `r` are `CHILDREN[CHILD_END[r - 1] .. CHILD_END[r]]` (from 0 for row 0):
//! the CSR layout. The running counts compress as deltas; B1 compared them with Elias–Fano.

use teigeodata_core::bitpack::{Packed, Values};
use teigeodata_core::table::Table;

use crate::{Error, Kind, Section, columns};

/// Parent column.
pub const PARENT: u16 = 1;
/// Running end of each row's children.
pub const CHILD_END: u16 = 2;
/// Child rows.
pub const CHILDREN: u16 = 3;

/// The deepest chain of parents a pack may hold: far above any admin ladder (Nepal has 5
/// levels), and a bound on every walk up the tree, so bad data cannot loop.
pub const MAX_DEPTH: usize = 32;

/// The HIERARCHY section of an open pack.
#[derive(Clone, Copy, Debug)]
pub struct Hierarchy<'a> {
    parent: Packed<'a>,
    child_end: Packed<'a>,
    children: Packed<'a>,
}

fn fail(column: u16, problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::HIERARCHY.0,
        column,
        problem,
    }
}

impl<'a> Hierarchy<'a> {
    /// Opens the section: its frame, the structure of every column, and the lengths, in
    /// O(columns + blocks).
    ///
    /// # Errors
    /// [`Error::Payload`] if the section is not a HIERARCHY column table, a column is missing or
    /// malformed, or the lengths disagree.
    pub fn open(section: &Section<'a>) -> Result<Self, Error> {
        let table: Table<'a> = columns::open(section, Kind::HIERARCHY)?;
        let rows = usize::try_from(section.item_count())
            .map_err(|_| fail(0, "the row count does not fit this platform"))?;
        let parent = columns::required(&table, Kind::HIERARCHY, PARENT, Some(rows))?;
        let child_end = columns::required(&table, Kind::HIERARCHY, CHILD_END, Some(rows))?;
        let children = columns::required(&table, Kind::HIERARCHY, CHILDREN, None)?;
        if table.ids().len() != 3 {
            return Err(fail(0, "an unknown column"));
        }
        let total = rows
            .checked_sub(1)
            .and_then(|r| child_end.get(r))
            .unwrap_or(0);
        if usize::try_from(total).ok() != Some(children.len()) {
            return Err(fail(
                CHILD_END,
                "the children count differs from the children column",
            ));
        }
        Ok(Self {
            parent,
            child_end,
            children,
        })
    }

    /// The number of rows (equal to PLACES').
    #[must_use]
    pub const fn len(&self) -> usize {
        self.parent.len()
    }

    /// Whether there are none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.parent.is_empty()
    }

    /// The parent row of `row`; `None` for a root or past the end.
    #[must_use]
    pub fn parent(&self, row: usize) -> Option<usize> {
        let v = self.parent.get(row)?.checked_sub(1)?;
        usize::try_from(v).ok()
    }

    /// The child rows of `row`, ascending; empty past the end.
    #[must_use]
    pub fn children(&self, row: usize) -> Values<'a> {
        let end = self.end(row);
        let start = row.checked_sub(1).map_or(0, |r| self.end(r)).min(end);
        self.children.values(start..end)
    }

    fn end(&self, row: usize) -> usize {
        self.child_end
            .get(row)
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(0)
    }

    /// The parent, grandparent and so on of `row`, at most [`MAX_DEPTH`] of them.
    #[must_use]
    pub fn ancestors(&self, row: usize) -> Ancestors<'a> {
        Ancestors {
            hierarchy: *self,
            row,
            left: MAX_DEPTH,
        }
    }

    /// Checks that the columns describe one forest: every parent is a row other than itself;
    /// the running counts never fall; each row's children are exactly the rows naming it as
    /// parent, ascending; and every chain of parents ends within [`MAX_DEPTH`]. O(rows ×
    /// depth); the forge and the tests run it.
    ///
    /// # Errors
    /// [`Error::Payload`] naming the column.
    pub fn check(&self) -> Result<(), Error> {
        let n = self.len();
        let mut start = 0;
        let mut listed = 0_usize;
        for (row, end) in self.child_end.values(0..n).enumerate() {
            let end = usize::try_from(end).unwrap_or(usize::MAX);
            if end < start || end > self.children.len() {
                return Err(fail(
                    CHILD_END,
                    "the running children count falls or overruns",
                ));
            }
            let mut prev = None;
            for child in self.children.values(start..end) {
                let child = usize::try_from(child).unwrap_or(usize::MAX);
                if prev >= Some(child) || self.parent(child) != Some(row) {
                    return Err(fail(
                        CHILDREN,
                        "a child list is unsorted or names a non-child",
                    ));
                }
                prev = Some(child);
            }
            listed += end - start;
            start = end;
        }
        let mut with_parent = 0_usize;
        for (row, p) in self.parent.values(0..n).enumerate() {
            if p == 0 {
                continue;
            }
            with_parent += 1;
            let p = usize::try_from(p - 1).unwrap_or(usize::MAX);
            if p >= n || p == row {
                return Err(fail(PARENT, "a parent is out of range or the row itself"));
            }
            if self.ancestors(row).count() == MAX_DEPTH
                && self
                    .ancestors(row)
                    .last()
                    .and_then(|a| self.parent(a))
                    .is_some()
            {
                return Err(fail(
                    PARENT,
                    "a chain of parents is a cycle or deeper than allowed",
                ));
            }
        }
        if with_parent != listed {
            return Err(fail(
                CHILDREN,
                "a row with a parent is missing from its children",
            ));
        }
        Ok(())
    }
}

/// The ancestors of a row ([`Hierarchy::ancestors`]).
#[derive(Clone, Debug)]
pub struct Ancestors<'a> {
    hierarchy: Hierarchy<'a>,
    row: usize,
    left: usize,
}

impl Iterator for Ancestors<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        self.left = self.left.checked_sub(1)?;
        self.row = self.hierarchy.parent(self.row)?;
        Some(self.row)
    }
}

/// Encodes a HIERARCHY payload ([`Encoding::TABLE`](crate::Encoding::TABLE)) from each row's
/// parent (feature `write`); its item count is `parents.len()`.
///
/// # Errors
/// [`Error::Write`] if a parent is out of range or the row itself, or a chain of parents is a
/// cycle or deeper than [`MAX_DEPTH`].
#[cfg(feature = "write")]
pub fn encode(parents: &[Option<u32>]) -> Result<alloc::vec::Vec<u8>, Error> {
    use alloc::vec::Vec;
    use teigeodata_core::bitpack::encode_auto;

    let n = parents.len();
    let mut ends = alloc::vec![0_u32; n];
    for (row, p) in parents.iter().enumerate() {
        if let Some(p) = *p {
            let slot = usize::try_from(p).ok().filter(|&p| p != row);
            let end = slot
                .and_then(|p| ends.get_mut(p))
                .ok_or(Error::Write("a parent is out of range or the row itself"))?;
            *end += 1;
        }
        // At most MAX_DEPTH steps up: a cycle never ends, so it is caught as too deep.
        let mut at = row;
        for _ in 0..MAX_DEPTH {
            match parents.get(at).copied().flatten() {
                Some(up) => at = usize::try_from(up).unwrap_or(usize::MAX),
                None => break,
            }
        }
        if parents.get(at).copied().flatten().is_some() {
            return Err(Error::Write("a chain of parents is a cycle or too deep"));
        }
    }
    // Counts to running ends; then each child goes to the next free slot of its parent, so
    // groups come out ascending because rows are visited in order.
    let mut total = 0_u32;
    for e in &mut ends {
        total += *e;
        *e = total;
    }
    let mut next: Vec<u32> = core::iter::once(0)
        .chain(ends.iter().copied())
        .take(n)
        .collect();
    let mut children = alloc::vec![0_u32; total as usize];
    for (row, p) in parents.iter().enumerate() {
        if let Some(slot) = p.and_then(|p| next.get_mut(p as usize)) {
            if let Some(c) = children.get_mut(*slot as usize) {
                *c = u32::try_from(row).map_err(|_| Error::Write("more than 2^32 rows"))?;
            }
            *slot += 1;
        }
    }
    let parent: Vec<u32> = parents.iter().map(|p| p.map_or(0, |p| p + 1)).collect();
    let cols = [
        (PARENT, encode_auto(&parent)),
        (CHILD_END, encode_auto(&ends)),
        (CHILDREN, encode_auto(&children)),
    ];
    let refs: Vec<(u16, &[u8])> = cols.iter().map(|(id, b)| (*id, &b[..])).collect();
    teigeodata_core::table::encode(&refs).map_err(|_| Error::Write("the hierarchy table"))
}
