//! COMPONENTS (kind 12): optional per-place fields, grouped into components (B6, D-076). A row
//! without a component costs at most about a bit; a field stores values only for the rows that
//! have the component.
//!
//! A [column table](teigeodata_core::table) whose column IDs are `component << 8 | field`
//! ([`column()`]). Component 0 is reserved, and components above [`MAX_COMPONENT`] (127) are
//! extension columns (`table::EXTENSION` and up, D-335), which this reader skips. For each
//! component:
//!
//! - field 0, [`PRESENCE`], says which rows have it, in the smallest of three forms (the writer
//!   picks; B6 measured the crossover near 10% of rows):
//!   - absent: every row;
//!   - a [bitmap](teigeodata_core::bits) of one bit per row (codec 1), when many rows have it;
//!   - the sorted present rows, a packed column (codec 0), when few do.
//! - fields 1 to 255 are packed columns of one value per present row, in row order: the value
//!   of row `r` is at the row's index among the present rows ([`Component::index`]).
//!
//! What each component and field means is the schema's
//! ([components](https://github.com/teispace/teigeodata/blob/main/docs/05-data/schema.md#components)); this module stores integers.
//! [`Components::open`] checks the frame and every column's structure and length;
//! [`Components::check`] also proves each presence column. Reads never panic either way.

use teigeodata_core::bitpack::Packed;
use teigeodata_core::bits::Bits;
use teigeodata_core::table::{Codec, Table};

use crate::sorted::Sorted;
use crate::{Error, Kind, Section, columns};

/// The presence field of every component.
pub const PRESENCE: u8 = 0;

/// The largest component ID: from 128 up, `component << 8` is an extension column ID
/// (`table::EXTENSION`, D-335), skipped by a reader that does not know it.
pub const MAX_COMPONENT: u8 = 127;

/// The column ID of `field` of `component`.
#[must_use]
pub const fn column(component: u8, field: u8) -> u16 {
    (component as u16) << 8 | field as u16
}

const fn split(id: u16) -> (u8, u8) {
    #[allow(clippy::cast_possible_truncation, reason = "the two bytes of a u16")]
    ((id >> 8) as u8, id as u8)
}

fn fail(column: u16, problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::COMPONENTS.0,
        column,
        problem,
    }
}

/// Which rows have a component.
#[derive(Clone, Copy, Debug)]
enum Presence<'a> {
    All,
    Bitmap(Bits<'a>),
    Rows(Sorted<'a>),
}

/// The COMPONENTS section of an open pack.
#[derive(Clone, Copy, Debug)]
pub struct Components<'a> {
    table: Table<'a>,
    rows: usize,
}

/// One component of a [`Components`] section.
#[derive(Clone, Copy, Debug)]
pub struct Component<'a> {
    table: Table<'a>,
    id: u8,
    rows: usize,
    presence: Presence<'a>,
    count: usize,
}

impl<'a> Components<'a> {
    /// Opens the section and checks every column: structure, codec and length, in
    /// O(columns + blocks).
    ///
    /// # Errors
    /// [`Error::Payload`] if the section is not a COMPONENTS column table, uses component 0, or
    /// a column is malformed, of the wrong codec or of the wrong length.
    pub fn open(section: &Section<'a>) -> Result<Self, Error> {
        let table = columns::open(section, Kind::COMPONENTS)?;
        let rows = usize::try_from(section.item_count())
            .map_err(|_| fail(0, "the row count does not fit this platform"))?;
        let this = Self { table, rows };
        // IDs are increasing, so each component's columns are adjacent: open each once.
        let mut last = None;
        for id in table.ids() {
            let (c, _) = split(id);
            if c == 0 {
                return Err(fail(id, "component 0 is reserved"));
            }
            if last != Some(c) {
                this.open_component(c)?;
                last = Some(c);
            }
        }
        Ok(this)
    }

