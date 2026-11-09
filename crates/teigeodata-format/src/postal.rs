//! `POSTAL` (kind 11): postal codes to places, for exact lookup, prefix search and place →
//! codes (D-090, D-105; [research note](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/postal.md)).
//!
//! Keys are a **scheme name** (`np`, `np-1991`, `gb`: ASCII `[a-z0-9-]`), a `0x00` byte, then
//! the **canonical code**. One [automaton](crate::search::fst) holds every key, so a key's
//! ordinal indexes its records and a prefix of a code is one ordinal range. Every record links
//! to a PLACES row: the admin unit of an area code, a settlement for a post office.
//!
//! A [column table](teigeodata_core::table) with these columns:
//!
//! | ID | Column | Values | |
//! |---:|---|---|---|
//! | 1 | [`LENGTH`] | one value: the automaton's length in bytes | required |
//! | 2 | [`AUTOMATON`] | bytes: the automaton, zero-padded to a multiple of 4 | required |
//! | 3 | [`STARTS`] | per key and one more: its first record, strictly increasing from 0 | required |
//! | 4 | [`PLACE`] | per record: its PLACES row; within a key, strictly increasing | required |
//! | 5 | [`KIND`] | per record: a [`CodeKind`] | optional (all areas) |
//! | 6 | [`VALID_FROM`] | per record: days since 1970-01-01 plus 1; 0 = the scheme's | optional |
//! | 7 | [`VALID_TO`] | per record: the same; 0 = the scheme's | optional |
//! | 8 | [`REDIRECT`] | per record: the ordinal of the key that replaces it, plus 1; 0 = none | optional |
//! | 9 | [`BY_PLACE`] | per record, sorted: the rows of the records, for place → codes | optional, with 10 |
//! | 10 | [`BY_PLACE_REC`] | per [`BY_PLACE`] entry: its record, ascending within a row | optional, with 9 |
//! | 11 | [`SCHEMES`] | bytes: the schemes and their [normalisation rules](Scheme) | required |
//!
//! Its item count is the number of records. [`Postal::open`] checks the structure in
//! O(columns + schemes); [`Postal::check`] every key, record and link, and that every code is
//! the canonical form of itself under its scheme. Queries never panic and never allocate.
//!
//! **Normalisation is data.** A scheme's rule comes from `data/countries/<cc>/postal.yaml` and
//! is compiled into SCHEMES, so the reader holds no country's rules. Input is mapped to ASCII
//! (full-width forms, the decimal digits of every script, Unicode spaces and dashes), trimmed,
//! upper-cased if the scheme says so, stripped of the scheme's ignored characters and of an
//! optional prefix, then matched against the scheme's **templates**: `9` a digit, `A` a letter,
//! `?` either, `\x` the literal `x`, any other character a literal. A literal that the scheme
//! ignores is inserted (`999-9999` turns `1000001` into `100-0001`); any other literal must be
//! typed. A code that matches no template is not found; it is never an error.

use core::ops::{ControlFlow, Range};

use teigeodata_core::bitpack::{BLOCK_LEN, Packed};
use teigeodata_core::table::Table;

use crate::search::fst::{self, Fst};
use crate::{Error, Kind, Section, columns};

/// Column: the automaton's length.
pub const LENGTH: u16 = 1;
/// Column: the automaton.
pub const AUTOMATON: u16 = 2;
/// Column: each key's first record.
pub const STARTS: u16 = 3;
/// Column: each record's place.
pub const PLACE: u16 = 4;
/// Column: each record's kind.
pub const KIND: u16 = 5;
/// Column: each record's first valid day.
pub const VALID_FROM: u16 = 6;
/// Column: each record's last valid day.
pub const VALID_TO: u16 = 7;
/// Column: each record's replacement key.
pub const REDIRECT: u16 = 8;
/// Column: the records' rows, sorted.
pub const BY_PLACE: u16 = 9;
/// Column: the record of each sorted row.
pub const BY_PLACE_REC: u16 = 10;
/// Column: the schemes.
pub const SCHEMES: u16 = 11;

/// The longest cleaned input or canonical code, in bytes.
pub const MAX_CODE: usize = 64;
/// The most class positions (`9`, `A`, `?`) in a template.
pub const MAX_POSITIONS: usize = 32;
/// The longest scheme name.
pub const MAX_NAME: usize = 32;

/// Scheme flag: input letters are upper-cased.
pub const UPPER: u8 = 1;
/// Scheme flag: codes are areas aligned with admin units, so point → code goes through the
/// admin hierarchy (Nepal: 5 digits the local level, 7 its ward).
pub const ADMIN_ALIGNED: u8 = 2;
const FLAGS: u8 = UPPER | ADMIN_ALIGNED;

/// What a record's code stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CodeKind {
    /// The area of its place (an admin unit, a postal area).
    Area = 0,
    /// An area inside its place, not a place of its own (a ward of a local level).
    Part = 1,
    /// A post office at its place.
    PostOffice = 2,
    /// A point at its place (a delivery point, a large user).
    Point = 3,
}

impl CodeKind {
    /// The kind of `value`, if one.
    #[must_use]
    pub const fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Area),
            1 => Some(Self::Part),
            2 => Some(Self::PostOffice),
            3 => Some(Self::Point),
            _ => None,
        }
    }
}

