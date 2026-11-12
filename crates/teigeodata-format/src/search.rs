//! `SEARCH` (kind 5): name keys to places, for exact, prefix and fuzzy search (D-052, B7).
//!
//! Keys are byte strings that start with a **tag** naming their map ([`EXACT`], [`LOOSE`],
//! [`PHONETIC`], [`MACHINE`]; the forge defines how each map folds a name), then the folded name. All maps
//! share one [automaton](fst), so their common suffixes are stored once. A key's ordinal (its
//! position in sorted order) indexes its **list** of rows: the places with that key, most
//! important first. Fuzzy matching counts edits in characters, so every script costs the same
//! ([`fuzzy`]).
//!
//! A [column table](teigeodata_core::table) with these columns:
//!
//! | ID | Column | Values |
//! |---:|---|---|
//! | 1 | [`LENGTH`] | one value: the automaton's length in bytes |
//! | 2 | [`AUTOMATON`] | bytes: the [automaton](fst), zero-padded to a multiple of 4 |
//! | 3 | [`STARTS`] | per key and one more: its list's first entry in IDS, from 0 |
//! | 4 | [`IDS`] | every list: rows by importance descending, then row ascending |
//! | 5 | [`IMPORTANCE`] | per row: its importance, 0 to 255 |
//! | 6 | [`BLOCK_MAX`] | per [`BLOCK`] keys: the highest importance in their lists |
//! | 7 | [`RULES`] | bytes: the country's query [rules] (optional) |
//! | 8 | [`UNIT`] | per row: its unit type as the rules name it, 0 = none (optional) |
//!
//! `SUBSTRING` (kind 23, D-199) has the same layout with one map, [`SUBSTRING`]: the word
//! suffixes of each folded name (at least [`SUBSTRING_MIN`] code points), so a query from the
//! middle of a word is a prefix range over them. It has no RULES or UNIT column, and
//! [`Search::check`] also proves every key's tag and length. [`Search::open`] reads both kinds.
//!
//! Its item count is the number of places (rows). [`Search::open`] checks the structure;
//! [`Search::check`] every key, list, importance and block maximum. Queries never panic and never
//! allocate, and return only rows below the row count.

pub mod fst;
pub mod fuzzy;
pub mod rules;

use core::ops::{ControlFlow, Range};

use teigeodata_core::bitpack::Packed;
use teigeodata_core::table::Table;

use crate::{Error, Kind, Section, columns};
use fst::{Fst, Matcher, Walked};
use rules::Rules;

/// Column: the automaton's length.
pub const LENGTH: u16 = 1;
/// Column: the automaton.
pub const AUTOMATON: u16 = 2;
/// Column: each key's first list entry.
pub const STARTS: u16 = 3;
/// Column: the lists of rows.
pub const IDS: u16 = 4;
/// Column: each row's importance.
pub const IMPORTANCE: u16 = 5;
/// Column: the highest importance per block of keys.
pub const BLOCK_MAX: u16 = 6;
/// Column: the query rules.
pub const RULES: u16 = 7;
/// Column: each row's unit type.
pub const UNIT: u16 = 8;

/// Tag of the exact map: the search fold of each name.
pub const EXACT: u8 = 1;
/// Tag of the loose map: the fold without spaces, and without a droppable suffix.
pub const LOOSE: u8 = 2;
/// Tag of the phonetic map: the phonetic key.
pub const PHONETIC: u8 = 3;
/// Tag of the substring map, the one map of a SUBSTRING section: word suffixes (D-199).
pub const SUBSTRING: u8 = 4;
/// Tag of the machine map: the search fold of each name a program wrote (D-228,
/// [`Origin::is_machine`](crate::names::Origin::is_machine)), kept apart so a query scores
/// such a match below a name a person wrote.
pub const MACHINE: u8 = 5;
/// The fewest code points of a substring key, and of a substring query (D-199).
pub const SUBSTRING_MIN: usize = 3;

/// Keys per block maximum.
pub const BLOCK: usize = 128;

fn fail(kind: Kind, column: u16, problem: &'static str) -> Error {
    Error::Payload {
        kind: kind.0,
        column,
        problem,
    }
}

/// A view of a SEARCH section.
#[derive(Clone, Copy, Debug)]
pub struct Search<'a> {
    fst: Fst<'a>,
    starts: Packed<'a>,
    ids: Packed<'a>,
    importance: Packed<'a>,
    block_max: Packed<'a>,
    rules: Option<Rules<'a>>,
    unit: Option<Packed<'a>>,
    rows: usize,
    kind: Kind,
}

