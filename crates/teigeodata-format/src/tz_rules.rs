//! `TZ_RULES` (kind 22): the rules of the timezones a pack's outlines name, from the same tzdb
//! release as the outlines (D-196, [ADR-0039], [research note]).
//!
//! The forge compiles tzdb with `zic -b slim`, parses each TZif file (RFC 9636) and writes its
//! rules as integer columns: transitions, local time types, abbreviations, and each POSIX TZ
//! footer already parsed into integers. This reader never parses text: it answers
//! [`Zone::offset_at`] (the offset at an instant) and [`Zone::local`] (the offsets a civil time
//! can have: one, a gap or an overlap) with binary searches and civil-date arithmetic. It does
//! not allocate and never panics; every reading that could overflow is checked, and data that
//! does not hold together is an [`Error`], never a wrong panic.
//!
//! A [column table](teigeodata_core::table); `item_count` is the number of zone names. A *set*
//! is one distinct rule set (zones with identical TZif content share it, as tzdb links do).
//!
//! | ID | Column | Values |
//! |---:|---|---|
//! | 1 | [`META`] | the base minute as two u32 halves of an i64 (low, high), then flags: bit 0 = built with `backzone` |
//! | 2 | [`NAME_TEXT`] | bytes: every zone name, concatenated, zero-padded |
//! | 3 | [`NAME_ENDS`] | per name: its end in [`NAME_TEXT`]; names strictly increasing in byte order |
//! | 4 | [`NAME_SET`] | per name: its set; sets are numbered by first use in name order |
//! | 5 | [`NAME_PLACE`] | per name: 1 + the PLACES row of its timezone area, or 0 |
//! | 6 | [`SET_TRANS_ENDS`] | per set: end of its transitions in columns 9–11 |
//! | 7 | [`SET_TYPE_ENDS`] | per set: end of its local time types in columns 12–13 (at least one each) |
//! | 8 | [`SET_FOOTER`] | per set: 1 + its footer, or 0 for an empty footer; footers numbered by first use |
//! | 9 | [`TRANS_MINUTE`] | per transition: minutes since the base minute; times strictly increase within a set |
//! | 10 | [`TRANS_SECOND`] | per transition: the second, 0–59 |
//! | 11 | [`TRANS_TYPE`] | per transition: the type it starts, an index within its set |
//! | 12 | [`TYPE_OFFSET`] | per type: UT offset + [`OFFSET_BIAS`] (offsets within ±[`MAX_OFFSET`]) |
//! | 13 | [`TYPE_FLAGS`] | per type: bit 0 `isdst`, the abbreviation's index << 1 |
//! | 14 | [`ABBR_TEXT`] | bytes: the distinct abbreviations in byte order, concatenated, zero-padded |
//! | 15 | [`ABBR_ENDS`] | per abbreviation: its end in [`ABBR_TEXT`] |
//! | 16 | [`FOOT_STD`] | per footer, two values: standard offset + [`OFFSET_BIAS`], its abbreviation |
//! | 17 | [`FOOT_DST`] | per footer, two values: 1 + the DST abbreviation (0: no DST), DST offset + [`OFFSET_BIAS`] (0 without DST) |
//! | 18 | [`FOOT_RULES`] | per footer, four values: start date, start time, end date, end time ([`RuleDate::code`], time + [`TIME_BIAS`]); zeros without DST |
//! | 19 | [`SET_TZIF`] | per set: its TZif version − 2 in bits 0–1 (0 for v2, 1 for v3, 2 for v4), and above them the type whose abbreviation `zic` laid out first (0 unless it moved its default type); kept so [`Zone::tzif`] is lossless |
//!
//! Instants are Unix seconds without leap seconds. Before a set's first transition its type 0
//! applies (tzdb's local mean time stub: [`LocalType::before_first`]); on or after its last
//! transition, or always when it has none, its footer applies; a set with neither reads type 0.
//! `isdst` is tzdb's flag: in tzdb's main form Europe/Dublin's winter time is the DST one.
//!
//! [ADR-0039]: https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/adr/ADR-0039-tz-rules.md
//! [research note]: https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/tz-rules.md

use teigeodata_core::bitpack::Packed;

use crate::{Error, Kind, Section, columns};

#[cfg(feature = "write")]
mod build;
#[cfg(feature = "alloc")]
mod render;

#[cfg(feature = "write")]
pub use build::{Dst, Footer, TypeRow, ZoneInput, ZoneRules, encode, parse_footer, parse_tzif};

/// Column: the base minute and flags.
pub const META: u16 = 1;
/// Column: the zone names' bytes.
pub const NAME_TEXT: u16 = 2;
/// Column: each name's end in [`NAME_TEXT`].
pub const NAME_ENDS: u16 = 3;
/// Column: each name's rule set.
pub const NAME_SET: u16 = 4;
/// Column: each name's timezone area, as 1 + its PLACES row.
pub const NAME_PLACE: u16 = 5;
/// Column: each set's end of transitions.
pub const SET_TRANS_ENDS: u16 = 6;
/// Column: each set's end of types.
pub const SET_TYPE_ENDS: u16 = 7;
/// Column: each set's footer, as 1 + its index.
pub const SET_FOOTER: u16 = 8;
/// Column: each transition's minute since the base.
pub const TRANS_MINUTE: u16 = 9;
/// Column: each transition's second.
pub const TRANS_SECOND: u16 = 10;
/// Column: each transition's type.
pub const TRANS_TYPE: u16 = 11;
/// Column: each type's biased offset.
pub const TYPE_OFFSET: u16 = 12;
/// Column: each type's `isdst` and abbreviation.
pub const TYPE_FLAGS: u16 = 13;
/// Column: the abbreviations' bytes.
pub const ABBR_TEXT: u16 = 14;
/// Column: each abbreviation's end.
pub const ABBR_ENDS: u16 = 15;
/// Column: each footer's standard offset and abbreviation.
pub const FOOT_STD: u16 = 16;
/// Column: each footer's DST abbreviation and offset.
pub const FOOT_DST: u16 = 17;
/// Column: each footer's DST start and end rules.
pub const FOOT_RULES: u16 = 18;
/// Column: each set's TZif version and abbreviation layout.
pub const SET_TZIF: u16 = 19;

