//! NAMES (kind 4): every place's name records, each with its language, script, type, validity
//! and text, the text compressed with FSST (B3, D-077). A place's records are adjacent; its
//! first record is its canonical name (the schema's required `name`), so a display name is one
//! O(1) read and one decode of about 10 bytes.
//!
//! A [column table](teigeodata_core::table):
//!
//! | ID | Column | Values | |
//! |---:|---|---|---|
//! | 1 | [`FIRST`] | per place and one more: its first record; place `r` has records `FIRST[r]..FIRST[r + 1]`, at least one | required |
//! | 2 | [`TEXT_END`] | per record: where its text ends in [`HEAP`]; it starts where the previous one ends | required |
//! | 3 | [`TABLE`] | per record: the index of its symbol table in [`TABLES`] | required |
//! | 4 | [`LANG`] | per record: the BCP 47 language as a [`Tag`], 0 = undetermined | optional |
//! | 5 | [`SCRIPT`] | per record: the ISO 15924 script as a [`Tag`], 0 = unknown | optional |
//! | 6 | [`TYPE`] | per record: the [`NameType`] code, 0 = unspecified | optional |
//! | 7, 8 | [`VALID_FROM`], [`VALID_TO`] | per record: a day ([biased](teigeodata_core::bitpack::bias) days since 1970-01-01), 0 = open | optional |
//! | 9 | [`RULE`] | per record: which precedence step chose a canonical name, 0 = none | optional |
//! | 10 | [`TABLES`] | bytes: a count u32, then each FSST table, back to back | required |
//! | 11 | [`HEAP`] | bytes: every record's codes, back to back, zero-padded to a multiple of 4 | required |
//!
//! Optional columns that are absent read as 0. [`Names::open`] checks the frame, every column's
//! structure and length, and every table; [`Names::check`] also checks every value and that
//! every text decodes to UTF-8. Reads never panic either way.

use core::fmt;
use core::ops::Range;

use teigeodata_core::bitpack::{Packed, unbias};
use teigeodata_core::fsst::{FsstError, Symbols};
use teigeodata_core::table::Table;

use crate::{Error, Kind, Section, columns};

/// Each place's first record, and one past the last place's.
pub const FIRST: u16 = 1;
/// Where each record's text ends.
pub const TEXT_END: u16 = 2;
/// Each record's symbol table.
pub const TABLE: u16 = 3;
/// Each record's language.
pub const LANG: u16 = 4;
/// Each record's script.
pub const SCRIPT: u16 = 5;
/// Each record's type.
pub const TYPE: u16 = 6;
/// The first day a record is valid.
pub const VALID_FROM: u16 = 7;
/// The last day a record is valid.
pub const VALID_TO: u16 = 8;
/// The precedence rule behind a canonical name.
pub const RULE: u16 = 9;
/// The symbol tables.
pub const TABLES: u16 = 10;
/// The compressed text.
pub const HEAP: u16 = 11;

/// The most symbol tables a section holds: a shared one and one per large script group.
pub const MAX_TABLES: usize = 16;
/// The most codes in one name's text.
pub const MAX_CODES: usize = 255;
/// A buffer this long holds any name's text ([`Name::text`]): 8 bytes per code at most.
pub const TEXT_BUFFER: usize = 8 * MAX_CODES;

const OPTIONAL: [u16; 6] = [LANG, SCRIPT, TYPE, VALID_FROM, VALID_TO, RULE];

/// A language or script code of up to four ASCII letters or digits, packed little-endian into
/// a u32 (`"ne"`, `"mai"`, `"Deva"`): every ISO 639 language and ISO 15924 script fits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tag(u32);

impl Tag {
    /// The tag of `code`; `None` if it is empty, longer than 4 bytes, or not ASCII letters and
    /// digits.
    #[must_use]
    pub fn new(code: &str) -> Option<Self> {
        let b = code.as_bytes();
        if b.is_empty() || b.len() > 4 || !b.iter().all(u8::is_ascii_alphanumeric) {
            return None;
        }
        let mut word = [0; 4];
        word.get_mut(..b.len())?.copy_from_slice(b);
        Some(Self(u32::from_le_bytes(word)))
    }