    /// The number of rows (places).
    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }

    /// The component IDs present, in increasing order.
    pub fn ids(&self) -> impl Iterator<Item = u8> + '_ {
        let mut last = None;
        self.table.ids().filter_map(move |id| {
            let c = split(id).0;
            (last != Some(c)).then(|| {
                last = Some(c);
                c
            })
        })
    }

    /// Component `id`, or `None` if the pack has none of it. Opening one revalidates its
    /// columns (O(blocks)): open it once and keep it for every read.
    #[must_use]
    pub fn component(&self, id: u8) -> Option<Component<'a>> {
        self.open_component(id).ok().flatten()
    }

    fn open_component(&self, c: u8) -> Result<Option<Component<'a>>, Error> {
        let table = &self.table;
        let presence_id = column(c, PRESENCE);
        let map = |e| columns::map(Kind::COMPONENTS, e);
        let (presence, count) = match table.column_raw(presence_id).map(|r| r.0) {
            None => (Presence::All, self.rows),
            Some(Codec::Bits) => {
                let bits = table.bits(presence_id).map_err(map)?;
                if bits.len() != self.rows {
                    return Err(fail(
                        presence_id,
                        "the bitmap length differs from the row count",
                    ));
                }
                (Presence::Bitmap(bits), bits.count())
            }
            Some(_) => {
                let lo = table.column(presence_id).map_err(map)?;
                (Presence::Rows(Sorted { hi: None, lo }), lo.len())
            }
        };
        let mut fields = table
            .ids()
            .filter(|&id| split(id).0 == c && split(id).1 != PRESENCE)
            .peekable();
        if fields.peek().is_none() {
            if matches!(presence, Presence::All) {
                return Ok(None);
            }
            return Err(fail(presence_id, "a component with no fields"));
        }
        for id in fields {
            columns::required(table, Kind::COMPONENTS, id, Some(count))?;
        }
        Ok(Some(Component {
            table: *table,
            id: c,
            rows: self.rows,
            presence,
            count,
        }))
    }

    /// Checks every component's presence: a bitmap's rank directory, sorted rows strictly
    /// increasing and below the row count. O(rows) per component.
    ///
    /// # Errors
    /// [`Error::Payload`] naming the presence column.
    pub fn check(&self) -> Result<(), Error> {
        for c in self.ids() {
            let Some(comp) = self.component(c) else {
                continue;
            };
            let id = column(c, PRESENCE);
            match comp.presence {
                Presence::All => {}
                Presence::Bitmap(bits) => bits
                    .check()
                    .map_err(|_| fail(id, "the bitmap rank directory is wrong"))?,
                Presence::Rows(sorted) => {
                    let below = sorted.iter().last().is_none_or(|r| r < self.rows as u64);
                    if !sorted.is_strictly_increasing() || !below {
                        return Err(fail(id, "the present rows are not increasing row numbers"));
                    }
                }
            }
        }
        Ok(())
    }
}

impl<'a> Component<'a> {
    /// The component's ID.
    #[must_use]
    pub const fn id(&self) -> u8 {
        self.id
    }

    /// The number of rows that have it.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether no row has it.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// If `row` has the component, the row's index among those that do: where its values are
    /// in every field. O(1) for a bitmap, a search of the sorted rows otherwise.
    #[must_use]
    pub fn index(&self, row: usize) -> Option<usize> {
        if row >= self.rows {
            return None;
        }
        match self.presence {
            Presence::All => Some(row),
            Presence::Bitmap(bits) => bits.index(row),
            Presence::Rows(sorted) => sorted.find(row as u64),
        }
    }

    /// Field `field` (1 to 255); `None` if the component has no such field. Opening a field
    /// validates its column (O(blocks)): open it once and keep it for every read.
    #[must_use]
    pub fn field(&self, field: u8) -> Option<Field<'a>> {
        if field == PRESENCE {
            return None;
        }
        let values = self.table.column(column(self.id, field)).ok()?;
        Some(Field {
            component: *self,
            values,
        })
    }
}

/// One field of a [`Component`]: one value per present row.
#[derive(Clone, Copy, Debug)]
pub struct Field<'a> {
    component: Component<'a>,
    values: Packed<'a>,
}

impl<'a> Field<'a> {
    /// The value for `row`; `None` if the row does not have the component.
    #[must_use]
    pub fn get(&self, row: usize) -> Option<u32> {
        self.values.get(self.component.index(row)?)
    }

    /// The values, one per present row in row order.
    #[must_use]
    pub const fn values(&self) -> Packed<'a> {
        self.values
    }
}

impl Component<'_> {
    /// The field numbers present, increasing (presence left out).
    pub fn fields(&self) -> impl Iterator<Item = u8> + '_ {
        let id = self.id;
        self.table
            .ids()
            .map(split)
            .filter(move |&(c, f)| c == id && f != PRESENCE)
            .map(|(_, f)| f)
    }
}

/// Component 1, `population` (D-351): the number of people of a place.
pub const POPULATION: u8 = 1;
/// Field 1 of [`POPULATION`]: the number of people.
pub const PEOPLE: u8 = 1;
/// Field 2 of [`POPULATION`]: where the number came from, a [`PopulationSource`] code (0:
/// unstated).
pub const PEOPLE_SOURCE: u8 = 2;

/// Where a place's number of people came from (D-351).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
#[non_exhaustive]
pub enum PopulationSource {
    /// Stated: a source's count for the place (a census or an estimate, as the source gives
    /// it), kept because the population grid agrees with it within a factor of 1.5 where the
    /// place is an admin unit (D-123).
    Stated = 1,
    /// Modelled: a population grid summed over the unit (D-123).
    Modelled = 2,
}

impl PopulationSource {
    /// The stored code.
    #[must_use]
    pub const fn code(self) -> u32 {
        self as u32
    }

    /// The source of a stored code; `None` for 0 (unstated) and unknown codes.
    #[must_use]
    pub const fn from_code(code: u32) -> Option<Self> {
        match code {
            1 => Some(Self::Stated),
            2 => Some(Self::Modelled),
            _ => None,
        }
    }

    /// Its name, as reports and bindings spell it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Stated => "stated",
            Self::Modelled => "modelled",
        }
    }
}

/// The [`POPULATION`] component of an open COMPONENTS section.
#[derive(Clone, Copy, Debug)]
pub struct Populations<'a> {
    people: Field<'a>,
    source: Option<Field<'a>>,
}