const COLUMNS: [u16; 19] = [
    META,
    NAME_TEXT,
    NAME_ENDS,
    NAME_SET,
    NAME_PLACE,
    SET_TRANS_ENDS,
    SET_TYPE_ENDS,
    SET_FOOTER,
    TRANS_MINUTE,
    TRANS_SECOND,
    TRANS_TYPE,
    TYPE_OFFSET,
    TYPE_FLAGS,
    ABBR_TEXT,
    ABBR_ENDS,
    FOOT_STD,
    FOOT_DST,
    FOOT_RULES,
    SET_TZIF,
];

/// The largest UT offset magnitude, in seconds (RFC 9636 bounds `utoff` to −89,999..93,599).
pub const MAX_OFFSET: i32 = 93_599;
/// Added to an offset so it stores unsigned.
pub const OFFSET_BIAS: u32 = 93_600;
/// The largest rule time magnitude: 167 hours (RFC 9636's version 3 extension).
pub const MAX_RULE_TIME: i32 = 167 * 3_600;
/// Added to a rule time so it stores unsigned.
pub const TIME_BIAS: u32 = 604_800;
/// The earliest civil year a query takes.
pub const MIN_YEAR: i32 = -9_999;
/// The latest civil year a query takes.
pub const MAX_YEAR: i32 = 9_999;
/// The earliest instant a query takes: -9999-01-01T00:00:00Z.
pub const MIN_UNIX: i64 = -377_705_116_800;
/// The latest instant a query takes: 9999-12-31T23:59:59Z.
pub const MAX_UNIX: i64 = 253_402_300_799;

/// META flag: the rules were compiled with tzdb's `backzone`.
pub const FLAG_BACKZONE: u32 = 1;

const DAY: i64 = 86_400;
/// The most distinct offsets and changes [`Zone::local`] follows in its window; real zones
/// have at most three changes within two days.
const MAX_CHANGES: usize = 16;
/// Instants beyond this are refused before any arithmetic: far outside every query's range,
/// far inside i64.
const FAR: i64 = 1 << 50;

const fn fail(column: u16, problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::TZ_RULES.0,
        column,
        problem,
    }
}

/// The day of a POSIX TZ rule, as RFC 9636 and POSIX spell it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuleDate {
    /// `Jn`: day 1–365, February 29 never counted.
    Julian1(u16),
    /// `n`: day 0–365, February 29 counted in leap years.
    Julian0(u16),
    /// `Mm.w.d`: weekday `d` (0 Sunday) of week `w` (1–5, 5 the last) of month `m`.
    Month {
        /// 1–12.
        month: u8,
        /// 1–5.
        week: u8,
        /// 0–6, 0 Sunday.
        weekday: u8,
    },
}

impl RuleDate {
    /// Its stored code: the form in the low two bits (0 `M`, 1 `J`, 2 `n`) and above them the
    /// day (`J`, `n`) or `month × 100 + week × 10 + weekday` (`M`).
    #[must_use]
    pub fn code(self) -> u32 {
        match self {
            Self::Month {
                month,
                week,
                weekday,
            } => (u32::from(month) * 100 + u32::from(week) * 10 + u32::from(weekday)) << 2,
            Self::Julian1(n) => (u32::from(n) << 2) | 1,
            Self::Julian0(n) => (u32::from(n) << 2) | 2,
        }
    }

    /// The date of a stored code; `None` for a code outside the ranges.
    #[must_use]
    pub fn from_code(code: u32) -> Option<Self> {
        let v = code >> 2;
        let d = match code & 3 {
            0 => {
                let (month, week, weekday) = (v / 100, v / 10 % 10, v % 10);
                if !(1..=12).contains(&month) || !(1..=5).contains(&week) || weekday > 6 {
                    return None;
                }
                Self::Month {
                    month: u8::try_from(month).ok()?,
                    week: u8::try_from(week).ok()?,
                    weekday: u8::try_from(weekday).ok()?,
                }
            }
            1 if (1..=365).contains(&v) => Self::Julian1(u16::try_from(v).ok()?),
            2 if v <= 365 => Self::Julian0(u16::try_from(v).ok()?),
            _ => return None,
        };
        Some(d)
    }

    /// The day this date falls on in `year`, in days since 1970-01-01.
    fn day_in(self, year: i64) -> i64 {
        let jan1 = days_from_civil(year, 1, 1);
        match self {
            Self::Julian1(n) => {
                let n = i64::from(n);
                jan1 + n - 1 + i64::from(is_leap(year) && n >= 60)
            }
            Self::Julian0(n) => jan1 + i64::from(n),
            Self::Month {
                month,
                week,
                weekday,
            } => {
                let first = days_from_civil(year, u32::from(month), 1);
                let mut d = (i64::from(weekday) - weekday_of(first)).rem_euclid(7)
                    + 7 * (i64::from(week) - 1);
                let len = days_in_month(year, u32::from(month));
                while d >= len {
                    d -= 7;
                }
                first + d
            }
        }
    }
}

/// When a POSIX TZ rule switches: a day and the local time on it, in seconds (±167 hours).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RuleTime {
    /// The day.
    pub date: RuleDate,
    /// Seconds after that day's local midnight, −167 h to 167 h; POSIX's default is 2:00.
    pub time: i32,
}

/// The local time in force at an instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalType<'a> {
    /// The UT offset in seconds, east positive (Kathmandu +20,700).
    pub offset: i32,
    /// tzdb's abbreviation (`+0545`, `EDT`, `LMT`); not localised.
    pub abbr: &'a str,
    /// tzdb's `isdst` flag (Dublin's winter is DST in tzdb's main form).
    pub is_dst: bool,
    /// Whether the instant is before the zone's first transition: its local mean time stub,
    /// tzdb's guess for a time before standard time. False for a zone with no transitions.
    pub before_first: bool,
}

/// A civil (wall-clock) date and time, without a zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Civil {
    /// [`MIN_YEAR`] to [`MAX_YEAR`], proleptic Gregorian.
    pub year: i32,
    /// 1–12.
    pub month: u8,
    /// 1 to the month's length.
    pub day: u8,
    /// 0–23.
    pub hour: u8,
    /// 0–59.
    pub minute: u8,
    /// 0–59 (no leap seconds).
    pub second: u8,
}

