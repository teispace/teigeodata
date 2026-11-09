//! `POSTAL` (kind 11): postal codes to places, for exact lookup, prefix search and place →
//! codes (D-090, D-105; [research note](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/postal.md)).
//!
//! Keys are a **scheme name** (`np`, `np-1991`, `gb`: ASCII `[a-z0-9-]`), a `0x00` byte, then
//! the **canonical code**. One [automaton](crate::search::fst) holds every key, so a key's
//! ordinal indexes its records and a prefix of a code is one ordinal range. Every record links
//! to a PLACES row: the admin unit of an area code, a settlement for a post office. Records are
//! in key order, and consecutive records with the same place form a **run** that stores its
//! place once: a Nepal local level's code and its wards are one run (30608, 3060801 …
//! 3060832), and a set with one record per place costs nothing extra (the run starts ramp-code
//! to zero bits).
//!
//! A [column table](teigeodata_core::table) with these columns:
//!
//! | ID | Column | Values | |
//! |---:|---|---|---|
//! | 1 | [`LENGTH`] | one value: the automaton's length in bytes | required |
//! | 2 | [`AUTOMATON`] | bytes: the automaton, zero-padded to a multiple of 4 | required |
//! | 3 | [`STARTS`] | per key and one more: its first record, strictly increasing from 0 | required |
//! | 4 | [`RUN_START`] | per run: its first record, strictly increasing from 0 | required |
//! | 5 | [`RUN_PLACE`] | per run: its PLACES row; two runs in a row differ | required |
//! | 6 | [`KIND`] | per record: a [`CodeKind`] | optional (all areas) |
//! | 7 | [`VALID_FROM`] | per record: days since 1970-01-01 plus 1; 0 = the scheme's | optional |
//! | 8 | [`VALID_TO`] | per record: the same; 0 = the scheme's | optional |
//! | 9 | [`REDIRECT`] | per record: the ordinal of the key that replaces it, plus 1; 0 = none | optional |
//! | 10 | [`BY_PLACE`] | per run: the runs sorted by (place, run), for place → codes | optional |
//! | 11 | [`SCHEMES`] | bytes: the schemes and their [normalisation rules](Scheme) | required |
//!
//! Within a key, the records' places strictly increase.
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

use teigeodata_core::bitpack::Packed;
use teigeodata_core::table::Table;

use crate::search::fst::{self, Fst};
use crate::{Error, Kind, Section, columns};

/// Column: the automaton's length.
pub const LENGTH: u16 = 1;
/// Column: the automaton.
pub const AUTOMATON: u16 = 2;
/// Column: each key's first record.
pub const STARTS: u16 = 3;
/// Column: each run's first record.
pub const RUN_START: u16 = 4;
/// Column: each run's place.
pub const RUN_PLACE: u16 = 5;
/// Column: each record's kind.
pub const KIND: u16 = 6;
/// Column: each record's first valid day.
pub const VALID_FROM: u16 = 7;
/// Column: each record's last valid day.
pub const VALID_TO: u16 = 8;
/// Column: each record's replacement key.
pub const REDIRECT: u16 = 9;
/// Column: the runs in place order.
pub const BY_PLACE: u16 = 10;
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
/// general category `Nd`, the blocks of the BMP), so a code typed in any script's digits is found.
const DIGIT_ZEROS: [u32; 36] = [
    0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66, 0x0CE6, 0x0D66,
    0x0DE6, 0x0E50, 0x0ED0, 0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946, 0x19D0, 0x1A80, 0x1A90,
    0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900, 0xA9D0, 0xA9F0, 0xAA50, 0xABF0, 0xFF10,
];

/// The ASCII form of `c`: itself if printable ASCII; a full-width form's ASCII; a decimal
/// digit of any script; a Unicode space or dash. `None` for anything else.
#[must_use]
pub fn ascii(c: char) -> Option<u8> {
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

/// The schemes of a pack, in name order. In SCHEMES: a `u16` count, a zero `u16`, a `u32`
/// offset per scheme (from the first scheme), then the schemes, zero-padded to 4 bytes; the
/// offsets make finding a scheme by name a binary search.
#[derive(Clone, Copy, Debug)]
pub struct Schemes<'a> {
    bytes: &'a [u8],
    offsets: &'a [u8],
    left: u16,
}