impl<'a> Search<'a> {
    /// Opens the section: the frame, every column's structure and length, and the automaton's
    /// root, in O(columns + blocks).
    ///
    /// # Errors
    /// [`Error::Payload`] if the section is not a SEARCH column table, or a column is missing,
    /// malformed or of the wrong length.
    pub fn open(section: &Section<'a>) -> Result<Self, Error> {
        if !section.part().is_base() {
            return Err(fail(
                section.kind(),
                0,
                "a part other than the base opens with its base part (Search::open_part)",
            ));
        }
        Self::open_with(section, None)
    }

    /// Opens a part other than the base of SEARCH or SUBSTRING (ADR-0040): its keys and lists,
    /// with the importance of `base`, the kind's base part, which it does not repeat. A part has
    /// no query rules or unit types of its own; a reader takes the base's.
    ///
    /// # Errors
    /// As [`Search::open`], and if the part holds an importance, rules or unit column, or its
    /// row count differs from the base's.
    pub fn open_part(section: &Section<'a>, base: &Self) -> Result<Self, Error> {
        if section.kind() != base.kind || section.item_count() != base.rows as u64 {
            return Err(fail(
                section.kind(),
                0,
                "a part's kind or row count differs from its base part's",
            ));
        }
        Self::open_with(section, Some(base.importance))
    }

    fn open_with(section: &Section<'a>, base: Option<Packed<'a>>) -> Result<Self, Error> {
        let kind = if section.kind() == Kind::SUBSTRING {
            Kind::SUBSTRING
        } else {
            Kind::SEARCH
        };
        let fail = |column, problem| fail(kind, column, problem);
        let t: Table<'a> = columns::open(section, kind)?;
        let rows = usize::try_from(section.item_count())
            .map_err(|_| fail(0, "the row count does not fit this platform"))?;
        // SUBSTRING has no query rules or unit types.
        let last = if kind == Kind::SUBSTRING {
            BLOCK_MAX
        } else {
            UNIT
        };
        if t.ids().any(|id| id == 0 || id > last) {
            return Err(fail(0, "an unknown column"));
        }
        let length = columns::required(&t, kind, LENGTH, Some(1))?;
        let len = length
            .get(0)
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(0);
        let padded = t.bytes(AUTOMATON).map_err(|e| columns::map(kind, e))?;
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
        let starts = columns::required(&t, kind, STARTS, None)?;
        let keys = starts
            .len()
            .checked_sub(1)
            .ok_or(fail(STARTS, "the list starts are empty"))?;
        let keys32 = u32::try_from(keys).map_err(|_| fail(STARTS, "too many keys"))?;
        let entries = starts
            .get(keys)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or(fail(STARTS, "the list starts are empty"))?;
        let ids = columns::required(&t, kind, IDS, Some(entries))?;
        let importance = match base {
            None => columns::required(&t, kind, IMPORTANCE, Some(rows))?,
            Some(imp) => {
                if t.ids()
                    .any(|id| id == IMPORTANCE || id == RULES || id == UNIT)
                {
                    return Err(fail(
                        IMPORTANCE,
                        "a part repeats its base part's importance, rules or unit types",
                    ));
                }
                imp
            }
        };
        let block_max = columns::required(&t, kind, BLOCK_MAX, Some(keys.div_ceil(BLOCK)))?;
        let fst =
            Fst::new(bytes, keys32).ok_or(fail(AUTOMATON, "the automaton's root is malformed"))?;
        let rules = if t.ids().any(|id| id == RULES) {
            Some(Rules::new(
                t.bytes(RULES).map_err(|e| columns::map(kind, e))?,
            ))
        } else {
            None
        };
        let unit = columns::optional(&t, kind, UNIT, rows)?;
        Ok(Self {
            fst,
            starts,
            ids,
            importance,
            block_max,
            rules,
            unit,
            rows,
            kind,
        })
    }

    /// The section's kind: SEARCH or SUBSTRING.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// The number of keys.
    #[must_use]
    pub const fn keys(&self) -> u32 {
        self.fst.keys()
    }