impl Civil {
    /// Seconds since 1970-01-01T00:00:00 of this civil time read as UT.
    ///
    /// # Errors
    /// [`Error::Input`] if a field is out of range (February 30, hour 24, year 10000).
    pub fn seconds(self) -> Result<i64, Error> {
        let bad = Error::Input("a civil time out of range");
        if !(MIN_YEAR..=MAX_YEAR).contains(&self.year)
            || !(1..=12).contains(&self.month)
            || self.day == 0
            || i64::from(self.day) > days_in_month(i64::from(self.year), u32::from(self.month))
            || self.hour > 23
            || self.minute > 59
            || self.second > 59
        {
            return Err(bad);
        }
        let days = days_from_civil(
            i64::from(self.year),
            u32::from(self.month),
            u32::from(self.day),
        );
        Ok(days * DAY
            + i64::from(self.hour) * 3_600
            + i64::from(self.minute) * 60
            + i64::from(self.second))
    }

    /// The civil time of `seconds` since 1970-01-01T00:00:00 read as UT.
    ///
    /// # Errors
    /// [`Error::Input`] outside [`MIN_UNIX`]..=[`MAX_UNIX`].
    pub fn from_seconds(seconds: i64) -> Result<Self, Error> {
        if !(MIN_UNIX..=MAX_UNIX).contains(&seconds) {
            return Err(Error::Input("an instant out of range"));
        }
        let (days, secs) = (seconds.div_euclid(DAY), seconds.rem_euclid(DAY));
        let (year, month, day) = civil_from_days(days);
        let narrow = |v: i64| u8::try_from(v).map_err(|_| Error::Input("an instant out of range"));
        Ok(Self {
            year: i32::try_from(year).map_err(|_| Error::Input("an instant out of range"))?,
            month: narrow(month)?,
            day: narrow(day)?,
            hour: narrow(secs / 3_600)?,
            minute: narrow(secs / 60 % 60)?,
            second: narrow(secs % 60)?,
        })
    }
}

/// The offsets a civil time can have in a zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Local {
    /// Exactly one offset.
    Unique(i32),
    /// The civil time is skipped by a transition: `before` is the offset in force before it,
    /// `after` the one after (New York 2024-03-10 02:30: −18,000 then −14,400).
    Gap {
        /// The offset before the transition.
        before: i32,
        /// The offset after it.
        after: i32,
    },
    /// The civil time happens twice: `earlier` gives the earlier instant (the offset before
    /// the transition), `later` the later one.
    Overlap {
        /// The offset of the earlier instant.
        earlier: i32,
        /// The offset of the later instant.
        later: i32,
    },
}

/// A footer, read: the standard type and, with DST, its type and rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Foot {
    pub(crate) std: i32,
    pub(crate) std_abbr: u32,
    pub(crate) dst: Option<FootDst>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FootDst {
    pub(crate) offset: i32,
    pub(crate) abbr: u32,
    pub(crate) start: RuleTime,
    pub(crate) end: RuleTime,
}

impl Foot {
    /// Whether DST is in force at `t`: the latest switch at or before `t` among the years
    /// around it, a start winning a tie (so an all-year DST rule reads DST throughout).
    fn is_dst(&self, t: i64) -> Result<bool, Error> {
        let Some(d) = self.dst else {
            return Ok(false);
        };
        let y = self.year_of(t)?;
        let mut best: Option<(i64, bool)> = None;
        for year in y - 2..=y + 1 {
            for (at, start) in self.switches(&d, year) {
                if at <= t && best.is_none_or(|b| (at, start) > b) {
                    best = Some((at, start));
                }
            }
        }
        Ok(best.is_some_and(|b| b.1))
    }

    /// The first switch after `t`.
    fn next(&self, t: i64) -> Result<Option<i64>, Error> {
        let Some(d) = self.dst else {
            return Ok(None);
        };
        let y = self.year_of(t)?;
        let mut best: Option<i64> = None;
        for year in y - 1..=y + 2 {
            for (at, _) in self.switches(&d, year) {
                if at > t && best.is_none_or(|b| at < b) {
                    best = Some(at);
                }
            }
        }
        Ok(best)
    }

    /// The year of `t` in standard local time.
    fn year_of(&self, t: i64) -> Result<i64, Error> {
        if !(-FAR..=FAR).contains(&t) {
            return Err(Error::Input("an instant out of range"));
        }
        Ok(civil_from_days((t + i64::from(self.std)).div_euclid(DAY)).0)
    }

    /// The year's two switches as instants: DST starts at its rule's time in standard time,
    /// and ends at its rule's time in DST.
    fn switches(&self, d: &FootDst, year: i64) -> [(i64, bool); 2] {
        let local = |r: RuleTime| r.date.day_in(year) * DAY + i64::from(r.time);
        [
            (local(d.start) - i64::from(self.std), true),
            (local(d.end) - i64::from(d.offset), false),
        ]
    }
}

/// The `TZ_RULES` section of an open pack.
#[derive(Clone, Copy, Debug)]
pub struct TzRules<'a> {
    base: i64,
    flags: u32,
    name_text: &'a [u8],
    name_ends: Packed<'a>,
    name_set: Packed<'a>,
    name_place: Packed<'a>,
    set_trans_ends: Packed<'a>,
    set_type_ends: Packed<'a>,
    set_footer: Packed<'a>,
    set_tzif: Packed<'a>,
    trans_minute: Packed<'a>,
    trans_second: Packed<'a>,
    trans_type: Packed<'a>,
    type_offset: Packed<'a>,
    type_flags: Packed<'a>,
    abbr_text: &'a [u8],
    abbr_ends: Packed<'a>,
    foot_std: Packed<'a>,
    foot_dst: Packed<'a>,
    foot_rules: Packed<'a>,
}

/// One zone name and its rule set.
#[derive(Clone, Copy, Debug)]
pub struct Zone<'r> {
    rules: &'r TzRules<'r>,
    name: &'r str,
    set: usize,
    trans: (usize, usize),
    types: (usize, usize),
    foot: Option<Foot>,
    version: u8,
    #[cfg_attr(
        not(feature = "alloc"),
        expect(dead_code, reason = "read by the TZif renderer")
    )]
    first_abbr: usize,
}

fn get(col: &Packed<'_>, i: usize, id: u16) -> Result<u32, Error> {
    col.get(i).ok_or(fail(id, "an index past the column"))
}