/// One record: a code's link to a place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    /// The PLACES row.
    pub place: u32,
    /// What the code stands for there.
    pub kind: CodeKind,
    /// The first valid day (days since 1970-01-01), its own or its scheme's.
    pub valid_from: Option<u32>,
    /// The last valid day, its own or its scheme's.
    pub valid_to: Option<u32>,
    /// The ordinal of the key that replaces it.
    pub redirect: Option<u32>,
}

fn fail(column: u16, problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::POSTAL.0,
        column,
        problem,
    }
}

/// A list of short byte strings: a count, then each as a length byte and its bytes.
#[derive(Clone, Copy, Debug)]
struct List<'a> {
    bytes: &'a [u8],
}

impl<'a> IntoIterator for List<'a> {
    type Item = &'a [u8];
    type IntoIter = ListIter<'a>;
    fn into_iter(self) -> ListIter<'a> {
        ListIter { bytes: self.bytes }
    }
}

/// The items of a [`List`].
#[derive(Clone, Debug)]
struct ListIter<'a> {
    bytes: &'a [u8],
}

impl<'a> Iterator for ListIter<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<&'a [u8]> {
        let (item, rest) = short(self.bytes)?;
        self.bytes = rest;
        Some(item)
    }
}

/// One scheme: its name, validity and normalisation rule. In SCHEMES, little-endian:
///
/// ```text
/// name       u8 length, ASCII [a-z0-9-], 1 to 32 bytes
/// valid_from u32: days since 1970-01-01 plus 1; 0 = open
/// valid_to   u32: the same
/// flags      u8: UPPER, ADMIN_ALIGNED
/// ignore     u8 length, printable ASCII: characters dropped from the input
/// prefixes   u8 count, each u8 length + printable ASCII: optional prefixes, cleaned
/// display    u8 length, printable ASCII: shown before a code (`LV-`)
/// templates  u8 count (1 or more), each u8 length + printable ASCII
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Scheme<'a> {
    name: &'a str,
    valid_from: u32,
    valid_to: u32,
    flags: u8,
    ignore: &'a [u8],
    prefixes: List<'a>,
    display: &'a str,
    templates: List<'a>,
}

impl<'a> Scheme<'a> {
    /// The scheme's name, the first part of its keys.
    #[must_use]
    pub const fn name(&self) -> &'a str {
        self.name
    }

    /// The first valid day of its codes (days since 1970-01-01).
    #[must_use]
    pub fn valid_from(&self) -> Option<u32> {
        self.valid_from.checked_sub(1)
    }

    /// The last valid day of its codes.
    #[must_use]
    pub fn valid_to(&self) -> Option<u32> {
        self.valid_to.checked_sub(1)
    }

    /// Whether its codes are areas of admin units, found from a point through the hierarchy.
    #[must_use]
    pub const fn admin_aligned(&self) -> bool {
        self.flags & ADMIN_ALIGNED != 0
    }

    /// What is shown before a code.
    #[must_use]
    pub const fn display_prefix(&self) -> &'a str {
        self.display
    }

    /// Parses one scheme at the start of `bytes`, and what follows it.
    fn parse(bytes: &'a [u8]) -> Option<(Self, &'a [u8])> {
        let (name, rest) = short(bytes)?;
        let (from, rest) = rest.split_first_chunk::<4>()?;
        let (to, rest) = rest.split_first_chunk::<4>()?;
        let (&flags, rest) = rest.split_first()?;
        let (ignore, rest) = short(rest)?;
        let (prefixes, rest) = list(rest)?;
        let (display, rest) = short(rest)?;
        let (templates, rest) = list(rest)?;
        let s = Self {
            name: core::str::from_utf8(name).ok()?,
            valid_from: u32::from_le_bytes(*from),
            valid_to: u32::from_le_bytes(*to),
            flags,
            ignore,
            prefixes: List { bytes: prefixes },
            display: core::str::from_utf8(display).ok()?,
            templates: List { bytes: templates },
        };
        Some((s, rest))
    }

    /// Whether the scheme follows the rules of the layout.
    fn valid(&self) -> bool {
        let name_ok = (1..=MAX_NAME).contains(&self.name.len())
            && self
                .name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        let printable = |s: &[u8]| s.iter().all(|&b| (0x20..0x7f).contains(&b));
        let dates = self.valid_from == 0 || self.valid_to == 0 || self.valid_from <= self.valid_to;
        let prefixes = self
            .prefixes
            .into_iter()
            .all(|p| !p.is_empty() && printable(p));
        let mut templates = self.templates.into_iter();
        let first = templates.next().is_some_and(template_ok);
        name_ok
            && dates
            && self.flags & !FLAGS == 0
            && printable(self.ignore)
            && printable(self.display.as_bytes())
            && prefixes
            && first
            && templates.all(template_ok)
    }

    /// `input` cleaned: mapped to ASCII, trimmed, upper-cased if the scheme says so, and
    /// stripped of ignored characters. Its length, or `None` if a character has no ASCII form
    /// or it is too long.
    fn clean(&self, input: &str, out: &mut [u8; MAX_CODE]) -> Option<usize> {
        let mut n = 0;
        for c in input.trim().chars() {
            let mut b = ascii(c)?;
            if self.flags & UPPER != 0 {
                b = b.to_ascii_uppercase();
            }
            if self.ignore.contains(&b) {
                continue;
            }
            *out.get_mut(n)? = b;
            n += 1;
        }
        Some(n)
    }

    /// Calls `f` with each canonical form of `input` (once per template that matches, with
    /// and without an optional prefix), or with each canonical prefix when `partial`.
    fn forms(&self, input: &str, partial: bool, mut f: impl FnMut(&[u8]) -> ControlFlow<()>) {
        let mut clean = [0_u8; MAX_CODE];
        let Some(n) = self.clean(input, &mut clean) else {
            return;
        };
        let Some(clean) = clean.get(..n) else { return };
        let mut out = [0_u8; MAX_CODE];
        let mut try_one = |c: &[u8]| -> ControlFlow<()> {
            for t in self.templates {
                if let Some(len) = apply(c, t, self.ignore, &mut out, partial)
                    && let Some(form) = out.get(..len)
                {
                    f(form)?;
                }
            }
            ControlFlow::Continue(())
        };
        if try_one(clean).is_break() {
            return;
        }
        for p in self.prefixes {
            if let Some(rest) = clean.strip_prefix(p)
                && try_one(rest).is_break()
            {
                return;
            }
        }
    }

    /// Whether `code` is canonical: it is one of its own forms.
    fn canonical(&self, code: &[u8]) -> bool {
        let Ok(text) = core::str::from_utf8(code) else {
            return false;
        };
        let mut found = false;
        self.forms(text, false, |form| {
            found = form == code;
            if found {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });
        found
    }
}