    /// The number of places.
    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }

    /// The automaton.
    #[must_use]
    pub const fn automaton(&self) -> Fst<'a> {
        self.fst
    }

    /// The country's query rules (none without the column).
    #[must_use]
    pub fn rules(&self) -> Rules<'a> {
        self.rules.unwrap_or_default()
    }

    /// Row `row`'s unit type, 0 for none (and without the column).
    #[must_use]
    pub fn unit(&self, row: u32) -> u8 {
        self.unit
            .and_then(|u| u.get(row as usize))
            .and_then(|v| u8::try_from(v).ok())
            .unwrap_or(0)
    }

    /// The ordinal of `key` in map `tag`.
    #[must_use]
    pub fn get(&self, tag: u8, key: &str) -> Option<u32> {
        let mut buf = [0_u8; fst::MAX_KEY];
        let k = tagged(&mut buf, tag, key)?;
        self.fst.get(k)
    }

    /// The ordinals of the keys of map `tag` starting with `prefix`.
    #[must_use]
    pub fn range(&self, tag: u8, prefix: &str) -> Range<u32> {
        let mut buf = [0_u8; fst::MAX_KEY];
        match tagged(&mut buf, tag, prefix) {
            Some(p) => {
                let (lo, hi) = self.fst.range(p);
                lo..hi
            }
            None => 0..0,
        }
    }

    /// Visits, in key order, the keys of map `tag` that `matcher` accepts (see [`Fst::walk`]).
    pub fn walk<M: Matcher>(
        &self,
        tag: u8,
        matcher: &M,
        anchor: Option<char>,
        budget: usize,
        visit: impl FnMut(u32, u8) -> ControlFlow<()>,
    ) -> Walked {
        self.fst.walk(&[tag], matcher, anchor, budget, visit)
    }

    /// The entries of key `ordinal`'s list in IDS; empty past the end.
    #[must_use]
    pub fn list(&self, ordinal: u32) -> Range<usize> {
        let o = ordinal as usize;
        match (self.starts.get(o), self.starts.get(o + 1)) {
            (Some(a), Some(b)) if a <= b => a as usize..(b as usize).min(self.ids.len()),
            _ => 0..0,
        }
    }

    /// The row at list entry `entry`, if it is below the row count.
    #[must_use]
    pub fn row(&self, entry: usize) -> Option<u32> {
        self.ids.get(entry).filter(|&r| (r as usize) < self.rows)
    }

    /// Calls `f` with the rows of key `ordinal`, most important first, until it breaks.
    pub fn rows_of(&self, ordinal: u32, mut f: impl FnMut(u32) -> ControlFlow<()>) {
        for r in self.ids.values(self.list(ordinal)) {
            if (r as usize) < self.rows && f(r).is_break() {
                return;
            }
        }
    }

    /// Row `row`'s importance (0 past the end).
    #[must_use]
    pub fn importance(&self, row: u32) -> u8 {
        self.importance
            .get(row as usize)
            .map_or(0, |v| u8::try_from(v).unwrap_or(u8::MAX))
    }

    /// The highest importance in key `ordinal`'s list: its first row's.
    #[must_use]
    pub fn key_max(&self, ordinal: u32) -> u8 {
        let first = self.list(ordinal).start;
        self.row(first).map_or(0, |r| self.importance(r))
    }

    /// The highest importance in the lists of the keys of block `block`.
    #[must_use]
    pub fn block_max(&self, block: usize) -> u8 {
        self.block_max
            .get(block)
            .map_or(0, |v| u8::try_from(v).unwrap_or(u8::MAX))
    }

    /// Visits the keys in `range` whose lists can still beat `floor`: whole blocks whose maximum
    /// is not above `floor()` are skipped, as are single keys. `floor` is read again after
    /// every visit, so a caller keeping the best `k` raises it as it fills. Returns the keys
    /// read.
    pub fn best_in(
        &self,
        range: Range<u32>,
        mut floor: impl FnMut() -> i32,
        mut visit: impl FnMut(u32) -> ControlFlow<()>,
    ) -> usize {
        let mut read = 0;
        let mut o = range.start;
        while o < range.end {
            let block = o as usize / BLOCK;
            let end = u32::try_from((block + 1) * BLOCK)
                .unwrap_or(u32::MAX)
                .min(range.end);
            if i32::from(self.block_max(block)) <= floor() {
                o = end;
                continue;
            }
            while o < end {
                read += 1;
                if i32::from(self.key_max(o)) > floor() && visit(o).is_break() {
                    return read;
                }
                o += 1;
            }
        }
        read
    }

    /// Checks every key, list, importance and block maximum: O(automaton + entries).
    ///
    /// # Errors
    /// [`Error::Payload`] if the automaton is not canonical, a SUBSTRING key is not its tag
    /// and [`SUBSTRING_MIN`] or more code points of UTF-8, a list is empty, out of order or
    /// names a row past the end, an importance is above 255, or a block maximum is wrong.
    pub fn check(&self) -> Result<(), Error> {
        let fail = |column, problem| fail(self.kind, column, problem);
        // A SUBSTRING key is its tag, then at least SUBSTRING_MIN code points of UTF-8.
        let substring = self.kind == Kind::SUBSTRING;
        let mut bad_key = false;
        self.fst
            .check(|_, key| {
                bad_key |= substring && !substring_key(key);
            })
            .map_err(|p| fail(AUTOMATON, p))?;
        if bad_key {
            return Err(fail(
                AUTOMATON,
                "a substring key is not its tag and 3 or more code points of UTF-8",
            ));
        }
        let units = match &self.rules {
            Some(r) => r.check()?,
            None => 0,
        };
        if let Some(u) = self.unit {
            for r in 0..self.rows {
                if u.get(r).is_none_or(|v| v > u32::from(units)) {
                    return Err(fail(UNIT, "a row names a unit type the rules do not"));
                }
            }
        }
        let mut prev = 0_u32;
        for o in 0..self.starts.len() {
            let s = self
                .starts
                .get(o)
                .ok_or(fail(STARTS, "a list start is missing"))?;
            if o == 0 && s != 0 {
                return Err(fail(STARTS, "the first list does not start at 0"));
            }
            if o > 0 && s <= prev {
                return Err(fail(STARTS, "a list is empty or the starts decrease"));
            }
            prev = s;
        }
        for r in 0..self.rows {
            if self
                .importance
                .get(r)
                .is_none_or(|v| v > u32::from(u8::MAX))
            {
                return Err(fail(IMPORTANCE, "an importance is above 255"));
            }
        }
        let mut block_best = 0_u8;
        for o in 0..self.keys() {
            let mut last: Option<(u8, u32)> = None;
            for e in self.list(o) {
                let r = self
                    .ids
                    .get(e)
                    .ok_or(fail(IDS, "a list entry is missing"))?;
                if r as usize >= self.rows {
                    return Err(fail(IDS, "a list names a row past the end"));
                }
                let imp = self.importance(r);
                if let Some((li, lr)) = last
                    && (imp > li || (imp == li && r <= lr))
                {
                    return Err(fail(IDS, "a list is not in importance order"));
                }
                last = Some((imp, r));
            }
            block_best = block_best.max(self.key_max(o));
            let o = o as usize;
            if (o + 1).is_multiple_of(BLOCK) || o + 1 == self.keys() as usize {
                if self.block_max(o / BLOCK) != block_best
                    || self.block_max.get(o / BLOCK).is_none_or(|v| v > 255)
                {
                    return Err(fail(BLOCK_MAX, "a block maximum is wrong"));
                }
                block_best = 0;
            }
        }
        Ok(())
    }
}