fn offset_of(v: u32, id: u16) -> Result<i32, Error> {
    let o = i64::from(v) - i64::from(OFFSET_BIAS);
    if (1..=i64::from(OFFSET_BIAS) * 2 - 1).contains(&i64::from(v)) {
        i32::try_from(o).map_err(|_| fail(id, "an offset out of range"))
    } else {
        Err(fail(id, "an offset out of range"))
    }
}

fn rule_time(date: u32, time: u32) -> Result<RuleTime, Error> {
    let date = RuleDate::from_code(date).ok_or(fail(FOOT_RULES, "an invalid rule date"))?;
    let t = i64::from(time) - i64::from(TIME_BIAS);
    if t.abs() > i64::from(MAX_RULE_TIME) {
        return Err(fail(FOOT_RULES, "a rule time beyond 167 hours"));
    }
    Ok(RuleTime {
        date,
        time: i32::try_from(t).map_err(|_| fail(FOOT_RULES, "a rule time beyond 167 hours"))?,
    })
}

/// Bytes `[start, end)` of a zero-padded text column as ASCII text.
fn text_at(text: &[u8], start: u32, end: u32, id: u16) -> Result<&str, Error> {
    let (s, e) = (start as usize, end as usize);
    let b = text
        .get(s..e)
        .filter(|b| !b.is_empty())
        .ok_or(fail(id, "a text range outside its column"))?;
    if !b.is_ascii() {
        return Err(fail(id, "text that is not ASCII"));
    }
    core::str::from_utf8(b).map_err(|_| fail(id, "text that is not ASCII"))
}