/// A length byte and that many bytes.
fn short(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let (&n, rest) = bytes.split_first()?;
    rest.split_at_checked(usize::from(n))
}

/// A list: a count, then each item as [`short`]. The items' bytes, and what follows.
fn list(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let (&count, items) = bytes.split_first()?;
    let mut tail = items;
    for _ in 0..count {
        tail = short(tail)?.1;
    }
    Some((items.get(..items.len() - tail.len())?, tail))
}

/// Whether `t` is a template: printable ASCII, no dangling escape, 1 to [`MAX_POSITIONS`]
/// class positions, and a canonical form that fits [`MAX_CODE`].
fn template_ok(t: &[u8]) -> bool {
    let (mut i, mut positions, mut len) = (0, 0, 0);
    while let Some(&c) = t.get(i) {
        if !(0x20..0x7f).contains(&c) {
            return false;
        }
        if c == b'\\' {
            match t.get(i + 1) {
                Some(&l) if (0x20..0x7f).contains(&l) => i += 1,
                _ => return false,
            }
        } else if matches!(c, b'9' | b'A' | b'?') {
            positions += 1;
        }
        len += 1;
        i += 1;
    }
    (1..=MAX_POSITIONS).contains(&positions) && len <= MAX_CODE
}

/// `clean` laid onto template `t` (see the module doc): the canonical form's length in `out`.
/// With `partial`, a `clean` that ends inside the template gives the form so far.
fn apply(
    clean: &[u8],
    template: &[u8],
    ignore: &[u8],
    out: &mut [u8; MAX_CODE],
    partial: bool,
) -> Option<usize> {
    let (mut read, mut len, mut at) = (0, 0, 0);
    while let Some(&c) = template.get(at) {
        if partial && read == clean.len() {
            return Some(len);
        }
        let (literal, c) = if c == b'\\' {
            at += 1;
            (true, *template.get(at)?)
        } else {
            (false, c)
        };
        at += 1;
        let emit = if !literal && matches!(c, b'9' | b'A' | b'?') {
            let b = *clean.get(read)?;
            let ok = match c {
                b'9' => b.is_ascii_digit(),
                b'A' => b.is_ascii_alphabetic(),
                _ => b.is_ascii_alphanumeric(),
            };
            if !ok {
                return None;
            }
            read += 1;
            b
        } else if ignore.contains(&c) {
            c
        } else {
            if *clean.get(read)? != c {
                return None;
            }
            read += 1;
            c
        };
        *out.get_mut(len)? = emit;
        len += 1;
    }
    (read == clean.len()).then_some(len)
}

/// The first code point of each block of ten decimal digits that is not ASCII (Unicode
/// general category Nd, the blocks of the BMP), so a code typed in any script's digits is found.
const DIGIT_ZEROS: [u32; 36] = [
    0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66, 0x0CE6, 0x0D66,
    0x0DE6, 0x0E50, 0x0ED0, 0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946, 0x19D0, 0x1A80, 0x1A90,
    0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900, 0xA9D0, 0xA9F0, 0xAA50, 0xABF0, 0xFF10,
];

/// The ASCII form of `c`: itself if printable ASCII; a full-width form's ASCII; a decimal
/// digit of any script; a Unicode space or dash. `None` for anything else.
fn ascii(c: char) -> Option<u8> {
    let u = u32::from(c);
    let digit = |d: u32| u8::try_from(d).ok().map(|d| b'0' + d);
    match u {
        0x20..0x7f => u8::try_from(u).ok(),
        0xFF01..=0xFF5E => u8::try_from(u - 0xFEE0).ok(),
        0x2010..=0x2015 | 0x2212 | 0xFE58 | 0xFE63 => Some(b'-'),
        // The mathematical digits: five styles of ten.
        0x1_D7CE..0x1_D800 => digit((u - 0x1_D7CE) % 10),
        _ if c.is_whitespace() => Some(b' '),
        _ => DIGIT_ZEROS
            .iter()
            .find(|&&z| (z..z + 10).contains(&u))
            .and_then(|&z| digit(u - z)),
    }
}

/// The schemes of a pack, in name order.
#[derive(Clone, Copy, Debug)]
pub struct Schemes<'a> {
    bytes: &'a [u8],
    left: u16,
}

