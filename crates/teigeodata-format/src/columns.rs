//! Opening a column-table section ([`Encoding::TABLE`]): one place that checks the frame and maps
//! the codec's errors, for every section made of integer columns.

use teigeodata_core::bitpack::{PackError, Packed};
use teigeodata_core::table::{Table, TableError};

use crate::{Encoding, Error, Kind, Section};

/// The section's table, after checking its kind and encoding.
pub(crate) fn open<'a>(section: &Section<'a>, kind: Kind) -> Result<Table<'a>, Error> {
    let fail = |problem| Error::Payload {
        kind: kind.0,
        column: 0,
        problem,
    };
    if section.kind() != kind {
        return Err(fail("not this kind of section"));
    }
    if section.encoding() != Encoding::TABLE {
        return Err(fail("not a column table"));
    }
    Table::parse(section.bytes()).map_err(|e| map(kind, e))
}

/// Column `id`, which the section requires, with `rows` values (`None`: any length).
pub(crate) fn required<'a>(
    table: &Table<'a>,
    kind: Kind,
    id: u16,
    rows: Option<usize>,
) -> Result<Packed<'a>, Error> {
    let col = table.column(id).map_err(|e| map(kind, e))?;
    check_len(kind, id, col, rows)
}

/// Column `id` if present, with `rows` values.
pub(crate) fn optional<'a>(
    table: &Table<'a>,
    kind: Kind,
    id: u16,
    rows: usize,
) -> Result<Option<Packed<'a>>, Error> {
    match table.optional(id).map_err(|e| map(kind, e))? {
        Some(col) => check_len(kind, id, col, Some(rows)).map(Some),
        None => Ok(None),
    }
}

fn check_len(
    kind: Kind,
    id: u16,
    col: Packed<'_>,
    rows: Option<usize>,
) -> Result<Packed<'_>, Error> {
    match rows {
        Some(n) if col.len() != n => Err(Error::Payload {
            kind: kind.0,
            column: id,
            problem: "the column length differs from the row count",
        }),
        _ => Ok(col),
    }
}

pub(crate) fn map(kind: Kind, e: TableError) -> Error {
    let (column, problem) = match e {
        TableError::Truncated => (0, "the column table is truncated"),
        TableError::Missing(id) => (id, "a required column is missing"),
        TableError::Column(id, PackError::Truncated) => (id, "the column is truncated"),
        TableError::Column(id, PackError::Header) => (id, "the column header is invalid"),
        TableError::Column(id, _) => (id, "the column directory is inconsistent"),
        TableError::Bits(id, _) => (id, "the bitmap is invalid"),
        TableError::Codec(id) => (id, "the column has the wrong codec"),
        _ => (0, "the column table directory is invalid"),
    };
    Error::Payload {
        kind: kind.0,
        column,
        problem,
    }
}