impl<'a> TzRules<'a> {
    /// Opens the section: its frame and the lengths of its columns, in O(blocks).
    ///
    /// # Errors
    /// [`Error::Payload`] if the section is not a `TZ_RULES` table with consistent lengths.
    pub fn open(section: &Section<'a>) -> Result<Self, Error> {
        let kind = Kind::TZ_RULES;
        let table = columns::open(section, kind)?;
        if table.ids().any(|id| !COLUMNS.contains(&id)) {
            return Err(fail(0, "an unknown column"));
        }
        let n = usize::try_from(section.item_count())
            .ok()
            .filter(|&n| n > 0)
            .ok_or(fail(0, "no zone names"))?;
        let col = |id, len| columns::required(&table, kind, id, len);
        let bytes = |id| table.bytes(id).map_err(|e| columns::map(kind, e));
        let meta = col(META, Some(3))?;
        let name_text = bytes(NAME_TEXT)?;
        let name_ends = col(NAME_ENDS, Some(n))?;
        let name_set = col(NAME_SET, Some(n))?;
        let name_place = col(NAME_PLACE, Some(n))?;
        let set_trans_ends = col(SET_TRANS_ENDS, None)?;
        let s = set_trans_ends.len();
        if s == 0 || s > n {
            return Err(fail(
                SET_TRANS_ENDS,
                "no rule sets, or more sets than names",
            ));
        }
        let set_type_ends = col(SET_TYPE_ENDS, Some(s))?;
        let set_footer = col(SET_FOOTER, Some(s))?;
        let set_tzif = col(SET_TZIF, Some(s))?;
        let trans_minute = col(TRANS_MINUTE, None)?;
        let t = trans_minute.len();
        let trans_second = col(TRANS_SECOND, Some(t))?;
        let trans_type = col(TRANS_TYPE, Some(t))?;
        let type_offset = col(TYPE_OFFSET, None)?;
        let type_flags = col(TYPE_FLAGS, Some(type_offset.len()))?;
        let abbr_text = bytes(ABBR_TEXT)?;
        let abbr_ends = col(ABBR_ENDS, None)?;
        let foot_std = col(FOOT_STD, None)?;
        let f = foot_std.len() / 2;
        if foot_std.len() % 2 != 0 {
            return Err(fail(FOOT_STD, "not two values per footer"));
        }
        let foot_dst = col(FOOT_DST, Some(2 * f))?;
        let foot_rules = col(FOOT_RULES, Some(4 * f))?;
        let last = |c: &Packed<'_>, id| -> Result<usize, Error> {
            Ok(c.len()
                .checked_sub(1)
                .map(|i| get(c, i, id))
                .transpose()?
                .unwrap_or(0) as usize)
        };
        if last(&name_ends, NAME_ENDS)? > name_text.len()
            || last(&abbr_ends, ABBR_ENDS)? > abbr_text.len()
            || last(&set_trans_ends, SET_TRANS_ENDS)? != t
            || last(&set_type_ends, SET_TYPE_ENDS)? != type_offset.len()
        {
            return Err(fail(0, "an end column does not end at its column's length"));
        }
        let (lo, hi) = (get(&meta, 0, META)?, get(&meta, 1, META)?);
        let base = ((u64::from(hi) << 32) | u64::from(lo)).cast_signed();
        if !(-FAR / 60..=FAR / 60).contains(&base) {
            return Err(fail(META, "a base minute out of range"));
        }
        Ok(Self {
            base,
            flags: get(&meta, 2, META)?,
            name_text,
            name_ends,
            name_set,
            name_place,
            set_trans_ends,
            set_type_ends,
            set_footer,
            set_tzif,
            trans_minute,
            trans_second,
            trans_type,
            type_offset,
            type_flags,
            abbr_text,
            abbr_ends,
            foot_std,
            foot_dst,
            foot_rules,
        })
    }

    /// The number of zone names.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.name_ends.len()
    }

    /// Whether there are none (never, for an open section).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.name_ends.is_empty()
    }

    /// The number of distinct rule sets.
    #[must_use]
    pub const fn sets(&self) -> usize {
        self.set_trans_ends.len()
    }

    /// The number of stored transitions over all sets.
    #[must_use]
    pub const fn transitions(&self) -> usize {
        self.trans_minute.len()
    }

    /// Whether the rules were compiled with tzdb's `backzone` (pre-1970 data for zones tzdb
    /// otherwise links).
    #[must_use]
    pub const fn backzone(&self) -> bool {
        self.flags & FLAG_BACKZONE != 0
    }

    /// Name `i`.
    ///
    /// # Errors
    /// [`Error::Payload`] past the end or on a broken name column.
    pub fn name(&self, i: usize) -> Result<&'a str, Error> {
        let start = match i.checked_sub(1) {
            Some(p) => get(&self.name_ends, p, NAME_ENDS)?,
            None => 0,
        };
        let end = get(&self.name_ends, i, NAME_ENDS)?;
        text_at(self.name_text, start, end, NAME_TEXT)
    }

    /// The zone names, in byte order; stops at a broken name.
    pub fn names(&self) -> impl Iterator<Item = &'a str> + '_ {
        (0..self.len()).map_while(|i| self.name(i).ok())
    }

    /// The zone named `name` (exactly, as tzdb spells it): a binary search over the names.
    #[must_use]
    pub fn zone(&self, name: &str) -> Option<Zone<'_>> {
        let (mut lo, mut hi) = (0, self.len());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match self.name(mid).ok()?.as_bytes().cmp(name.as_bytes()) {
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => return self.zone_at(mid).ok(),
            }
        }
        None
    }

    /// The zone whose timezone area is PLACES row `row`: a scan of the names (a few hundred).
    #[must_use]
    pub fn zone_of_place(&self, row: u32) -> Option<Zone<'_>> {
        let want = row.checked_add(1)?;
        let i = self
            .name_place
            .values(0..self.len())
            .position(|v| v == want)?;
        self.zone_at(i).ok()
    }

    /// The PLACES row of name `i`'s timezone area, if it has one.
    #[must_use]
    pub fn place(&self, i: usize) -> Option<u32> {
        self.name_place.get(i)?.checked_sub(1)
    }

    /// Zone `i` (in name order).
    ///
    /// # Errors
    /// [`Error::Payload`] past the end or on broken set columns.
    pub fn zone_at(&self, i: usize) -> Result<Zone<'_>, Error> {
        let name = self.name(i)?;
        let set = get(&self.name_set, i, NAME_SET)? as usize;
        let range = |c: &Packed<'_>, id| -> Result<(usize, usize), Error> {
            let start = match set.checked_sub(1) {
                Some(p) => get(c, p, id)? as usize,
                None => 0,
            };
            let end = get(c, set, id)? as usize;
            if start > end {
                return Err(fail(id, "ends decrease"));
            }
            Ok((start, end))
        };
        let trans = range(&self.set_trans_ends, SET_TRANS_ENDS)?;
        let types = range(&self.set_type_ends, SET_TYPE_ENDS)?;
        if types.0 == types.1 {
            return Err(fail(SET_TYPE_ENDS, "a set with no types"));
        }
        let foot = match get(&self.set_footer, set, SET_FOOTER)?.checked_sub(1) {
            Some(f) => Some(self.foot(f as usize)?),
            None => None,
        };
        let tzif = get(&self.set_tzif, set, SET_TZIF)?;
        let version = u8::try_from(tzif & 3)
            .ok()
            .filter(|&v| v <= 2)
            .ok_or(fail(SET_TZIF, "an unknown TZif version"))?;
        let first_abbr = usize::try_from(tzif >> 2)
            .ok()
            .filter(|&k| k < types.1 - types.0)
            .ok_or(fail(SET_TZIF, "a first abbreviation past the set's types"))?;
        Ok(Zone {
            rules: self,
            name,
            set,
            trans,
            types,
            foot,
            version: version + 2,
            first_abbr,
        })
    }

    fn foot(&self, foot: usize) -> Result<Foot, Error> {
        let at = |k: usize, id| {
            foot.checked_mul(k)
                .ok_or(fail(id, "an index past the column"))
        };
        let (si, di, ri) = (at(2, FOOT_STD)?, at(2, FOOT_DST)?, at(4, FOOT_RULES)?);
        let std = offset_of(get(&self.foot_std, si, FOOT_STD)?, FOOT_STD)?;
        let std_abbr = get(&self.foot_std, si + 1, FOOT_STD)?;
        let rules = [0, 1, 2, 3].map(|k| get(&self.foot_rules, ri + k, FOOT_RULES));
        let [start_date, start_time, end_date, end_time] = rules;
        let dst = match get(&self.foot_dst, di, FOOT_DST)?.checked_sub(1) {
            None => {
                if get(&self.foot_dst, di + 1, FOOT_DST)? != 0
                    || [start_date?, start_time?, end_date?, end_time?] != [0; 4]
                {
                    return Err(fail(FOOT_DST, "DST fields on a footer without DST"));
                }
                None
            }
            Some(abbr) => Some(FootDst {
                offset: offset_of(get(&self.foot_dst, di + 1, FOOT_DST)?, FOOT_DST)?,
                abbr,
                start: rule_time(start_date?, start_time?)?,
                end: rule_time(end_date?, end_time?)?,
            }),
        };
        Ok(Foot { std, std_abbr, dst })
    }

    /// Abbreviation `i`.
    fn abbr(&self, i: u32) -> Result<&'a str, Error> {
        let i = i as usize;
        let start = match i.checked_sub(1) {
            Some(p) => get(&self.abbr_ends, p, ABBR_ENDS)?,
            None => 0,
        };
        text_at(
            self.abbr_text,
            start,
            get(&self.abbr_ends, i, ABBR_ENDS)?,
            ABBR_TEXT,
        )
    }

    /// Transition `i`'s instant.
    fn time(&self, i: usize) -> Result<i64, Error> {
        let m = i64::from(get(&self.trans_minute, i, TRANS_MINUTE)?);
        let s = get(&self.trans_second, i, TRANS_SECOND)?;
        if s > 59 {
            return Err(fail(TRANS_SECOND, "a second past 59"));
        }
        // |base| < 2^44 and m < 2^32, so neither step can overflow.
        Ok((self.base + m) * 60 + i64::from(s))
    }

    /// Checks every value and the relations between columns, in O(size): names strictly
    /// increasing and spelled as tzdb names are, sets and footers numbered by first use, ends
    /// increasing, transitions strictly increasing within a set, every index in range, every
    /// offset, abbreviation and rule within bounds, the base the smallest minute, and each
    /// footer agreeing with its set's last transition (RFC 9636).
    ///
    /// # Errors
    /// [`Error::Payload`] on the first bad value.
    pub fn check(&self) -> Result<(), Error> {
        if self.flags & !FLAG_BACKZONE != 0 {
            return Err(fail(META, "unknown flags"));
        }
        self.check_text()?;
        let (sets, feet) = (self.sets(), self.foot_std.len() / 2);
        let (mut next_set, mut next_foot) = (0_usize, 0_usize);
        let mut prev: Option<&str> = None;
        for i in 0..self.len() {
            let name = self.name(i)?;
            if prev.is_some_and(|p| p >= name) {
                return Err(fail(NAME_ENDS, "names not strictly increasing"));
            }
            prev = Some(name);
            let set = get(&self.name_set, i, NAME_SET)? as usize;
            if set > next_set || set >= sets {
                return Err(fail(NAME_SET, "sets not numbered by first use"));
            }
            if set == next_set {
                next_set += 1;
                let f = get(&self.set_footer, set, SET_FOOTER)? as usize;
                if f > next_foot + 1 || f > feet {
                    return Err(fail(SET_FOOTER, "footers not numbered by first use"));
                }
                if f == next_foot + 1 {
                    next_foot += 1;
                }
            }
        }
        if next_set != sets || next_foot != feet {
            return Err(fail(NAME_SET, "a set or footer no name uses"));
        }
        let mut min_minute = None;
        for set in 0..sets {
            let z = self.zone_of_set(set)?;
            z.check()?;
            if z.trans.0 < z.trans.1 {
                let m = get(&self.trans_minute, z.trans.0, TRANS_MINUTE)?;
                min_minute = Some(min_minute.map_or(m, |x: u32| x.min(m)));
            }
        }
        if min_minute.unwrap_or(0) != 0 || (min_minute.is_none() && self.base != 0) {
            return Err(fail(META, "the base is not the smallest minute"));
        }
        for i in 0..self.len() {
            let row = get(&self.name_place, i, NAME_PLACE)?;
            if row != 0 && self.name_place.values(0..i).any(|v| v == row) {
                return Err(fail(NAME_PLACE, "two names with one place"));
            }
        }
        Ok(())
    }

    /// The first name of set `set` as a zone (sets are numbered by first use).
    fn zone_of_set(&self, set: usize) -> Result<Zone<'_>, Error> {
        let i = self
            .name_set
            .values(0..self.len())
            .position(|s| s as usize == set)
            .ok_or(fail(NAME_SET, "a set no name uses"))?;
        self.zone_at(i)
    }

    /// The text columns: ends strictly increasing and ending at the text, zero padding, names
    /// in tzdb's characters, abbreviations distinct, sorted and in RFC 9636's characters.
    fn check_text(&self) -> Result<(), Error> {
        let ends = |c: &Packed<'_>, text: &[u8], id, tid| -> Result<(), Error> {
            let mut last = 0;
            for e in c.values(0..c.len()) {
                if e <= last {
                    return Err(fail(id, "ends not strictly increasing"));
                }
                last = e;
            }
            let pad = text.get(last as usize..).unwrap_or_default();
            if pad.len() >= 4 || pad.iter().any(|&b| b != 0) || last as usize > text.len() {
                return Err(fail(tid, "padding that is not zeros up to a multiple of 4"));
            }
            Ok(())
        };
        ends(&self.name_ends, self.name_text, NAME_ENDS, NAME_TEXT)?;
        ends(&self.abbr_ends, self.abbr_text, ABBR_ENDS, ABBR_TEXT)?;
        for i in 0..self.len() {
            if !is_zone_name(self.name(i)?) {
                return Err(fail(NAME_TEXT, "a name not in tzdb's characters"));
            }
        }
        let mut prev: Option<&str> = None;
        for i in 0..self.abbr_ends.len() {
            let a = self.abbr(u32::try_from(i).unwrap_or(u32::MAX))?;
            if !is_abbr(a) || prev.is_some_and(|p| p >= a) {
                return Err(fail(
                    ABBR_TEXT,
                    "abbreviations not distinct, sorted and valid",
                ));
            }
            prev = Some(a);
        }
        Ok(())
    }
}