    /// The tag stored as `v`; `None` for 0 (none) or a value no [`Tag::new`] makes.
    #[must_use]
    pub fn from_u32(v: u32) -> Option<Self> {
        let b = v.to_le_bytes();
        let len = b.iter().position(|&x| x == 0).unwrap_or(4);
        let (used, rest) = b.split_at(len);
        (len > 0 && used.iter().all(u8::is_ascii_alphanumeric) && rest.iter().all(|&x| x == 0))
            .then_some(Self(v))
    }

    /// The stored value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// The code's bytes and their length.
    #[must_use]
    pub fn to_bytes(self) -> ([u8; 4], usize) {
        let b = self.0.to_le_bytes();
        (b, b.iter().position(|&x| x == 0).unwrap_or(4))
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (b, n) = self.to_bytes();
        for &c in b.get(..n).unwrap_or_default() {
            write!(f, "{}", char::from(c))?;
        }
        Ok(())
    }
}

/// The type of a name ([schema](https://github.com/teispace/teigeodata/blob/main/docs/05-data/schema.md#names)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum NameType {
    /// The official name.
    Official,
    /// The local name.
    Local,
    /// Another spelling, including every spelling the canonical one won against.
    Alt,
    /// A colloquial name.
    Colloquial,
    /// A historical name.
    Historical,
    /// A former official name.
    FormerOfficial,
}

impl NameType {
    /// Every type, in code order (codes 1 to 6).
    pub const ALL: [Self; 6] = [
        Self::Official,
        Self::Local,
        Self::Alt,
        Self::Colloquial,
        Self::Historical,
        Self::FormerOfficial,
    ];

    /// The stored code, 1 to 6.
    #[must_use]
    pub const fn code(self) -> u32 {
        match self {
            Self::Official => 1,
            Self::Local => 2,
            Self::Alt => 3,
            Self::Colloquial => 4,
            Self::Historical => 5,
            Self::FormerOfficial => 6,
        }
    }

    /// The type of `code`; `None` for 0 (unspecified) or an unknown code.
    #[must_use]
    pub fn from_code(code: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.code() == code)
    }

    /// The schema's name of the type.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::Local => "local",
            Self::Alt => "alt",
            Self::Colloquial => "colloquial",
            Self::Historical => "historical",
            Self::FormerOfficial => "former_official",
        }
    }
}

fn fail(column: u16, problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::NAMES.0,
        column,
        problem,
    }
}