/// The SEARCH (or SUBSTRING) parts a reader has (ADR-0040): the base part, with the rows'
/// importance, the query rules and the unit types, then each other part's keys and lists.
#[derive(Clone, Debug, Default)]
pub struct SearchSet<'a> {
    parts: alloc::vec::Vec<Search<'a>>,
}

impl<'a> SearchSet<'a> {
    /// The set of `sections`, parts of one kind in table order, the base part first.
    ///
    /// # Errors
    /// As [`Search::open`] and [`Search::open_part`].
    pub fn open(sections: impl IntoIterator<Item = Section<'a>>) -> Result<Self, Error> {
        let mut parts: alloc::vec::Vec<Search<'a>> = alloc::vec::Vec::new();
        for s in sections {
            let opened = match parts.first() {
                None => Search::open(&s)?,
                Some(base) => Search::open_part(&s, base)?,
            };
            parts.push(opened);
        }
        Ok(Self { parts })
    }

    /// The base part: importance, rules and unit types.
    #[must_use]
    pub fn base(&self) -> Option<&Search<'a>> {
        self.parts.first()
    }

    /// Every part, the base first.
    #[must_use]
    pub fn parts(&self) -> &[Search<'a>] {
        &self.parts
    }
}

/// Whether `key` is a SUBSTRING key: the tag, then [`SUBSTRING_MIN`] or more code points of
/// UTF-8.
fn substring_key(key: &[u8]) -> bool {
    match key.split_first() {
        Some((&SUBSTRING, text)) => {
            core::str::from_utf8(text).is_ok_and(|t| t.chars().nth(SUBSTRING_MIN - 1).is_some())
        }
        _ => false,
    }
}