/// Whether `name` is spelled as tzdb's theory allows: ASCII letters, digits, `.`, `_`, `-`,
/// `+` and `/` between components, no empty component, no `.` or `..` component.
#[must_use]
pub fn is_zone_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name.split('/').all(|c| {
            !c.is_empty()
                && c != "."
                && c != ".."
                && c.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))
        })
}

/// Whether `abbr` is an abbreviation RFC 9636 allows: 1 to 16 ASCII letters, digits, `+`, `-`.
#[must_use]
pub fn is_abbr(abbr: &str) -> bool {
    (1..=16).contains(&abbr.len())
        && abbr
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-')
}

impl<'r> Zone<'r> {
    /// Its name, as tzdb spells it (`Asia/Kathmandu`).
    #[must_use]
    pub const fn name(&self) -> &'r str {
        self.name
    }

    /// The number of its stored transitions (the footer's are not counted).
    #[must_use]
    pub const fn transition_count(&self) -> usize {
        self.trans.1 - self.trans.0
    }

    /// Its rule set's index: zones with the same index have identical rules.
    #[must_use]
    pub const fn set(&self) -> usize {
        self.set
    }

    /// The local time type in force at `unix_seconds`.
    ///
    /// # Errors
    /// [`Error::Input`] outside [`MIN_UNIX`]..=[`MAX_UNIX`]; [`Error::Payload`] on broken data.
    pub fn offset_at(&self, unix_seconds: i64) -> Result<LocalType<'r>, Error> {
        if !(MIN_UNIX..=MAX_UNIX).contains(&unix_seconds) {
            return Err(Error::Input("an instant out of range"));
        }
        self.offset_in(unix_seconds)
    }

    /// Type `k` of the set.
    fn local_type(&self, k: usize, before_first: bool) -> Result<LocalType<'r>, Error> {
        let i = self
            .types
            .0
            .checked_add(k)
            .filter(|&i| i < self.types.1)
            .ok_or(fail(TRANS_TYPE, "a type past its set"))?;
        let flags = get(&self.rules.type_flags, i, TYPE_FLAGS)?;
        Ok(LocalType {
            offset: offset_of(get(&self.rules.type_offset, i, TYPE_OFFSET)?, TYPE_OFFSET)?,
            abbr: self.rules.abbr(flags >> 1)?,
            is_dst: flags & 1 != 0,
            before_first,
        })
    }

    /// The footer's type at `t`.
    fn foot_type(&self, f: &Foot, t: i64) -> Result<LocalType<'r>, Error> {
        let (offset, abbr, is_dst) = match (f.dst, f.is_dst(t)?) {
            (Some(d), true) => (d.offset, d.abbr, true),
            _ => (f.std, f.std_abbr, false),
        };
        Ok(LocalType {
            offset,
            abbr: self.rules.abbr(abbr)?,
            is_dst,
            before_first: false,
        })
    }

    /// The number of stored transitions at or before `t`: a binary search.
    fn at_or_before(&self, t: i64) -> Result<usize, Error> {
        let (mut lo, mut hi) = self.trans;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.rules.time(mid)? <= t {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Ok(lo - self.trans.0)
    }

    /// [`Self::offset_at`] without the range check.
    fn offset_in(&self, t: i64) -> Result<LocalType<'r>, Error> {
        let count = self.transition_count();
        let k = self.at_or_before(t)?;
        if count == 0 || k == count {
            return match &self.foot {
                Some(f) => self.foot_type(f, t),
                None if count == 0 => self.local_type(0, false),
                None => {
                    let i = self.trans.1 - 1;
                    self.local_type(get(&self.rules.trans_type, i, TRANS_TYPE)? as usize, false)
                }
            };
        }
        match k.checked_sub(1) {
            None => self.local_type(0, true),
            Some(j) => {
                let i = self.trans.0 + j;
                self.local_type(get(&self.rules.trans_type, i, TRANS_TYPE)? as usize, false)
            }
        }
    }

    /// The first change of rules after `t`: a stored transition, or the footer's next switch
    /// on or after the last one.
    fn next_change(&self, t: i64) -> Result<Option<i64>, Error> {
        let count = self.transition_count();
        let k = self.at_or_before(t)?;
        if k < count {
            return self.rules.time(self.trans.0 + k).map(Some);
        }
        match &self.foot {
            Some(f) => f.next(t),
            None => Ok(None),
        }
    }

    /// The offsets civil time `civil` can have here: one, a gap (no instant has this civil
    /// time) or an overlap (two do).
    ///
    /// # Errors
    /// [`Error::Input`] for an invalid civil time or one outside [`MIN_YEAR`]..=[`MAX_YEAR`];
    /// [`Error::Payload`] on broken data.
    pub fn local(&self, civil: Civil) -> Result<Local, Error> {
        let l = civil.seconds()?;
        // An offset `o` is valid when the offset at `l - o` is `o`; |o| ≤ MAX_OFFSET, so every
        // candidate is in force somewhere in this window of instants.
        let (lo, hi) = (l - i64::from(MAX_OFFSET), l + i64::from(MAX_OFFSET));
        let mut seen = [0_i32; MAX_CHANGES];
        let mut changes = [(0_i64, 0_i32, 0_i32); MAX_CHANGES];
        let (mut n_seen, mut n_changes) = (0, 0);
        let mut cur = self.offset_in(lo)?.offset;
        let mut add = |o: i32, seen: &mut [i32; MAX_CHANGES]| {
            if !seen.iter().take(n_seen).any(|&s| s == o) && n_seen < MAX_CHANGES {
                if let Some(slot) = seen.get_mut(n_seen) {
                    *slot = o;
                }
                n_seen += 1;
            }
        };
        add(cur, &mut seen);
        let mut t = lo;
        for _ in 0..MAX_CHANGES {
            let Some(next) = self.next_change(t)?.filter(|&n| n <= hi) else {
                break;
            };
            let o = self.offset_in(next)?.offset;
            if o != cur {
                if let Some(slot) = changes.get_mut(n_changes) {
                    *slot = (next, cur, o);
                    n_changes += 1;
                }
                add(o, &mut seen);
                cur = o;
            }
            t = next;
        }
        let mut valid = [0_i32; MAX_CHANGES];
        let mut n_valid = 0;
        for &o in seen.iter().take(n_seen) {
            if self.offset_in(l - i64::from(o))?.offset == o {
                if let Some(slot) = valid.get_mut(n_valid) {
                    *slot = o;
                }
                n_valid += 1;
            }
        }
        let valid = valid.get(..n_valid).unwrap_or_default();
        match valid {
            [o] => Ok(Local::Unique(*o)),
            [] => {
                let changes = changes.get(..n_changes).unwrap_or_default();
                changes
                    .iter()
                    .find(|&&(at, a, b)| b > a && at + i64::from(a) <= l && l < at + i64::from(b))
                    .or_else(|| changes.iter().find(|c| c.2 > c.1))
                    .map(|&(_, before, after)| Local::Gap { before, after })
                    .ok_or(fail(0, "a civil time with no offset and no gap"))
            }
            _ => {
                let (mut earlier, mut later) = (i32::MIN, i32::MAX);
                for &o in valid {
                    earlier = earlier.max(o);
                    later = later.min(o);
                }
                Ok(Local::Overlap { earlier, later })
            }
        }
    }

    /// The stored transitions with the type each starts, in time order (the footer's are not
    /// listed); ends early at broken data.
    pub fn transitions(&self) -> impl Iterator<Item = (i64, LocalType<'r>)> + '_ {
        (self.trans.0..self.trans.1).map_while(move |i| {
            let t = self.rules.time(i).ok()?;
            let k = get(&self.rules.trans_type, i, TRANS_TYPE).ok()?;
            Some((t, self.local_type(k as usize, false).ok()?))
        })
    }

    /// Checks this zone's set: transitions strictly increasing with types in range, types and
    /// footer within bounds, and the footer agreeing with the last transition.
    fn check(&self) -> Result<(), Error> {
        let ntypes = self.types.1 - self.types.0;
        if ntypes > 256 {
            return Err(fail(SET_TYPE_ENDS, "more than 256 types in a set"));
        }
        for k in 0..ntypes {
            let flags = get(&self.rules.type_flags, self.types.0 + k, TYPE_FLAGS)?;
            if flags >> 1 >= u32::try_from(self.rules.abbr_ends.len()).unwrap_or(u32::MAX) {
                return Err(fail(TYPE_FLAGS, "an abbreviation past the table"));
            }
            self.local_type(k, false)?;
        }
        let mut last: Option<i64> = None;
        for i in self.trans.0..self.trans.1 {
            let t = self.rules.time(i)?;
            if last.is_some_and(|l| t <= l) {
                return Err(fail(TRANS_MINUTE, "transitions not strictly increasing"));
            }
            last = Some(t);
            if get(&self.rules.trans_type, i, TRANS_TYPE)? as usize >= ntypes {
                return Err(fail(TRANS_TYPE, "a type past its set"));
            }
        }
        if let Some(f) = &self.foot {
            let n = u32::try_from(self.rules.abbr_ends.len()).unwrap_or(u32::MAX);
            if f.std_abbr >= n || f.dst.is_some_and(|d| d.abbr >= n) {
                return Err(fail(FOOT_STD, "an abbreviation past the table"));
            }
            if let Some(t) = last {
                let i = self.trans.1 - 1;
                let want =
                    self.local_type(get(&self.rules.trans_type, i, TRANS_TYPE)? as usize, false)?;
                let got = self.foot_type(f, t)?;
                if (got.offset, got.abbr, got.is_dst) != (want.offset, want.abbr, want.is_dst) {
                    return Err(fail(
                        SET_FOOTER,
                        "a footer that disagrees with its set's last transition",
                    ));
                }
            }
        }
        Ok(())
    }

    /// The TZif version it was compiled as (2, 3 or 4).
    #[must_use]
    pub const fn version(&self) -> u8 {
        self.version
    }
}