/// The NAMES section of an open pack.
#[derive(Clone, Copy, Debug)]
pub struct Names<'a> {
    first: Packed<'a>,
    text_end: Packed<'a>,
    table: Packed<'a>,
    optional: [Option<Packed<'a>>; 6],
    tables: [Option<Symbols<'a>>; MAX_TABLES],
    heap: &'a [u8],
}

/// One name record, borrowing its section's view.
#[derive(Clone, Copy, Debug)]
pub struct Name<'n, 'a> {
    names: &'n Names<'a>,
    record: usize,
}

impl<'a> Names<'a> {
    /// Opens the section: the frame, every column's structure and length, and every symbol
    /// table, in O(columns + blocks + tables).
    ///
    /// # Errors
    /// [`Error::Payload`] if the section is not a NAMES column table or a column or table is
    /// missing, malformed or of the wrong length.
    pub fn open(section: &Section<'a>) -> Result<Self, Error> {
        let t: Table<'a> = columns::open(section, Kind::NAMES)?;
        let rows = usize::try_from(section.item_count())
            .map_err(|_| fail(0, "the row count does not fit this platform"))?;
        let first = columns::required(&t, Kind::NAMES, FIRST, rows.checked_add(1))?;
        let text_end = columns::required(&t, Kind::NAMES, TEXT_END, None)?;
        let records = text_end.len();
        let table = columns::required(&t, Kind::NAMES, TABLE, Some(records))?;
        let mut optional = [None; 6];
        for (slot, id) in optional.iter_mut().zip(OPTIONAL) {
            *slot = columns::optional(&t, Kind::NAMES, id, records)?;
        }
        let bytes = |id| t.bytes(id).map_err(|e| columns::map(Kind::NAMES, e));
        let (tables_bytes, heap) = (bytes(TABLES)?, bytes(HEAP)?);
        if t.ids().any(|id| id == 0 || id > HEAP) {
            return Err(fail(0, "an unknown column"));
        }
        Ok(Self {
            first,
            text_end,
            table,
            optional,
            tables: parse_tables(tables_bytes)?,
            heap,
        })
    }

    /// The number of places.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.first.len().saturating_sub(1)
    }

    /// Whether there are no places.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The number of name records.
    #[must_use]
    pub const fn records(&self) -> usize {
        self.text_end.len()
    }

    /// The records of place `row`, its canonical name first; empty past the end.
    #[must_use]
    pub fn of(&self, row: usize) -> Range<usize> {
        let (Some(a), Some(b)) = (
            self.first.get(row),
            row.checked_add(1).and_then(|r| self.first.get(r)),
        ) else {
            return 0..0;
        };
        let (a, b) = (a as usize, b as usize);
        a..b.max(a).min(self.records())
    }

    /// The names of place `row`, its canonical name first.
    pub fn names(&self, row: usize) -> impl Iterator<Item = Name<'_, 'a>> + '_ {
        self.of(row).map(move |record| Name {
            names: self,
            record,
        })
    }

    /// The canonical name of place `row`.
    #[must_use]
    pub fn canonical(&self, row: usize) -> Option<Name<'_, 'a>> {
        self.names(row).next()
    }

    /// Record `record`.
    #[must_use]
    pub fn record(&self, record: usize) -> Option<Name<'_, 'a>> {
        (record < self.records()).then_some(Name {
            names: self,
            record,
        })
    }

    fn optional(&self, slot: usize, record: usize) -> u32 {
        self.optional
            .get(slot)
            .copied()
            .flatten()
            .and_then(|c| c.get(record))
            .unwrap_or(0)
    }

    /// The codes of `record` and its table.
    fn codes(&self, record: usize) -> Result<(&'a [u8], Symbols<'a>), Error> {
        let bad = || fail(TEXT_END, "a text span is outside the heap");
        let end = self.text_end.get(record).ok_or_else(bad)? as usize;
        let start = match record.checked_sub(1) {
            Some(prev) => self.text_end.get(prev).ok_or_else(bad)? as usize,
            None => 0,
        };
        let codes = self.heap.get(start..end).ok_or_else(bad)?;
        let table = self
            .table
            .get(record)
            .and_then(|t| self.tables.get(t as usize).copied().flatten())
            .ok_or_else(|| fail(TABLE, "a record names no symbol table"))?;
        Ok((codes, table))
    }

    /// Checks every value: places start at record 0, each has at least one record and they
    /// end at the last; text spans increase and stay in the heap, whose padding is zero;
    /// tables, types and tags are valid; every text decodes to UTF-8. O(records).
    ///
    /// # Errors
    /// [`Error::Payload`] naming the first column found wrong.
    pub fn check(&self) -> Result<(), Error> {
        let mut prev = None;
        for f in self.first.values(0..self.first.len()) {
            if prev.map_or(f != 0, |p| f <= p) {
                return Err(fail(
                    FIRST,
                    "places must start at record 0 and have records",
                ));
            }
            prev = Some(f);
        }
        if prev.map_or(0, |p| p as usize) != self.records() {
            return Err(fail(FIRST, "the last place must end at the last record"));
        }
        let mut end = 0;
        for e in self.text_end.values(0..self.records()) {
            if (e as usize) < end {
                return Err(fail(TEXT_END, "text spans must not go backwards"));
            }
            end = e as usize;
        }
        let padding = self
            .heap
            .get(end..)
            .ok_or_else(|| fail(TEXT_END, "a text span is outside the heap"))?;
        if padding.len() >= 4 || padding.iter().any(|&b| b != 0) {
            return Err(fail(HEAP, "the heap has bytes past the last text"));
        }
        // Every column streamed a block at a time: O(records), whatever the coding.
        let n = self.records();
        let stream = |slot: usize| {
            self.optional
                .get(slot)
                .copied()
                .flatten()
                .map(|c| c.values(0..n))
                .into_iter()
                .flatten()
                .chain(core::iter::repeat(0))
        };
        let (mut lang, mut script, mut ty) = (stream(0), stream(1), stream(2));
        let mut buf = [0_u8; TEXT_BUFFER];
        let mut start = 0;
        for (end, t) in self.text_end.values(0..n).zip(self.table.values(0..n)) {
            let codes = self.heap.get(start..end as usize).unwrap_or_default();
            start = end as usize;
            if codes.len() > MAX_CODES {
                return Err(fail(HEAP, "a name is longer than 255 codes"));
            }
            let table = self
                .tables
                .get(t as usize)
                .copied()
                .flatten()
                .ok_or_else(|| fail(TABLE, "a record names no symbol table"))?;
            decode(table, codes, &mut buf)?;
            for (column, v) in [(LANG, lang.next()), (SCRIPT, script.next())] {
                if v.is_some_and(|v| v != 0 && Tag::from_u32(v).is_none()) {
                    return Err(fail(column, "not a language or script code"));
                }
            }
            if ty
                .next()
                .is_some_and(|v| v != 0 && NameType::from_code(v).is_none())
            {
                return Err(fail(TYPE, "an unknown name type"));
            }
        }
        Ok(())
    }
}