impl<'a> IntoIterator for Schemes<'a> {
    type Item = Scheme<'a>;
    type IntoIter = SchemeIter<'a>;
    fn into_iter(self) -> SchemeIter<'a> {
        SchemeIter {
            bytes: self.bytes,
            left: self.left,
        }
    }
}

/// The schemes of a pack, one by one.
#[derive(Clone, Debug)]
pub struct SchemeIter<'a> {
    bytes: &'a [u8],
    left: u16,
}

impl<'a> Iterator for SchemeIter<'a> {
    type Item = Scheme<'a>;
    fn next(&mut self) -> Option<Scheme<'a>> {
        self.left = self.left.checked_sub(1)?;
        let (s, rest) = Scheme::parse(self.bytes)?;
        self.bytes = rest;
        Some(s)
    }
}

/// A view of a POSTAL section.
#[derive(Clone, Copy, Debug)]
pub struct Postal<'a> {
    fst: Fst<'a>,
    schemes: Schemes<'a>,
    starts: Packed<'a>,
    place: Packed<'a>,
    kind: Option<Packed<'a>>,
    valid_from: Option<Packed<'a>>,
    valid_to: Option<Packed<'a>>,
    redirect: Option<Packed<'a>>,
    by_place: Option<(Packed<'a>, Packed<'a>)>,
    records: usize,
    places: usize,
}

impl<'a> Postal<'a> {
    /// Opens the section of a pack with `places` PLACES rows: the frame, every column's
    /// structure and length, the automaton's root and every scheme, in O(columns + schemes).
    ///
    /// # Errors
    /// [`Error::Payload`] if the section is not a POSTAL column table, or a column or scheme is
    /// missing, malformed or of the wrong length.
    pub fn open(section: &Section<'a>, places: usize) -> Result<Self, Error> {
        let k = Kind::POSTAL;
        let t: Table<'a> = columns::open(section, k)?;
        let records = usize::try_from(section.item_count())
            .map_err(|_| fail(0, "the record count does not fit this platform"))?;
        if t.ids().any(|id| id == 0 || id > SCHEMES) {
            return Err(fail(0, "an unknown column"));
        }
        let len = columns::required(&t, k, LENGTH, Some(1))?
            .get(0)
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(0);
        let padded = t.bytes(AUTOMATON).map_err(|e| columns::map(k, e))?;
        if padded.len() != len.next_multiple_of(4) {
            return Err(fail(
                AUTOMATON,
                "the automaton length differs from its column",
            ));
        }
        let (bytes, pad) = padded.split_at(len);
        if pad.iter().any(|&b| b != 0) || len == 0 {
            return Err(fail(
                AUTOMATON,
                "the automaton is empty or its padding is not zero",
            ));
        }
        let starts = columns::required(&t, k, STARTS, None)?;
        let keys = starts
            .len()
            .checked_sub(1)
            .ok_or(fail(STARTS, "the record starts are empty"))?;
        let keys32 = u32::try_from(keys).map_err(|_| fail(STARTS, "too many keys"))?;
        if starts.get(keys).and_then(|v| usize::try_from(v).ok()) != Some(records) {
            return Err(fail(STARTS, "the last start is not the record count"));
        }
        let place = columns::required(&t, k, PLACE, Some(records))?;
        let opt = |id| columns::optional(&t, k, id, records);
        let (kind, valid_from, valid_to, redirect) =
            (opt(KIND)?, opt(VALID_FROM)?, opt(VALID_TO)?, opt(REDIRECT)?);
        let by_place = match (opt(BY_PLACE)?, opt(BY_PLACE_REC)?) {
            (Some(a), Some(b)) => Some((a, b)),
            (None, None) => None,
            _ => return Err(fail(BY_PLACE, "the place index has one of its two columns")),
        };
        let fst =
            Fst::new(bytes, keys32).ok_or(fail(AUTOMATON, "the automaton's root is malformed"))?;
        let schemes = open_schemes(t.bytes(SCHEMES).map_err(|e| columns::map(k, e))?)?;
        Ok(Self {
            fst,
            schemes,
            starts,
            place,
            kind,
            valid_from,
            valid_to,
            redirect,
            by_place,
            records,
            places,
        })
    }

    /// The number of keys (distinct codes, per scheme).
    #[must_use]
    pub const fn keys(&self) -> u32 {
        self.fst.keys()
    }

    /// The number of records.
    #[must_use]
    pub const fn records(&self) -> usize {
        self.records
    }