/// What the TZif renderer reads.
#[cfg(feature = "alloc")]
impl<'r> Zone<'r> {
    /// The footer as stored: what the TZif footer string is rebuilt from.
    pub(crate) const fn foot(&self) -> Option<&Foot> {
        self.foot.as_ref()
    }

    /// The type whose abbreviation `zic` laid out first.
    pub(crate) const fn first_abbr(&self) -> usize {
        self.first_abbr
    }

    /// An abbreviation of the section's table.
    pub(crate) fn abbr_text(&self, i: u32) -> Result<&'r str, Error> {
        self.rules.abbr(i)
    }

    /// The set's types in order.
    pub(crate) fn types(&self) -> impl Iterator<Item = Result<LocalType<'r>, Error>> + '_ {
        (0..self.types.1 - self.types.0).map(|k| self.local_type(k, false))
    }

    /// The stored transitions as (instant, type index).
    pub(crate) fn raw_transitions(&self) -> impl Iterator<Item = Result<(i64, u32), Error>> + '_ {
        (self.trans.0..self.trans.1).map(|i| {
            Ok((
                self.rules.time(i)?,
                get(&self.rules.trans_type, i, TRANS_TYPE)?,
            ))
        })
    }
}

/// Whether `y` is a Gregorian leap year.
const fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