/// `tag` then `key` in `buf`, if it fits a key.
fn tagged<'b>(buf: &'b mut [u8; fst::MAX_KEY], tag: u8, key: &str) -> Option<&'b [u8]> {
    let n = key.len() + 1;
    let out = buf.get_mut(..n)?;
    let (t, rest) = out.split_first_mut()?;
    *t = tag;
    rest.copy_from_slice(key.as_bytes());
    Some(out)
}

#[cfg(feature = "write")]
pub use write::{
    Extra, Maps, SubstringKeys, encode, encode_maps, encode_maps_with, encode_substring,
    encode_with, substring_from_names, substring_part_from_names, tagged_keys,
};

#[cfg(feature = "write")]
mod write {
    use alloc::collections::{BTreeMap, BTreeSet};
    use alloc::vec::Vec;

    use teigeodata_core::bitpack::{Coding, encode as pack, encode_direct};
    use teigeodata_core::table::{self, Codec};

    use super::fst::Builder;
    use super::{
        AUTOMATON, BLOCK, BLOCK_MAX, EXACT, IDS, IMPORTANCE, LENGTH, LOOSE, MACHINE, PHONETIC,
        RULES, STARTS, SUBSTRING, SUBSTRING_MIN, UNIT,
    };
    use crate::Error;

    /// The three maps of a SEARCH section, each from a key's text to its rows.
    #[derive(Clone, Debug, Default)]
    pub struct Maps {
        /// The search fold of each name.
        pub exact: BTreeMap<alloc::string::String, BTreeSet<u32>>,
        /// The fold without spaces, and without a droppable suffix.
        pub loose: BTreeMap<alloc::string::String, BTreeSet<u32>>,
        /// The phonetic key.
        pub phonetic: BTreeMap<alloc::string::String, BTreeSet<u32>>,
        /// The search fold of each machine name (D-228).
        pub machine: BTreeMap<alloc::string::String, BTreeSet<u32>>,
    }

    /// Encodes a SEARCH body from its three maps (D-086): a loose key keeps only the rows its
    /// exact twin (the same text) lacks, since a loose lookup reads both, and is left out when
    /// none remain. Empty keys are skipped.
    ///
    /// # Errors
    /// As [`encode`].
    pub fn encode_maps(maps: &Maps, importance: &[u8]) -> Result<Vec<u8>, Error> {
        encode_maps_with(maps, importance, &Extra::default())
    }