/// Decodes `codes` with `table` into `out`, as UTF-8.
fn decode<'b>(table: Symbols<'_>, codes: &[u8], out: &'b mut [u8]) -> Result<&'b str, Error> {
    let n = table.decode(codes, out).map_err(|e| match e {
        FsstError::Space => fail(HEAP, "the output buffer is too small"),
        _ => fail(HEAP, "a text does not decode"),
    })?;
    core::str::from_utf8(out.get(..n).unwrap_or_default())
        .map_err(|_| fail(HEAP, "a text is not UTF-8"))
}

/// Parses the TABLES column: a count, then each table.
fn parse_tables(bytes: &[u8]) -> Result<[Option<Symbols<'_>>; MAX_TABLES], Error> {
    let bad = |problem| fail(TABLES, problem);
    let (&count, mut rest) = bytes
        .split_first_chunk::<4>()
        .ok_or(bad("the symbol tables are truncated"))?;
    let count = u32::from_le_bytes(count) as usize;
    if count == 0 || count > MAX_TABLES {
        return Err(bad("the symbol table count is not 1 to 16"));
    }
    let mut tables = [None; MAX_TABLES];
    for slot in tables.iter_mut().take(count) {
        let n = usize::from(*rest.first().ok_or(bad("the symbol tables are truncated"))?);
        let len = 8 + n.div_ceil(8) * 8 + 8 * n;
        let (table, tail) = rest
            .split_at_checked(len)
            .ok_or(bad("the symbol tables are truncated"))?;
        *slot = Some(Symbols::parse(table).map_err(|_| bad("a symbol table is invalid"))?);
        rest = tail;
    }
    if rest.len() >= 4 || rest.iter().any(|&b| b != 0) {
        return Err(bad("bytes past the last symbol table"));
    }
    Ok(tables)
}

impl Name<'_, '_> {
    /// The record's index.
    #[must_use]
    pub const fn record(&self) -> usize {
        self.record
    }