const fn days_in_month(y: i64, m: u32) -> i64 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Hinnant's `days_from_civil`). Callers
/// keep |y| within a few million years, so nothing overflows.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of `z` days since 1970-01-01 (Hinnant's `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// The weekday of `z` days since 1970-01-01, 0 Sunday (1970-01-01 was a Thursday).
const fn weekday_of(z: i64) -> i64 {
    (z + 4).rem_euclid(7)
}

#[cfg(test)]
mod tests {
    use super::{Civil, RuleDate, civil_from_days, days_from_civil, weekday_of};

    #[test]
    fn civil_days_round_trip_and_weekdays() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(weekday_of(days_from_civil(2024, 3, 10)), 0, "a Sunday");
        for z in (-4_000_000..4_000_000).step_by(997) {
            let (y, m, d) = civil_from_days(z);
            let (m, d) = (u32::try_from(m).unwrap(), u32::try_from(d).unwrap());
            assert_eq!(days_from_civil(y, m, d), z);
        }
        let c = Civil {
            year: 2024,
            month: 2,
            day: 29,
            hour: 23,
            minute: 59,
            second: 59,
        };
        assert_eq!(Civil::from_seconds(c.seconds().unwrap()).unwrap(), c);
        assert!(Civil { day: 30, ..c }.seconds().is_err());
        assert!(Civil { hour: 24, ..c }.seconds().is_err());
        assert!(Civil { year: 10_000, ..c }.seconds().is_err());
        assert!(Civil::from_seconds(super::MAX_UNIX + 1).is_err());
        assert_eq!(super::MIN_UNIX, days_from_civil(-9_999, 1, 1) * 86_400);
        assert_eq!(
            super::MAX_UNIX,
            days_from_civil(9_999, 12, 31) * 86_400 + 86_399
        );
    }

    #[test]
    fn rule_dates_code_and_fall_on_their_days() {
        for d in [
            RuleDate::Julian1(1),
            RuleDate::Julian1(365),
            RuleDate::Julian0(0),
            RuleDate::Julian0(365),
            RuleDate::Month {
                month: 12,
                week: 5,
                weekday: 6,
            },
        ] {
            assert_eq!(RuleDate::from_code(d.code()), Some(d));
        }
        // 2 is J0 day 0, a valid date; 0 (no month), 1 (J day 0) and 3 (no form) are not.
        for bad in [0, 1, 3, (366 << 2) | 1, (366 << 2) | 2, 1305 << 2, 117 << 2] {
            assert_eq!(RuleDate::from_code(bad), None, "{bad}");
        }
        // M3.2.0 in 2024: the second Sunday of March is the 10th.
        let m = RuleDate::Month {
            month: 3,
            week: 2,
            weekday: 0,
        };
        assert_eq!(m.day_in(2024), days_from_civil(2024, 3, 10));
        // M10.5.0: the last Sunday of October 2024 is the 27th.
        let last = RuleDate::Month {
            month: 10,
            week: 5,
            weekday: 0,
        };
        assert_eq!(last.day_in(2024), days_from_civil(2024, 10, 27));
        // J60 is March 1 in any year; day 59 (zero-based) is Feb 29 in a leap year.
        assert_eq!(
            RuleDate::Julian1(60).day_in(2024),
            days_from_civil(2024, 3, 1)
        );
        assert_eq!(
            RuleDate::Julian1(60).day_in(2023),
            days_from_civil(2023, 3, 1)
        );
        assert_eq!(
            RuleDate::Julian0(59).day_in(2024),
            days_from_civil(2024, 2, 29)
        );
        assert_eq!(
            RuleDate::Julian0(59).day_in(2023),
            days_from_civil(2023, 3, 1)
        );
    }
}