    /// The optional columns: the country's query rules and each row's unit type.
    #[derive(Clone, Debug, Default)]
    pub struct Extra<'r> {
        /// The rules; none leaves RULES out.
        pub rules: Option<&'r [super::rules::Rule<'r>]>,
        /// Each row's unit type (0 = none); empty leaves UNIT out.
        pub units: &'r [u8],
        /// A part other than the base (ADR-0040): IMPORTANCE is left out, its reader borrows
        /// the base part's ([`Search::open_part`](super::Search::open_part)); no rules or units.
        pub part: bool,
    }

    /// As [`encode_maps`], with the optional columns.
    ///
    /// # Errors
    /// As [`encode_with`].
    pub fn encode_maps_with(
        maps: &Maps,
        importance: &[u8],
        extra: &Extra<'_>,
    ) -> Result<Vec<u8>, Error> {
        encode_with(&tagged_keys(maps), importance, extra)
    }

    /// The tagged keys of the three maps, as [`encode_maps`] writes them: a loose key keeps
    /// only the rows its exact twin lacks, and empty keys are skipped.
    #[must_use]
    pub fn tagged_keys(maps: &Maps) -> BTreeMap<Vec<u8>, BTreeSet<u32>> {
        let mut keys: BTreeMap<Vec<u8>, BTreeSet<u32>> = BTreeMap::new();
        for (tag, map) in [
            (EXACT, &maps.exact),
            (LOOSE, &maps.loose),
            (PHONETIC, &maps.phonetic),
            (MACHINE, &maps.machine),
        ] {
            for (text, rows) in map {
                let rows: BTreeSet<u32> = match maps.exact.get(text) {
                    Some(twin) if tag == LOOSE => rows.difference(twin).copied().collect(),
                    _ => rows.clone(),
                };
                if text.is_empty() || rows.is_empty() {
                    continue;
                }
                let mut key = Vec::with_capacity(text.len() + 1);
                key.push(tag);
                key.extend_from_slice(text.as_bytes());
                keys.insert(key, rows);
            }
        }
        keys
    }

    /// Encodes a SUBSTRING body (D-199): `keys` maps each word suffix (folded, untagged) to its
    /// rows; `importance` holds every row's importance, as SEARCH's.
    ///
    /// # Errors
    /// As [`encode`], and if a key has fewer than [`SUBSTRING_MIN`] code points.
    pub fn encode_substring(
        keys: &BTreeMap<alloc::string::String, BTreeSet<u32>>,
        importance: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let mut tagged: BTreeMap<Vec<u8>, BTreeSet<u32>> = BTreeMap::new();
        for (text, rows) in keys {
            if text.chars().nth(SUBSTRING_MIN - 1).is_none() {
                return Err(Error::Write("a substring key has fewer than 3 code points"));
            }
            let mut key = Vec::with_capacity(text.len() + 1);
            key.push(SUBSTRING);
            key.extend_from_slice(text.as_bytes());
            tagged.insert(key, rows.clone());
        }
        encode(&tagged, importance)
    }

    /// Distinct keys, each given a dense ID in first-seen order: an open-addressing table over
    /// one text buffer (FNV-1a), no allocation per key (D-235). A `BTreeMap` of boxed keys took
    /// five times as long for the 1.15 million keys of the Tithipatra layer.
    #[derive(Default)]
    struct Interner {
        /// Per slot, an ID plus one; 0 is empty. The length is a power of two.
        slots: Vec<u32>,
        /// Per ID, its text's start and end in `text`.
        spans: Vec<(u32, u32)>,
        text: alloc::string::String,
    }

    impl Interner {
        fn hash(k: &[u8]) -> u64 {
            k.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
                (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
            })
        }

        const fn len(&self) -> usize {
            self.spans.len()
        }

        fn text(&self, id: u32) -> &str {
            self.spans
                .get(id as usize)
                .and_then(|&(a, b)| self.text.get(a as usize..b as usize))
                .unwrap_or_default()
        }

        /// The slot of `k`: where it is, or the empty one where it goes.
        fn slot(&self, k: &str) -> usize {
            let mask = self.slots.len() - 1;
            #[allow(clippy::cast_possible_truncation, reason = "masked to the table")]
            let mut i = Self::hash(k.as_bytes()) as usize & mask;
            loop {
                match self.slots.get(i).copied().unwrap_or(0) {
                    0 => return i,
                    s if self.text(s - 1) == k => return i,
                    _ => i = (i + 1) & mask,
                }
            }
        }

        /// The ID of `k`, added if new; `None` past `u32` IDs or text.
        fn id(&mut self, key: &str) -> Option<u32> {
            if self.slots.len() < 2 * (self.spans.len() + 1) {
                self.grow();
            }
            let at = self.slot(key);
            let taken = self.slots.get(at).copied().unwrap_or(0);
            if taken != 0 {
                return Some(taken - 1);
            }
            let id = u32::try_from(self.spans.len())
                .ok()
                .filter(|&id| id < u32::MAX)?;
            let start = u32::try_from(self.text.len()).ok()?;
            self.text.push_str(key);
            let end = u32::try_from(self.text.len()).ok()?;
            self.spans.push((start, end));
            *self.slots.get_mut(at)? = id + 1;
            Some(id)
        }

        /// Doubles the table (at least 1,024 slots) and places every key again.
        fn grow(&mut self) {
            let n = (self.slots.len() * 2).max(1_024);
            self.slots = alloc::vec![0; n];
            for id in 0..self.spans.len() {
                let Ok(id) = u32::try_from(id) else { break };
                let i = self.slot(self.text(id));
                if let Some(s) = self.slots.get_mut(i) {
                    *s = id + 1;
                }
            }
        }
    }

    /// The substring keys of one name text, each passed to the sink: the caller's fold and
    /// key rule (`teigeodata-names`), which the format does not depend on.
    pub type SubstringKeys<'a> = dyn Fn(&str, &mut dyn FnMut(&str)) + 'a;

    /// The SUBSTRING body of a NAMES section (D-199, D-213): every substring key `keys` gives
    /// for each name text, with `importance` (SEARCH's, one per row). The one code path of the
    /// forge, an embedded layer's build and a device that derives the index instead of
    /// downloading it, so all three write the same bytes. Keys of 255 bytes or more are left
    /// out, as in SEARCH.
    ///
    /// # Errors
    /// As [`encode_substring`]; a reader's error for a malformed name.
    pub fn substring_from_names(
        names: &crate::names::Names<'_>,
        importance: &[u8],
        keys: &SubstringKeys<'_>,
    ) -> Result<Vec<u8>, Error> {
        substring_with(names, importance, keys, &Extra::default())
    }

    /// [`substring_from_names`] for a NAMES part other than the base (ADR-0040): the SUBSTRING
    /// part of the same language, which borrows the base part's importance.
    ///
    /// # Errors
    /// As [`substring_from_names`].
    pub fn substring_part_from_names(
        names: &crate::names::Names<'_>,
        importance: &[u8],
        keys: &SubstringKeys<'_>,
    ) -> Result<Vec<u8>, Error> {
        let part = Extra {
            part: true,
            ..Extra::default()
        };
        substring_with(names, importance, keys, &part)
    }

    fn substring_with(
        names: &crate::names::Names<'_>,
        importance: &[u8],
        keys: &SubstringKeys<'_>,
        extra: &Extra<'_>,
    ) -> Result<Vec<u8>, Error> {
        // Each distinct key once, and a flat (key, row) pair per key a name gives: a few bytes a
        // pair instead of a set per key, then one sort (D-235).
        let mut ids = Interner::default();
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        let mut full = false;
        let mut buf = [0_u8; crate::names::TEXT_BUFFER];
        for row in 0..names.len() {
            let r = u32::try_from(row).map_err(|_| Error::Write("too many rows"))?;
            for name in names.names(row) {
                let text = name.text(&mut buf)?;
                keys(text, &mut |k: &str| {
                    if k.len() >= 255 {
                        return;
                    }
                    match ids.id(k) {
                        Some(id) => pairs.push((id, r)),
                        None => full = true,
                    }
                });
            }
        }
        if full {
            return Err(Error::Write("too many substring keys"));
        }
        // Renumber the keys in byte order, so the pairs sort into the automaton's order.
        let mut order: Vec<u32> = (0..ids.len())
            .filter_map(|i| u32::try_from(i).ok())
            .collect();
        order.sort_unstable_by(|&a, &b| ids.text(a).cmp(ids.text(b)));
        let mut rank = alloc::vec![0_u32; ids.len()];
        let mut texts: Vec<&str> = Vec::with_capacity(ids.len());
        for (i, &id) in order.iter().enumerate() {
            let text = ids.text(id);
            if text.chars().nth(SUBSTRING_MIN - 1).is_none() {
                return Err(Error::Write("a substring key has fewer than 3 code points"));
            }
            let i = u32::try_from(i).map_err(|_| Error::Write("too many substring keys"))?;
            *rank
                .get_mut(id as usize)
                .ok_or(Error::Write("a substring key is out of range"))? = i;
            texts.push(text);
        }
        for p in &mut pairs {
            p.0 = *rank
                .get(p.0 as usize)
                .ok_or(Error::Write("a substring key is out of range"))?;
        }
        drop((rank, order));
        pairs.sort_unstable();
        pairs.dedup();
        let groups = pairs.chunk_by(|a, b| a.0 == b.0);
        let entries = groups.map(|g| {
            let text = g
                .first()
                .and_then(|p| texts.get(p.0 as usize))
                .copied()
                .unwrap_or_default();
            let mut key = Vec::with_capacity(text.len() + 1);
            key.push(SUBSTRING);
            key.extend_from_slice(text.as_bytes());
            (key, g.iter().map(|p| p.1))
        });
        encode_entries(texts.len(), entries, importance, extra)
    }

    /// Encodes a SEARCH body: `keys` maps each tagged key (tag byte, then the folded text) to
    /// its rows; `importance` holds every row's importance (its length is the row count).
    ///
    /// # Errors
    /// [`Error::Write`] if a key is empty or too long, a list is empty, or a row is past the
    /// end.
    pub fn encode(
        keys: &BTreeMap<Vec<u8>, BTreeSet<u32>>,
        importance: &[u8],
    ) -> Result<Vec<u8>, Error> {
        encode_with(keys, importance, &Extra::default())
    }

    /// As [`encode`], with the optional columns.
    ///
    /// # Errors
    /// As [`encode`], and if `extra.units` is not one per row or names a unit type no rule
    /// names, or a rule does not encode.
    pub fn encode_with(
        keys: &BTreeMap<Vec<u8>, BTreeSet<u32>>,
        importance: &[u8],
        extra: &Extra<'_>,
    ) -> Result<Vec<u8>, Error> {
        encode_entries(
            keys.len(),
            keys.iter()
                .map(|(k, rows)| (k.as_slice(), rows.iter().copied())),
            importance,
            extra,
        )
    }

    /// The one encoder behind [`encode_with`] and [`substring_from_names`]: `n` entries of a
    /// tagged key and its rows, keys strictly ascending, each row once.
    fn encode_entries<K: AsRef<[u8]>, R: IntoIterator<Item = u32>>(
        n: usize,
        entries: impl Iterator<Item = (K, R)>,
        importance: &[u8],
        extra: &Extra<'_>,
    ) -> Result<Vec<u8>, Error> {
        if extra.part && (extra.rules.is_some() || !extra.units.is_empty()) {
            return Err(Error::Write(
                "a search part takes its rules and unit types from its base part",
            ));
        }
        let rules = extra.rules.map(super::rules::encode).transpose()?;
        let named = rules
            .as_deref()
            .map_or(Ok(0), |r| super::rules::Rules::new(r).check())?;
        if !extra.units.is_empty()
            && (extra.units.len() != importance.len() || extra.units.iter().any(|&u| u > named))
        {
            return Err(Error::Write(
                "the unit types are not one per row, or name a type no rule names",
            ));
        }
        let mut b = Builder::new();
        let mut starts = Vec::with_capacity(n + 1);
        let mut ids = Vec::new();
        let mut block_max = Vec::with_capacity(n.div_ceil(BLOCK));
        let mut best = 0_u8;
        let mut list: Vec<(u8, u32)> = Vec::new();
        let mut i = 0;
        for (key, rows) in entries {
            let key = key.as_ref();
            list.clear();
            for r in rows {
                let imp = *importance
                    .get(r as usize)
                    .ok_or(Error::Write("a search list names a row past the end"))?;
                list.push((imp, r));
            }
            if key.is_empty() || list.is_empty() {
                return Err(Error::Write("a search key or its list is empty"));
            }
            b.insert(key)?;
            starts.push(
                u32::try_from(ids.len()).map_err(|_| Error::Write("too many search entries"))?,
            );
            list.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            best = best.max(list.first().map_or(0, |e| e.0));
            ids.extend(list.iter().map(|e| e.1));
            i += 1;
            if i % BLOCK == 0 || i == n {
                block_max.push(u32::from(best));
                best = 0;
            }
        }
        if i != n {
            return Err(Error::Write("the search entries are not as many as stated"));
        }
        starts.push(u32::try_from(ids.len()).map_err(|_| Error::Write("too many search entries"))?);
        let (mut automaton, _) = b.finish()?;
        let len = u32::try_from(automaton.len())
            .map_err(|_| Error::Write("the search automaton is too large"))?;
        automaton.resize(automaton.len().next_multiple_of(4), 0);
        let imp: Vec<u32> = importance.iter().map(|&v| u32::from(v)).collect();
        let mut cols = alloc::vec![
            (LENGTH, encode_direct(&[len])),
            (STARTS, pack(&starts, Coding::Delta)),
            (IDS, encode_direct(&ids)),
            (BLOCK_MAX, encode_direct(&block_max)),
        ];
        if !extra.part {
            cols.push((IMPORTANCE, encode_direct(&imp)));
        }
        let units: Vec<u32> = extra.units.iter().map(|&u| u32::from(u)).collect();
        let units = (!units.is_empty()).then(|| encode_direct(&units));
        let mut with: Vec<(u16, Codec, &[u8])> = cols
            .iter()
            .map(|(id, b)| (*id, Codec::Bitpack, b.as_slice()))
            .collect();
        with.push((AUTOMATON, Codec::Bytes, automaton.as_slice()));
        if let Some(r) = &rules {
            with.push((RULES, Codec::Bytes, r.as_slice()));
        }
        if let Some(u) = &units {
            with.push((UNIT, Codec::Bitpack, u.as_slice()));
        }
        with.sort_by_key(|c| c.0);
        table::encode_with(&with)
            .map_err(|_| Error::Write("the search table could not be laid out"))
    }
}