    /// The schemes, in name order.
    #[must_use]
    pub const fn schemes(&self) -> Schemes<'a> {
        self.schemes
    }

    /// The scheme named `name`.
    #[must_use]
    pub fn scheme(&self, name: &str) -> Option<Scheme<'a>> {
        self.schemes.into_iter().find(|s| s.name == name)
    }

    /// Whether the pack can answer place → codes.
    #[must_use]
    pub const fn has_place_index(&self) -> bool {
        self.by_place.is_some()
    }

    /// Calls `visit` with the ordinal of each key that `input` names, in scheme `scheme` or,
    /// with `None`, in every scheme whose rule accepts it, until it breaks. A code typed with
    /// or without separators, in any case the scheme folds, in full-width forms or another
    /// script's digits, is found; a form no template accepts is not.
    pub fn lookup(
        &self,
        input: &str,
        scheme: Option<&str>,
        mut visit: impl FnMut(u32) -> ControlFlow<()>,
    ) {
        let mut key = [0_u8; fst::MAX_KEY];
        for s in self
            .schemes
            .into_iter()
            .filter(|s| scheme.is_none_or(|n| n == s.name))
        {
            // A code matching two templates gives one key once.
            let mut last = None;
            let mut stop = false;
            s.forms(input, false, |form| {
                if let Some(o) = keyed(&mut key, s.name, form).and_then(|k| self.fst.get(k))
                    && last != Some(o)
                {
                    last = Some(o);
                    if visit(o).is_break() {
                        stop = true;
                        return ControlFlow::Break(());
                    }
                }
                ControlFlow::Continue(())
            });
            if stop {
                return;
            }
        }
    }

    /// Calls `visit` with the ordinals of the keys that start with `input`, in code order per
    /// scheme, until it breaks: autocomplete. An empty input names nothing.
    pub fn prefix(
        &self,
        input: &str,
        scheme: Option<&str>,
        mut visit: impl FnMut(u32) -> ControlFlow<()>,
    ) {
        if input.trim().is_empty() {
            return;
        }
        let mut key = [0_u8; fst::MAX_KEY];
        for s in self
            .schemes
            .into_iter()
            .filter(|s| scheme.is_none_or(|n| n == s.name))
        {
            // The ranges of the scheme's partial forms, merged in order.
            let mut ranges = [(0_u32, 0_u32); 16];
            let mut n = 0;
            s.forms(input, true, |form| {
                if let Some(k) = keyed(&mut key, s.name, form) {
                    let r = self.fst.range(k);
                    if r.0 < r.1
                        && let Some(slot) = ranges.get_mut(n)
                    {
                        *slot = r;
                        n += 1;
                    }
                }
                ControlFlow::Continue(())
            });
            let ranges = ranges.get_mut(..n).unwrap_or(&mut []);
            ranges.sort_unstable();
            let mut next = 0_u32;
            for &(lo, hi) in &*ranges {
                for o in lo.max(next)..hi {
                    if visit(o).is_break() {
                        return;
                    }
                }
                next = next.max(hi);
            }
        }
    }

    /// Key `ordinal`'s scheme and canonical code, written into `buf`.
    #[must_use]
    pub fn code<'b>(
        &self,
        ordinal: u32,
        buf: &'b mut [u8; fst::MAX_KEY],
    ) -> Option<(&'b str, &'b str)> {
        let len = self.fst.key(ordinal, buf)?;
        let key = buf.get(..len)?;
        let cut = key.iter().position(|&b| b == 0)?;
        let (name, code) = (key.get(..cut)?, key.get(cut + 1..)?);
        Some((
            core::str::from_utf8(name).ok()?,
            core::str::from_utf8(code).ok()?,
        ))
    }

    /// The records of key `ordinal`; empty past the end.
    #[must_use]
    pub fn records_of(&self, ordinal: u32) -> Range<usize> {
        let o = ordinal as usize;
        match (self.starts.get(o), self.starts.get(o + 1)) {
            (Some(a), Some(b)) if a <= b => a as usize..(b as usize).min(self.records),
            _ => 0..0,
        }
    }

    /// The key of record `record`: a binary search of the starts.
    #[must_use]
    pub fn key_of(&self, record: usize) -> Option<u32> {
        if record >= self.records {
            return None;
        }
        let r = u32::try_from(record).ok()?;
        let (mut lo, mut hi) = (0_usize, self.starts.len().checked_sub(1)?);
        // The last key whose start is not above `record`.
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if self.starts.get(mid)? <= r {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        u32::try_from(lo).ok()
    }

    /// Record `record`, with its scheme's validity where it has none of its own.
    #[must_use]
    pub fn record(&self, record: usize) -> Option<Record> {
        let place = self.place.get(record)?;
        if place as usize >= self.places {
            return None;
        }
        let read = |c: &Option<Packed<'_>>| c.as_ref().map_or(Some(0), |c| c.get(record));
        let kind = CodeKind::from_u32(read(&self.kind)?)?;
        let (mut from, mut to) = (read(&self.valid_from)?, read(&self.valid_to)?);
        if from == 0 || to == 0 {
            let mut buf = [0_u8; fst::MAX_KEY];
            let o = self.key_of(record)?;
            if let Some((name, _)) = self.code(o, &mut buf)
                && let Some(s) = self.scheme(name)
            {
                if from == 0 {
                    from = s.valid_from;
                }
                if to == 0 {
                    to = s.valid_to;
                }
            }
        }
        Some(Record {
            place,
            kind,
            valid_from: from.checked_sub(1),
            valid_to: to.checked_sub(1),
            redirect: read(&self.redirect)?
                .checked_sub(1)
                .filter(|&t| t < self.keys()),
        })
    }

    /// Calls `visit` with the records of place row `place`, in record order (so in code order),
    /// until it breaks. Nothing without the place index.
    pub fn records_at(&self, place: u32, mut visit: impl FnMut(usize) -> ControlFlow<()>) {
        let Some((rows, recs)) = &self.by_place else {
            return;
        };
        // The first block that can hold `place`, by block firsts, then a scan from there.
        let blocks = rows.len().div_ceil(BLOCK_LEN);
        let (mut lo, mut hi) = (0_usize, blocks);
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            match rows.block_first(mid) {
                Some(f) if f < place => lo = mid,
                _ => hi = mid,
            }
        }
        let mut buf = [0_u32; BLOCK_LEN];
        let mut b = lo;
        while b < blocks {
            let got = rows.decode_block(b, &mut buf);
            for (j, &r) in buf.iter().take(got).enumerate() {
                if r > place {
                    return;
                }
                if r == place
                    && let Some(rec) = recs.get(b * BLOCK_LEN + j)
                    && visit(rec as usize).is_break()
                {
                    return;
                }
            }
            b += 1;
        }
    }

    /// Checks every key, record and link: O(automaton + records + keys × templates).
    ///
    /// # Errors
    /// [`Error::Payload`] if the automaton is not canonical, a key names no scheme or is not
    /// its scheme's canonical form of itself, a key has no records, the records of a key are
    /// not in place order or name a row past the end, a kind or date is out of range, a
    /// redirect names no key or its own, or the place index differs from the records.
    pub fn check(&self) -> Result<(), Error> {
        let mut bad = None;
        let schemes = self.schemes;
        self.fst
            .check(|_, key| {
                if bad.is_some() {
                    return;
                }
                let ok = key.iter().position(|&b| b == 0).is_some_and(|cut| {
                    let (name, code) = (key.get(..cut), key.get(cut + 1..));
                    match (name, code) {
                        (Some(n), Some(c)) if !c.is_empty() => schemes
                            .into_iter()
                            .find(|s| s.name.as_bytes() == n)
                            .is_some_and(|s| s.canonical(c)),
                        _ => false,
                    }
                });
                if !ok {
                    bad = Some("a key names no scheme or is not a canonical code");
                }
            })
            .map_err(|p| fail(AUTOMATON, p))?;
        if let Some(p) = bad {
            return Err(fail(AUTOMATON, p));
        }
        let mut prev = 0_u32;
        for o in 0..self.starts.len() {
            let s = self
                .starts
                .get(o)
                .ok_or(fail(STARTS, "a record start is missing"))?;
            if (o == 0 && s != 0) || (o > 0 && s <= prev) {
                return Err(fail(STARTS, "a key has no records or the starts decrease"));
            }
            prev = s;
        }
        for o in 0..self.keys() {
            let mut last = None;
            for r in self.records_of(o) {
                let p = self
                    .place
                    .get(r)
                    .ok_or(fail(PLACE, "a record's place is missing"))?;
                if p as usize >= self.places {
                    return Err(fail(PLACE, "a record names a row past the end"));
                }
                if last.is_some_and(|l| p <= l) {
                    return Err(fail(PLACE, "a key's records are not in place order"));
                }
                last = Some(p);
            }
        }
        for r in 0..self.records {
            let get = |c: &Option<Packed<'_>>| c.as_ref().map_or(Some(0), |c| c.get(r));
            if get(&self.kind).and_then(CodeKind::from_u32).is_none() {
                return Err(fail(KIND, "a record's kind is unknown"));
            }
            let (from, to) = (get(&self.valid_from), get(&self.valid_to));
            if let (Some(f), Some(t)) = (from, to)
                && f != 0
                && t != 0
                && f > t
            {
                return Err(fail(VALID_TO, "a record ends before it starts"));
            }
            let target = get(&self.redirect).ok_or(fail(REDIRECT, "a redirect is missing"))?;
            if target != 0 {
                let own = self.key_of(r);
                if target > self.keys() || own == Some(target - 1) {
                    return Err(fail(REDIRECT, "a redirect names no key or its own"));
                }
            }
        }
        if let Some((rows, recs)) = &self.by_place {
            let mut last: Option<(u32, u32)> = None;
            for e in 0..rows.len() {
                let (row, rec) = (rows.get(e), recs.get(e));
                let (Some(row), Some(rec)) = (row, rec) else {
                    return Err(fail(BY_PLACE, "a place index entry is missing"));
                };
                if self.place.get(rec as usize) != Some(row)
                    || last.is_some_and(|l| (row, rec) <= l)
                {
                    return Err(fail(BY_PLACE, "the place index differs from the records"));
                }
                last = Some((row, rec));
            }
        }
        Ok(())
    }
}