    /// Decodes the text into `out`; a buffer of [`TEXT_BUFFER`] bytes holds any name the check
    /// accepts.
    ///
    /// # Errors
    /// [`Error::Payload`] if the record's span or codes are invalid, the text is not UTF-8, or
    /// `out` is too small.
    pub fn text<'b>(&self, out: &'b mut [u8]) -> Result<&'b str, Error> {
        let (codes, table) = self.names.codes(self.record)?;
        decode(table, codes, out)
    }

    /// The language; `None` if undetermined.
    #[must_use]
    pub fn lang(&self) -> Option<Tag> {
        Tag::from_u32(self.names.optional(0, self.record))
    }

    /// The script; `None` if unknown.
    #[must_use]
    pub fn script(&self) -> Option<Tag> {
        Tag::from_u32(self.names.optional(1, self.record))
    }

    /// The type; `None` if unspecified.
    #[must_use]
    pub fn kind(&self) -> Option<NameType> {
        NameType::from_code(self.names.optional(2, self.record))
    }

    /// The validity: first and last day (days since 1970-01-01), each `None` if open.
    #[must_use]
    pub fn valid(&self) -> (Option<i32>, Option<i32>) {
        let day = |slot| match self.names.optional(slot, self.record) {
            0 => None,
            v => Some(unbias(v)),
        };
        (day(3), day(4))
    }

    /// The precedence rule code behind a canonical name; 0 for none.
    #[must_use]
    pub fn rule(&self) -> u32 {
        self.names.optional(5, self.record)
    }
}

/// A name to write (feature `write`).
#[cfg(feature = "write")]
#[derive(Clone, Copy, Debug, Default)]
pub struct NameRecord<'v> {
    /// The text, NFC.
    pub text: &'v str,
    /// The language.
    pub lang: Option<Tag>,
    /// The script; it also picks the record's symbol table group.
    pub script: Option<Tag>,
    /// The type.
    pub kind: Option<NameType>,
    /// The first day valid, days since 1970-01-01.
    pub valid_from: Option<i32>,
    /// The last day valid.
    pub valid_to: Option<i32>,
    /// The precedence rule code, 0 for none.
    pub rule: u32,
}