impl<'a> Populations<'a> {
    /// The population component of `components`; `None` if it has none, or the component has no
    /// [`PEOPLE`] field.
    #[must_use]
    pub fn of(components: &Components<'a>) -> Option<Self> {
        let c = components.component(POPULATION)?;
        Some(Self {
            people: c.field(PEOPLE)?,
            source: c.field(PEOPLE_SOURCE),
        })
    }

    /// The number of people of `row`, and where it came from; `None` if unknown.
    #[must_use]
    pub fn get(&self, row: usize) -> Option<(u32, Option<PopulationSource>)> {
        let n = self.people.get(row)?;
        let source = self
            .source
            .and_then(|s| s.get(row))
            .and_then(PopulationSource::from_code);
        Some((n, source))
    }
}

/// A COMPONENTS payload holding only [`POPULATION`], one entry per row (feature `write`);
/// `None` when no row has a population.
///
/// # Errors
/// As [`encode`].
#[cfg(feature = "write")]
pub fn encode_populations(
    people: &[Option<(u32, PopulationSource)>],
) -> Result<Option<alloc::vec::Vec<u8>>, Error> {
    use alloc::vec::Vec;
    let mut rows = Vec::new();
    let mut counts = Vec::new();
    let mut sources = Vec::new();
    for (r, p) in people.iter().enumerate() {
        if let Some((n, s)) = p {
            rows.push(u32::try_from(r).map_err(|_| Error::Write("too many rows"))?);
            counts.push(*n);
            sources.push(s.code());
        }
    }
    if rows.is_empty() {
        return Ok(None);
    }
    let fields: [(u8, &[u32]); 2] = [(PEOPLE, &counts), (PEOPLE_SOURCE, &sources)];
    encode(
        people.len(),
        &[ComponentColumns {
            id: POPULATION,
            rows: &rows,
            fields: &fields,
        }],
    )
    .map(Some)
}

/// A component to write: its ID, the rows that have it (strictly increasing), and its fields,
/// each `(field, values)` with one value per present row, fields strictly increasing from 1.
#[cfg(feature = "write")]
#[derive(Clone, Copy, Debug)]
pub struct ComponentColumns<'v> {
    /// The component ID, 1 to [`MAX_COMPONENT`].
    pub id: u8,
    /// The rows that have it.
    pub rows: &'v [u32],
    /// Its fields.
    pub fields: &'v [(u8, &'v [u32])],
}

/// Encodes a COMPONENTS payload for `rows` places (feature `write`), components in increasing
/// ID order. Each presence takes the smallest form: none when every row has the component, else
/// the smaller of the bitmap and the sorted rows.
///
/// # Errors
/// [`Error::Write`] if IDs are 0 or out of order, present rows are not strictly increasing or
/// not below `rows`, a component has no fields, or a field has the wrong number of values.
#[cfg(feature = "write")]
pub fn encode(
    rows: usize,
    components: &[ComponentColumns<'_>],
) -> Result<alloc::vec::Vec<u8>, Error> {
    use alloc::vec::Vec;
    use teigeodata_core::bitpack::encode_auto;
    use teigeodata_core::bits;
    use teigeodata_core::table::encode_with;

    let refuse = Error::Write;
    let mut bodies: Vec<(u16, Codec, Vec<u8>)> = Vec::new();
    let mut last = 0;
    for comp in components {
        if comp.id <= last {
            return Err(refuse("component IDs must be increasing and above 0"));
        }
        if comp.id > MAX_COMPONENT {
            return Err(refuse("component IDs above 127 are extension columns"));
        }
        last = comp.id;
        // Rows at or past `rows` are refused by the bitmap's encoder, which every component
        // with a row missing goes through.
        if !comp.rows.is_sorted_by(|a, b| a < b) {
            return Err(refuse("present rows must be increasing"));
        }
        if comp.fields.is_empty() {
            return Err(refuse("a component needs a field"));
        }
        if comp.rows.len() != rows {
            let bitmap = bits::encode(rows, comp.rows.iter().map(|&r| r as usize))
                .map_err(|_| refuse("present rows must be below the row count, below 2^32"))?;
            let sorted = encode_auto(comp.rows);
            let id = column(comp.id, PRESENCE);
            bodies.push(if bitmap.len() <= sorted.len() {
                (id, Codec::Bits, bitmap)
            } else {
                (id, Codec::Bitpack, sorted)
            });
        }
        let mut last_field = PRESENCE;
        for &(field, values) in comp.fields {
            if field <= last_field {
                return Err(refuse("field IDs must be increasing and above 0"));
            }
            last_field = field;
            if values.len() != comp.rows.len() {
                return Err(refuse("a field needs one value per present row"));
            }
            bodies.push((column(comp.id, field), Codec::Bitpack, encode_auto(values)));
        }
    }
    let refs: Vec<(u16, Codec, &[u8])> = bodies
        .iter()
        .map(|(i, c, b)| (*i, *c, b.as_slice()))
        .collect();
    encode_with(&refs).map_err(|_| refuse("too many columns or a column too large"))
}