/// The SCHEMES column: its count, every scheme valid and in strictly increasing name order,
/// and only zero padding (fewer than 4 bytes) after the last.
fn open_schemes(bytes: &[u8]) -> Result<Schemes<'_>, Error> {
    let bad = || fail(SCHEMES, "a scheme is malformed or out of order");
    let (count, mut rest) = bytes.split_first_chunk::<2>().ok_or_else(bad)?;
    let count = u16::from_le_bytes(*count);
    if count == 0 {
        return Err(bad());
    }
    let all = rest;
    let mut last: Option<&str> = None;
    for _ in 0..count {
        let (s, r) = Scheme::parse(rest).ok_or_else(bad)?;
        if !s.valid() || last.is_some_and(|l| s.name <= l) {
            return Err(bad());
        }
        last = Some(s.name);
        rest = r;
    }
    if rest.len() >= 4 || rest.iter().any(|&b| b != 0) {
        return Err(fail(SCHEMES, "the schemes are followed by bytes"));
    }
    Ok(Schemes {
        bytes: all,
        left: count,
    })
}

/// `name`, 0x00, then `code` in `buf`, if it fits a key.
fn keyed<'b>(buf: &'b mut [u8; fst::MAX_KEY], name: &str, code: &[u8]) -> Option<&'b [u8]> {
    let n = name.len();
    let out = buf.get_mut(..n + 1 + code.len())?;
    out.get_mut(..n)?.copy_from_slice(name.as_bytes());
    *out.get_mut(n)? = 0;
    out.get_mut(n + 1..)?.copy_from_slice(code);
    Some(out)
}