impl<'a> Schemes<'a> {
    /// Scheme `i`.
    fn get(&self, i: usize) -> Option<Scheme<'a>> {
        let at = self.offsets.get(i * 4..i * 4 + 4)?;
        let at = u32::from_le_bytes(at.try_into().ok()?) as usize;
        Scheme::parse(self.bytes.get(at..)?).map(|s| s.0)
    }

    /// The scheme named `name`: a binary search of the names.
    fn find(&self, name: &str) -> Option<Scheme<'a>> {
        let (mut lo, mut hi) = (0_usize, usize::from(self.left));
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let s = self.get(mid)?;
            match s.name.cmp(name) {
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => return Some(s),
            }
        }
        None
    }
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
    run_start: Packed<'a>,
    run_place: Packed<'a>,
    kind: Option<Packed<'a>>,
    valid_from: Option<Packed<'a>>,
    valid_to: Option<Packed<'a>>,
    redirect: Option<Packed<'a>>,
    by_place: Option<Packed<'a>>,
    records: usize,
    runs: usize,
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
        let run_start = columns::required(&t, k, RUN_START, None)?;
        let runs = run_start.len();
        if (runs == 0) != (records == 0) || runs > records {
            return Err(fail(RUN_START, "the runs do not fit the records"));
        }
        let run_place = columns::required(&t, k, RUN_PLACE, Some(runs))?;
        let opt = |id| columns::optional(&t, k, id, records);
        let (kind, valid_from, valid_to, redirect) =
            (opt(KIND)?, opt(VALID_FROM)?, opt(VALID_TO)?, opt(REDIRECT)?);
        let by_place = columns::optional(&t, k, BY_PLACE, runs)?;
        let fst =
            Fst::new(bytes, keys32).ok_or(fail(AUTOMATON, "the automaton's root is malformed"))?;
        let schemes = open_schemes(t.bytes(SCHEMES).map_err(|e| columns::map(k, e))?)?;
        Ok(Self {
            fst,
            schemes,
            starts,
            run_start,
            run_place,
            kind,
            valid_from,
            valid_to,
            redirect,
            by_place,
            records,
            runs,
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
        self.schemes.find(name)
    }

    /// The scheme named `scheme`, or with `None` every scheme.
    fn chosen(&self, scheme: Option<&str>) -> impl Iterator<Item = Scheme<'a>> {
        let one = scheme.and_then(|n| self.scheme(n));
        let all = scheme.is_none().then(|| self.schemes.into_iter());
        one.into_iter().chain(all.into_iter().flatten())
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
        for s in self.chosen(scheme) {
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
        for s in self.chosen(scheme) {
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

    /// The run holding record `record`: a binary search of the run starts.
    fn run_of(&self, record: usize) -> Option<usize> {
        if record >= self.records {
            return None;
        }
        let r = u32::try_from(record).ok()?;
        let (mut lo, mut hi) = (0_usize, self.runs);
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if self.run_start.get(mid)? <= r {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(lo)
    }

    /// The records of run `run`.
    fn run_records(&self, run: usize) -> Range<usize> {
        let end = if run + 1 < self.runs {
            self.run_start.get(run + 1).map_or(0, |v| v as usize)
        } else {
            self.records
        };
        self.run_start
            .get(run)
            .map_or(0..0, |s| s as usize..end.min(self.records))
    }

    /// Record `record`'s place, if below the place count.
    #[must_use]
    pub fn place_of(&self, record: usize) -> Option<u32> {
        let p = self.run_place.get(self.run_of(record)?)?;
        ((p as usize) < self.places).then_some(p)
    }

    /// Record `record`, with its scheme's validity where it has none of its own.
    #[must_use]
    pub fn record(&self, record: usize) -> Option<Record> {
        let place = self.place_of(record)?;
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
        let Some(order) = &self.by_place else {
            return;
        };
        let place_at = |i: usize| {
            order
                .get(i)
                .and_then(|run| self.run_place.get(run as usize))
        };
        // The first entry whose run's place is not below `place`.
        let (mut lo, mut hi) = (0_usize, self.runs);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match place_at(mid) {
                Some(p) if p < place => lo = mid + 1,
                _ => hi = mid,
            }
        }
        let mut i = lo;
        while place_at(i) == Some(place) {
            if let Some(run) = order.get(i) {
                for r in self.run_records(run as usize) {
                    if visit(r).is_break() {
                        return;
                    }
                }
            }
            i += 1;
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
        // Keys of one scheme are consecutive: the last scheme found is tried first.
        let mut last: Option<Scheme<'_>> = None;
        self.fst
            .check(|_, key| {
                if bad.is_some() {
                    return;
                }
                let ok = key.iter().position(|&b| b == 0).is_some_and(|cut| {
                    let (name, code) = (key.get(..cut), key.get(cut + 1..));
                    let (Some(n), Some(c)) = (name, code) else {
                        return false;
                    };
                    if last.is_none_or(|s| s.name.as_bytes() != n) {
                        last = core::str::from_utf8(n).ok().and_then(|n| self.scheme(n));
                    }
                    !c.is_empty() && last.is_some_and(|s| s.canonical(c))
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
        self.check_runs()?;
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
        if let Some(order) = &self.by_place {
            let mut last: Option<(u32, u32)> = None;
            for e in 0..self.runs {
                let run = order
                    .get(e)
                    .ok_or(fail(BY_PLACE, "a place index entry is missing"))?;
                let p = self
                    .run_place
                    .get(run as usize)
                    .ok_or(fail(BY_PLACE, "a place index entry names no run"))?;
                // Strictly increasing (place, run) over `runs` entries below `runs`: a
                // permutation of the runs in place order.
                if last.is_some_and(|l| (p, run) <= l) {
                    return Err(fail(BY_PLACE, "the place index is not in place order"));
                }
                last = Some((p, run));
            }
        }
        Ok(())
    }
}

impl Postal<'_> {
    /// The runs: starts strictly increasing from 0 below the record count, places below the
    /// place count and differing from the run before; and within each key, places strictly
    /// increasing.
    fn check_runs(&self) -> Result<(), Error> {
        let mut prev: Option<(u32, u32)> = None;
        for run in 0..self.runs {
            let (s, p) = (self.run_start.get(run), self.run_place.get(run));
            let (Some(s), Some(p)) = (s, p) else {
                return Err(fail(RUN_START, "a run is missing"));
            };
            let start_ok = match prev {
                None => s == 0,
                Some((ps, pp)) => s > ps && p != pp,
            };
            if !start_ok || s as usize >= self.records {
                return Err(fail(RUN_START, "the runs are not in order or not maximal"));
            }
            if p as usize >= self.places {
                return Err(fail(RUN_PLACE, "a run names a row past the end"));
            }
            prev = Some((s, p));
        }
        // One pass over the records, the run advancing with them.
        let mut run = 0_usize;
        let mut next = self.run_records(0).end;
        for o in 0..self.keys() {
            let mut last = None;
            for r in self.records_of(o) {
                while r >= next && run + 1 < self.runs {
                    run += 1;
                    next = self.run_records(run).end;
                }
                let p = self.run_place.get(run).unwrap_or(u32::MAX);
                if last.is_some_and(|l| p <= l) {
                    return Err(fail(RUN_PLACE, "a key's records are not in place order"));
                }
                last = Some(p);
            }
        }
        Ok(())
    }
}

/// The SCHEMES column: its count, every scheme valid and in strictly increasing name order,
/// and only zero padding (fewer than 4 bytes) after the last.
fn open_schemes(bytes: &[u8]) -> Result<Schemes<'_>, Error> {
    let bad = || fail(SCHEMES, "a scheme is malformed or out of order");
    let (head, rest) = bytes.split_first_chunk::<4>().ok_or_else(bad)?;
    let count = u16::from_le_bytes([head[0], head[1]]);
    if count == 0 || head[2] != 0 || head[3] != 0 {
        return Err(bad());
    }
    let (offsets, all) = rest
        .split_at_checked(usize::from(count) * 4)
        .ok_or_else(bad)?;
    let mut at = all;
    let mut last: Option<&str> = None;
    for i in 0..usize::from(count) {
        let want = offsets.get(i * 4..i * 4 + 4).ok_or_else(bad)?;
        if u32::from_le_bytes(want.try_into().map_err(|_| bad())?) as usize != all.len() - at.len()
        {
            return Err(bad());
        }
        let (s, r) = Scheme::parse(at).ok_or_else(bad)?;
        if !s.valid() || last.is_some_and(|l| s.name <= l) {
            return Err(bad());
        }
        last = Some(s.name);
        at = r;
    }
    if at.len() >= 4 || at.iter().any(|&b| b != 0) {
        return Err(fail(SCHEMES, "the schemes are followed by bytes"));
    }
    Ok(Schemes {
        bytes: all,
        offsets,
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

    use teigeodata_core::bitpack::encode_direct;
    use teigeodata_core::table::{self, Codec};

    use super::{
        AUTOMATON, BY_PLACE, CodeKind, KIND, LENGTH, REDIRECT, RUN_PLACE, RUN_START, SCHEMES,
        STARTS, Scheme, VALID_FROM, VALID_TO, open_schemes,
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
        let mut body = Vec::new();
        let mut offsets = Vec::with_capacity(sorted.len() * 4);
        for r in &sorted {
            let at =
                u32::try_from(body.len()).map_err(|_| fail("the postal schemes are too large"))?;
            offsets.extend_from_slice(&at.to_le_bytes());
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
            encode_one(&mut body, r, &prefixes)?;
        }
        let mut out = Vec::with_capacity(4 + offsets.len() + body.len() + 3);
        out.extend_from_slice(
            &u16::try_from(sorted.len())
                .map_err(|_| fail("too many postal schemes"))?
                .to_le_bytes(),
        );
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&offsets);
        out.extend_from_slice(&body);
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

    /// The runs of the records' places: (starts, places, the runs in (place, run) order).
    fn runs(place: &[u32]) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
        let (mut starts, mut places) = (Vec::new(), Vec::new());
        for (r, &p) in place.iter().enumerate() {
            if places.last() != Some(&p) {
                starts.push(u32::try_from(r).unwrap_or(u32::MAX));
                places.push(p);
            }
        }
        let mut order: Vec<u32> = (0..u32::try_from(places.len()).unwrap_or(u32::MAX)).collect();
        order.sort_by_key(|&run| (places.get(run as usize).copied(), run));
        (starts, places, order)
    }

    /// Appends the key of `code` in `scheme` to `arena`: the scheme's name, 0x00, the first
    /// canonical form. Its length.
    fn key_into(
        schemes: &BTreeMap<&str, Scheme<'_>>,
        scheme: &str,
        code: &str,
        arena: &mut Vec<u8>,
    ) -> Result<u8, Error> {
        let s = schemes
            .get(scheme)
            .ok_or(fail("a postal code names no scheme"))?;
        let start = arena.len();
        arena.extend_from_slice(scheme.as_bytes());
        arena.push(0);
        let mut found = false;
        s.forms(code, false, |f| {
            arena.extend_from_slice(f);
            found = true;
            core::ops::ControlFlow::Break(())
        });
        if !found {
            arena.truncate(start);
            return Err(fail("a postal code matches none of its templates"));
        }
        u8::try_from(arena.len() - start).map_err(|_| fail("a postal key is too long"))
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
        let by_name: BTreeMap<&str, Scheme<'_>> = parsed.into_iter().map(|s| (s.name, s)).collect();
        // Every key in one arena; an entry is (key start, key length, place, code index), so
        // memory stays near the key text for millions of codes.
        let mut arena = Vec::new();
        let mut entries: Vec<(u32, u8, u32, u32)> = Vec::with_capacity(codes.len());
        for (i, c) in codes.iter().enumerate() {
            if c.place >= places {
                return Err(fail("a postal code names a row past the end"));
            }
            let start = u32::try_from(arena.len()).map_err(|_| fail("too much postal key text"))?;
            let len = key_into(&by_name, &c.scheme, &c.code, &mut arena)?;
            let i = u32::try_from(i).map_err(|_| fail("too many postal records"))?;
            entries.push((start, len, c.place, i));
        }
        let text = |e: &(u32, u8, u32, u32)| {
            arena
                .get(e.0 as usize..e.0 as usize + usize::from(e.1))
                .unwrap_or(&[])
        };
        entries.sort_unstable_by(|a, b| text(a).cmp(text(b)).then(a.2.cmp(&b.2)));
        // The first entry of each key, in key order.
        let mut firsts: Vec<usize> = Vec::new();
        for (j, e) in entries.iter().enumerate() {
            match j.checked_sub(1).and_then(|p| entries.get(p)) {
                Some(prev) if text(prev) == text(e) => {
                    if prev.2 == e.2 {
                        return Err(fail("a postal key has the same place twice"));
                    }
                }
                _ => firsts.push(j),
            }
        }
        let ordinal = |key: &[u8]| -> Option<u32> {
            let at = firsts
                .binary_search_by(|&j| entries.get(j).map_or(&[][..], text).cmp(key))
                .ok()?;
            u32::try_from(at).ok()
        };
        let mut b = Builder::new();
        let mut starts = Vec::with_capacity(firsts.len() + 1);
        for &j in &firsts {
            b.insert(entries.get(j).map_or(&[][..], text))?;
            starts.push(u32::try_from(j).map_err(|_| fail("too many postal records"))?);
        }
        let n = entries.len();
        starts.push(u32::try_from(n).map_err(|_| fail("too many postal records"))?);
        let (mut place, mut kind, mut from, mut to, mut redirect) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );
        let mut scratch = Vec::new();
        for e in &entries {
            let c = codes
                .get(e.3 as usize)
                .ok_or(fail("a postal entry lost its code"))?;
            place.push(e.2);
            kind.push(c.kind as u32);
            from.push(day(c.valid_from)?);
            to.push(day(c.valid_to)?);
            redirect.push(match &c.redirect {
                Some((s, code)) => {
                    scratch.clear();
                    key_into(&by_name, s, code, &mut scratch)?;
                    ordinal(&scratch)
                        .map(|o| o + 1)
                        .ok_or(fail("a redirect names a code that is not in the pack"))?
                }
                None => 0,
            });
        }
        let records = Records {
            place,
            kind,
            from,
            to,
            redirect,
        };
        layout(b, &starts, &records, by_place, &scheme_bytes)
    }

    /// The per-record columns, in key order.
    struct Records {
        place: Vec<u32>,
        kind: Vec<u32>,
        from: Vec<u32>,
        to: Vec<u32>,
        redirect: Vec<u32>,
    }

    /// The column table of a POSTAL body.
    fn layout(
        b: Builder,
        starts: &[u32],
        r: &Records,
        by_place: bool,
        scheme_bytes: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let (mut automaton, _) = b.finish()?;
        let len = u32::try_from(automaton.len())
            .map_err(|_| fail("the postal automaton is too large"))?;
        automaton.resize(automaton.len().next_multiple_of(4), 0);
        let mut cols: Vec<(u16, Vec<u8>)> = alloc::vec![
            (LENGTH, encode_direct(&[len])),
            (STARTS, encode_direct(starts)),
        ];
        let (run_start, run_place, order) = runs(&r.place);
        cols.push((RUN_START, encode_direct(&run_start)));
        cols.push((RUN_PLACE, encode_direct(&run_place)));
        // Optional columns only when a value differs from the default.
        for (id, values) in [
            (KIND, &r.kind),
            (VALID_FROM, &r.from),
            (VALID_TO, &r.to),
            (REDIRECT, &r.redirect),
        ] {
            if values.iter().any(|&v| v != 0) {
                cols.push((id, encode_direct(values)));
            }
        }
        if by_place {
            cols.push((BY_PLACE, encode_direct(&order)));
        }
        let mut with: Vec<(u16, Codec, &[u8])> = cols
            .iter()
            .map(|(id, b)| (*id, Codec::Bitpack, b.as_slice()))
            .collect();
        with.push((AUTOMATON, Codec::Bytes, automaton.as_slice()));
        with.push((SCHEMES, Codec::Bytes, scheme_bytes));
        with.sort_by_key(|c| c.0);
        table::encode_with(&with).map_err(|_| fail("the postal table could not be laid out"))
    }
}