/// Encodes a NAMES payload (feature `write`): each place's records, canonical name first.
///
/// One symbol table is trained on every text, and one more for each script with at least
/// 16 KiB of text and less than half of all text (the largest first, up to 15); each record takes whichever of the shared
/// table and its script's table codes it smaller. Optional columns that are all 0 are left out.
///
/// # Errors
/// [`Error::Write`] if a place has no records, a name is longer than 255 codes, a day is
/// `i32::MIN`, or the section would pass 4 GiB.
#[cfg(feature = "write")]
pub fn encode(places: &[&[NameRecord<'_>]]) -> Result<alloc::vec::Vec<u8>, Error> {
    use alloc::vec::Vec;
    use teigeodata_core::bitpack::{bias, encode_auto, encode_direct};
    use teigeodata_core::table::{Codec, encode_with};

    let refuse = Error::Write;
    let records: Vec<&NameRecord<'_>> = places.iter().flat_map(|p| p.iter()).collect();
    if places.iter().any(|p| p.is_empty()) {
        return Err(refuse("every place needs its canonical name"));
    }
    let (tables, group_of) = tables_for(&records);

    let mut heap = Vec::new();
    let (mut text_end, mut table) = (
        Vec::with_capacity(records.len()),
        Vec::with_capacity(records.len()),
    );
    for r in &records {
        let own = r.script.and_then(|s| group_of.get(&s.get()).copied());
        let pick = own
            .filter(|&g| {
                tables.get(g).map(|t| t.encoded_len(r.text.as_bytes()))
                    < tables.first().map(|t| t.encoded_len(r.text.as_bytes()))
            })
            .unwrap_or(0);
        let start = heap.len();
        if let Some(t) = tables.get(pick) {
            t.encode(r.text.as_bytes(), &mut heap);
        }
        if heap.len() - start > MAX_CODES {
            return Err(refuse("a name is longer than 255 codes"));
        }
        text_end.push(u32::try_from(heap.len()).map_err(|_| refuse("the names pass 4 GiB"))?);
        #[allow(clippy::cast_possible_truncation, reason = "at most 16 tables")]
        table.push(pick as u32);
    }
    heap.resize(heap.len().div_ceil(4) * 4, 0);

    let mut first = Vec::with_capacity(places.len() + 1);
    let mut at = 0_u32;
    first.push(0);
    for p in places {
        at = at
            .checked_add(u32::try_from(p.len()).map_err(|_| refuse("too many names"))?)
            .ok_or(refuse("too many names"))?;
        first.push(at);
    }
    let day = |d: Option<i32>| match d {
        Some(i32::MIN) => Err(refuse("day i32::MIN is not a date")),
        Some(d) => Ok(bias(d)),
        None => Ok(0),
    };
    let mut optional: [Vec<u32>; 6] = Default::default();
    for r in &records {
        let row = [
            r.lang.map_or(0, Tag::get),
            r.script.map_or(0, Tag::get),
            r.kind.map_or(0, NameType::code),
            day(r.valid_from)?,
            day(r.valid_to)?,
            r.rule,
        ];
        for (col, v) in optional.iter_mut().zip(row) {
            col.push(v);
        }
    }

    let mut tables_bytes = u32::try_from(tables.len())
        .unwrap_or(0)
        .to_le_bytes()
        .to_vec();
    for t in &tables {
        tables_bytes.extend_from_slice(&t.to_bytes());
    }
    tables_bytes.resize(tables_bytes.len().div_ceil(4) * 4, 0);

    let mut bodies: Vec<(u16, Codec, Vec<u8>)> = alloc::vec![
        // Read for every name: O(1) codings only (B3: delta saved 6% and cost 50 ns a read).
        (FIRST, Codec::Bitpack, encode_direct(&first)),
        (TEXT_END, Codec::Bitpack, encode_direct(&text_end)),
        (TABLE, Codec::Bitpack, encode_auto(&table)),
    ];
    for (id, col) in OPTIONAL.into_iter().zip(&optional) {
        if col.iter().any(|&v| v != 0) {
            bodies.push((id, Codec::Bitpack, encode_auto(col)));
        }
    }
    bodies.push((TABLES, Codec::Bytes, tables_bytes));
    bodies.push((HEAP, Codec::Bytes, heap));
    let refs: Vec<(u16, Codec, &[u8])> = bodies
        .iter()
        .map(|(i, c, b)| (*i, *c, b.as_slice()))
        .collect();
    encode_with(&refs).map_err(|_| refuse("the names pass 4 GiB"))
}

/// The symbol tables of `records`: a shared one first, then one per script with at least
/// 16 KiB of text and less than half of all text, the largest first (ties by tag), up to 15;
/// and each such script's table.
#[cfg(feature = "write")]
fn tables_for(
    records: &[&NameRecord<'_>],
) -> (
    alloc::vec::Vec<teigeodata_core::fsst::build::Encoder>,
    alloc::collections::BTreeMap<u32, usize>,
) {
    use alloc::collections::BTreeMap;
    use alloc::vec::Vec;
    use teigeodata_core::fsst::build::{Encoder, train};

    const MIN_GROUP: usize = 16 << 10;
    let texts: Vec<&[u8]> = records.iter().map(|r| r.text.as_bytes()).collect();
    let mut tables: Vec<Encoder> = alloc::vec![train(&texts)];
    let mut by_script: BTreeMap<u32, (usize, Vec<&[u8]>)> = BTreeMap::new();
    for r in records {
        if let Some(s) = r.script {
            let e = by_script.entry(s.get()).or_default();
            e.0 += r.text.len();
            e.1.push(r.text.as_bytes());
        }
    }
    // A group with half the text or more is what the shared table was trained on already.
    let total: usize = texts.iter().map(|t| t.len()).sum();
    let mut groups: Vec<(u32, usize, Vec<&[u8]>)> = by_script
        .into_iter()
        .filter(|g| g.1.0 >= MIN_GROUP && g.1.0 * 2 < total)
        .map(|(s, (n, t))| (s, n, t))
        .collect();
    groups.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    groups.truncate(MAX_TABLES - 1);
    let mut group_of: BTreeMap<u32, usize> = BTreeMap::new();
    for (i, (s, _, t)) in groups.iter().enumerate() {
        tables.push(train(t));
        group_of.insert(*s, i + 1);
    }

    (tables, group_of)
}