#[cfg(feature = "write")]
pub use write::{Code, Rule, encode};

#[cfg(feature = "write")]
mod write {
    use alloc::collections::BTreeMap;
    use alloc::string::String;
    use alloc::vec::Vec;

    use teigeodata_core::bitpack::{encode_auto, encode_direct};
    use teigeodata_core::table::{self, Codec};

    use super::{
        AUTOMATON, BY_PLACE, BY_PLACE_REC, CodeKind, KIND, LENGTH, PLACE, REDIRECT, SCHEMES,
        STARTS, Scheme, Schemes, VALID_FROM, VALID_TO, open_schemes,
    };
    use crate::Error;
    use crate::search::fst::Builder;

    /// One scheme's rule, as `data/countries/<cc>/postal.yaml` gives it.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct Rule {
        /// The scheme's name: ASCII `[a-z0-9-]`, unique worldwide.
        pub name: String,
        /// The first valid day (days since 1970-01-01).
        pub valid_from: Option<u32>,
        /// The last valid day.
        pub valid_to: Option<u32>,
        /// Upper-case input letters.
        pub upper: bool,
        /// Codes are areas of admin units.
        pub admin_aligned: bool,
        /// Characters dropped from input (spaces, hyphens).
        pub ignore: String,
        /// Optional prefixes (`LV-`); cleaned by the rule before they are stored.
        pub prefixes: Vec<String>,
        /// Shown before a code.
        pub display: String,
        /// The templates (see the module doc), most common first.
        pub templates: Vec<String>,
    }

    /// One code of the input, and its place.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Code {
        /// Its scheme's name.
        pub scheme: String,
        /// The code as the source writes it; stored in its canonical form.
        pub code: String,
        /// Its PLACES row.
        pub place: u32,
        /// What it stands for there.
        pub kind: CodeKind,
        /// Its own first valid day, if it differs from its scheme's.
        pub valid_from: Option<u32>,
        /// Its own last valid day.
        pub valid_to: Option<u32>,
        /// The code that replaces it, in the same or another scheme: (scheme, code).
        pub redirect: Option<(String, String)>,
    }

    /// A record before its key's ordinal is known: kind, days, the redirect's key.
    type Rec = (CodeKind, u32, u32, Option<Vec<u8>>);

    fn fail(problem: &'static str) -> Error {
        Error::Write(problem)
    }

    fn day(d: Option<u32>) -> Result<u32, Error> {
        d.map_or(Ok(0), |d| {
            d.checked_add(1).ok_or(fail("a day is out of range"))
        })
    }

    fn short(out: &mut Vec<u8>, s: &[u8]) -> Result<(), Error> {
        out.push(u8::try_from(s.len()).map_err(|_| fail("a scheme field is too long"))?);
        out.extend_from_slice(s);
        Ok(())
    }

    fn list(out: &mut Vec<u8>, items: &[Vec<u8>]) -> Result<(), Error> {
        out.push(u8::try_from(items.len()).map_err(|_| fail("a scheme has too many items"))?);
        for i in items {
            short(out, i)?;
        }
        Ok(())
    }

    /// The SCHEMES column of `rules`, sorted by name. Prefixes are cleaned with their own
    /// rule, so they compare with cleaned input.
    fn schemes(rules: &[Rule]) -> Result<Vec<u8>, Error> {
        let mut sorted: Vec<&Rule> = rules.iter().collect();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        let mut out = Vec::new();
        out.extend_from_slice(
            &u16::try_from(sorted.len())
                .map_err(|_| fail("too many postal schemes"))?
                .to_le_bytes(),
        );
        for r in sorted {
            // The rule without prefixes cleans the prefixes.
            let mut bare = Vec::new();
            encode_one(&mut bare, r, &[])?;
            let (s, _) = Scheme::parse(&bare).ok_or(fail("a postal scheme is malformed"))?;
            let mut prefixes = Vec::new();
            for p in &r.prefixes {
                let mut buf = [0_u8; super::MAX_CODE];
                let n = s
                    .clean(p, &mut buf)
                    .ok_or(fail("a scheme prefix is not ASCII"))?;
                prefixes.push(buf.get(..n).unwrap_or(&[]).to_vec());
            }
            encode_one(&mut out, r, &prefixes)?;
        }
        out.resize(out.len().next_multiple_of(4), 0);
        Ok(out)
    }

    fn encode_one(out: &mut Vec<u8>, r: &Rule, prefixes: &[Vec<u8>]) -> Result<(), Error> {
        short(out, r.name.as_bytes())?;
        out.extend_from_slice(&day(r.valid_from)?.to_le_bytes());
        out.extend_from_slice(&day(r.valid_to)?.to_le_bytes());
        out.push(
            if r.upper { super::UPPER } else { 0 }
                | if r.admin_aligned {
                    super::ADMIN_ALIGNED
                } else {
                    0
                },
        );
        short(out, r.ignore.as_bytes())?;
        list(out, prefixes)?;
        short(out, r.display.as_bytes())?;
        let t: Vec<Vec<u8>> = r.templates.iter().map(|t| t.as_bytes().to_vec()).collect();
        list(out, &t)
    }

    /// The place → codes columns: (row, record) pairs in order.
    fn place_index(place: &[u32]) -> [(u16, Vec<u8>); 2] {
        let mut pairs: Vec<(u32, u32)> = place
            .iter()
            .enumerate()
            .map(|(r, &p)| (p, u32::try_from(r).unwrap_or(u32::MAX)))
            .collect();
        pairs.sort_unstable();
        let rows: Vec<u32> = pairs.iter().map(|p| p.0).collect();
        let recs: Vec<u32> = pairs.iter().map(|p| p.1).collect();
        [
            (BY_PLACE, encode_auto(&rows)),
            (BY_PLACE_REC, encode_direct(&recs)),
        ]
    }

    /// The key of `code` in `scheme`: the scheme's name, 0x00, the first canonical form.
    fn key(schemes: Schemes<'_>, scheme: &str, code: &str) -> Result<Vec<u8>, Error> {
        let s = schemes
            .into_iter()
            .find(|s| s.name == scheme)
            .ok_or(fail("a postal code names no scheme"))?;
        let mut first = None;
        s.forms(code, false, |f| {
            first = Some(f.to_vec());
            core::ops::ControlFlow::Break(())
        });
        let form = first.ok_or(fail("a postal code matches none of its templates"))?;
        let mut key = Vec::with_capacity(scheme.len() + 1 + form.len());
        key.extend_from_slice(scheme.as_bytes());
        key.push(0);
        key.extend_from_slice(&form);
        if key.len() > super::fst::MAX_KEY {
            return Err(fail("a postal key is too long"));
        }
        Ok(key)
    }

    /// Encodes a POSTAL body for a pack of `places` rows: `rules` give the schemes, `codes`
    /// the records. Each code is stored in its canonical form under its scheme's rule (the
    /// reader's own normaliser), and codes that are the same key are one key with a record per
    /// place. With `by_place`, the place → codes index is added.
    ///
    /// # Errors
    /// [`Error::Write`] if a rule is malformed or two share a name, a code names no scheme or
    /// matches none of its templates, a place is past the end, a key has the same place twice,
    /// or a redirect names a code that is not in `codes`.
    pub fn encode(
        rules: &[Rule],
        codes: &[Code],
        places: u32,
        by_place: bool,
    ) -> Result<Vec<u8>, Error> {
        let scheme_bytes = schemes(rules)?;
        let parsed = open_schemes(&scheme_bytes)
            .map_err(|_| fail("a postal scheme is malformed or two share a name"))?;
        let canonical = |scheme: &str, code: &str| key(parsed, scheme, code);
        // Key → its records, by place.
        let mut keys: BTreeMap<Vec<u8>, BTreeMap<u32, Rec>> = BTreeMap::new();
        for c in codes {
            if c.place >= places {
                return Err(fail("a postal code names a row past the end"));
            }
            let key = canonical(&c.scheme, &c.code)?;
            let redirect = match &c.redirect {
                Some((s, code)) => Some(canonical(s, code)?),
                None => None,
            };
            let rec = (c.kind, day(c.valid_from)?, day(c.valid_to)?, redirect);
            if keys.entry(key).or_default().insert(c.place, rec).is_some() {
                return Err(fail("a postal key has the same place twice"));
            }
        }
        let ordinal: BTreeMap<&[u8], u32> = keys
            .keys()
            .enumerate()
            .map(|(i, k)| (k.as_slice(), u32::try_from(i).unwrap_or(u32::MAX)))
            .collect();
        let mut b = Builder::new();
        let mut starts = Vec::with_capacity(keys.len() + 1);
        let (mut place, mut kind, mut from, mut to, mut redirect) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (key, recs) in &keys {
            b.insert(key)?;
            starts.push(u32::try_from(place.len()).map_err(|_| fail("too many postal records"))?);
            for (&p, (k, f, t, r)) in recs {
                place.push(p);
                kind.push(*k as u32);
                from.push(*f);
                to.push(*t);
                redirect.push(match r {
                    Some(r) => ordinal
                        .get(r.as_slice())
                        .map(|o| o + 1)
                        .ok_or(fail("a redirect names a code that is not in the pack"))?,
                    None => 0,
                });
            }
        }
        starts.push(u32::try_from(place.len()).map_err(|_| fail("too many postal records"))?);
        let (mut automaton, _) = b.finish()?;
        let len = u32::try_from(automaton.len())
            .map_err(|_| fail("the postal automaton is too large"))?;
        automaton.resize(automaton.len().next_multiple_of(4), 0);
        let mut cols: Vec<(u16, Vec<u8>)> = alloc::vec![
            (LENGTH, encode_direct(&[len])),
            (STARTS, encode_direct(&starts)),
            (PLACE, encode_direct(&place)),
        ];
        // Optional columns only when a value differs from the default.
        for (id, values) in [
            (KIND, &kind),
            (VALID_FROM, &from),
            (VALID_TO, &to),
            (REDIRECT, &redirect),
        ] {
            if values.iter().any(|&v| v != 0) {
                cols.push((id, encode_direct(values)));
            }
        }
        if by_place {
            cols.extend(place_index(&place));
        }
        let mut with: Vec<(u16, Codec, &[u8])> = cols
            .iter()
            .map(|(id, b)| (*id, Codec::Bitpack, b.as_slice()))
            .collect();
        with.push((AUTOMATON, Codec::Bytes, automaton.as_slice()));
        with.push((SCHEMES, Codec::Bytes, scheme_bytes.as_slice()));
        with.sort_by_key(|c| c.0);
        table::encode_with(&with).map_err(|_| fail("the postal table could not be laid out"))
    }
}
