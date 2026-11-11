//! The build side of `TZ_RULES` (feature `write`): a strict TZif parser (RFC 9636), a POSIX TZ
//! footer parser, and the encoder. The forge parses `zic`'s output with these, so the reader
//! never sees text; fuzzing reaches them through the format crate's harnesses.

use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;

use teigeodata_core::bitpack::{encode_auto, encode_direct};
use teigeodata_core::table::{Codec, encode_with};

use super::{
    ABBR_ENDS, ABBR_TEXT, FLAG_BACKZONE, FOOT_DST, FOOT_RULES, FOOT_STD, MAX_OFFSET, MAX_RULE_TIME,
    META, NAME_ENDS, NAME_PLACE, NAME_SET, NAME_TEXT, OFFSET_BIAS, RuleDate, RuleTime, SET_FOOTER,
    SET_TRANS_ENDS, SET_TYPE_ENDS, SET_TZIF, TIME_BIAS, TRANS_MINUTE, TRANS_SECOND, TRANS_TYPE,
    TYPE_FLAGS, TYPE_OFFSET, TzRules, is_abbr, is_zone_name,
};
use crate::{Error, Kind, Section};

/// A local time type of a TZif file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TypeRow {
    /// UT offset in seconds, east positive.
    pub offset: i32,
    /// tzdb's `isdst`.
    pub is_dst: bool,
    /// The abbreviation.
    pub abbr: String,
}

/// A footer's DST part.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Dst {
    /// UT offset in DST, east positive.
    pub offset: i32,
    /// The DST abbreviation.
    pub abbr: String,
    /// When DST starts, in standard local time.
    pub start: RuleTime,
    /// When DST ends, in DST local time.
    pub end: RuleTime,
}

/// A TZif footer: a POSIX TZ string, parsed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Footer {
    /// Standard UT offset, east positive (the string's sign is the other way).
    pub std_offset: i32,
    /// The standard abbreviation.
    pub std_abbr: String,
    /// DST, if the zone has it.
    pub dst: Option<Dst>,
}

/// One TZif file's rules, as the section stores them.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ZoneRules {
    /// The TZif version: 2, 3 or 4.
    pub version: u8,
    /// Each transition's instant (Unix seconds) and the type it starts.
    pub transitions: Vec<(i64, u8)>,
    /// The local time types; type 0 applies before the first transition.
    pub types: Vec<TypeRow>,
    /// The footer; `None` for an empty one.
    pub footer: Option<Footer>,
    /// The type whose abbreviation `zic` laid out first in the file's characters: 0, unless
    /// `zic` swapped its default type into place 0. [`parse_tzif`] finds it, so the file can be
    /// rebuilt byte for byte; 0 when no choice reproduces the file's layout.
    pub first_abbr: u8,
}

/// A zone to encode: its name, its rules and its timezone area's PLACES row.
#[derive(Clone, Copy, Debug)]
pub struct ZoneInput<'z> {
    /// The tzdb name (`Asia/Kathmandu`).
    pub name: &'z str,
    /// Its rules.
    pub rules: &'z ZoneRules,
    /// The PLACES row of its timezone area, if the pack has one.
    pub place: Option<u32>,
}

const fn bad(problem: &'static str) -> Error {
    Error::Input(problem)
}

/// A cursor over untrusted bytes: every read is bounds-checked.
struct Bytes<'b> {
    b: &'b [u8],
    at: usize,
}

impl<'b> Bytes<'b> {
    fn take(&mut self, n: usize) -> Result<&'b [u8], Error> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(bad("TZif: a count too large"))?;
        let out = self.b.get(self.at..end).ok_or(bad("TZif: truncated"))?;
        self.at = end;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        self.take(N)?.try_into().map_err(|_| bad("TZif: truncated"))
    }

    fn u32(&mut self) -> Result<usize, Error> {
        usize::try_from(u32::from_be_bytes(self.array()?))
            .map_err(|_| bad("TZif: a count too large"))
    }
}

/// TZif header counts.
struct Counts {
    isut: usize,
    isstd: usize,
    leap: usize,
    time: usize,
    types: usize,
    chars: usize,
}

/// A header: the magic, the version (0 for version 1, else 2–4), 15 zero bytes, six counts.
fn header(r: &mut Bytes<'_>) -> Result<(u8, Counts), Error> {
    if r.take(4)? != b"TZif" {
        return Err(bad("TZif: no magic"));
    }
    let version = match r.array::<1>()?[0] {
        0 => 0,
        b'2' => 2,
        b'3' => 3,
        b'4' => 4,
        _ => return Err(bad("TZif: an unknown version")),
    };
    if r.take(15)?.iter().any(|&b| b != 0) {
        return Err(bad("TZif: reserved bytes that are not zero"));
    }
    Ok((
        version,
        Counts {
            isut: r.u32()?,
            isstd: r.u32()?,
            leap: r.u32()?,
            time: r.u32()?,
            types: r.u32()?,
            chars: r.u32()?,
        },
    ))
}

/// The bytes of the version 1 block after its header.
fn v1_len(v1: &Counts) -> Result<usize, Error> {
    let mul = |a: usize, b: usize| a.checked_mul(b).ok_or(bad("TZif: a count too large"));
    [
        mul(v1.time, 5)?,
        mul(v1.types, 6)?,
        v1.chars,
        mul(v1.leap, 8)?,
        v1.isstd,
        v1.isut,
    ]
    .into_iter()
    .try_fold(0_usize, usize::checked_add)
    .ok_or(bad("TZif: a count too large"))
}

/// Parses a TZif file of version 2 or later strictly: every count checked against the bytes,
/// transitions strictly increasing, offsets within ±[`MAX_OFFSET`], abbreviations
/// NUL-terminated and of RFC 9636's characters, a footer it can parse and nothing after it.
/// The version 1 block is skipped; standard/wall and UT/local indicators are checked and
/// dropped (they only serve readers without a footer). Refuses version 1 files and leap
/// seconds (the `right/` zones).
///
/// # Errors
/// [`Error::Input`] naming the first problem.
pub fn parse_tzif(bytes: &[u8]) -> Result<ZoneRules, Error> {
    let mut r = Bytes { b: bytes, at: 0 };
    let (version, v1) = header(&mut r)?;
    if version == 0 {
        return Err(bad("TZif: version 1 has no footer and 32-bit times"));
    }
    r.take(v1_len(&v1)?)?;
    let (v2, n) = header(&mut r)?;
    if v2 != version {
        return Err(bad("TZif: the two headers differ in version"));
    }
    if n.leap != 0 {
        return Err(bad("TZif: leap seconds (a right/ zone)"));
    }
    if n.types == 0 || n.types > 256 || n.chars == 0 {
        return Err(bad("TZif: no types, more than 256, or no abbreviations"));
    }
    if (n.isstd != 0 && n.isstd != n.types) || (n.isut != 0 && n.isut != n.types) {
        return Err(bad(
            "TZif: indicator counts that are neither 0 nor the types",
        ));
    }
    let mut times = Vec::with_capacity(n.time.min(bytes.len() / 8));
    for _ in 0..n.time {
        let t = i64::from_be_bytes(r.array()?);
        if times.last().is_some_and(|&l| t <= l) {
            return Err(bad("TZif: transitions not strictly increasing"));
        }
        times.push(t);
    }
    let idx = r.take(n.time)?;
    if idx.iter().any(|&i| usize::from(i) >= n.types) {
        return Err(bad("TZif: a transition type past the types"));
    }
    let mut raw = Vec::with_capacity(n.types);
    let mut raw_abbr = Vec::with_capacity(n.types);
    for _ in 0..n.types {
        let [o0, o1, o2, o3, isdst, ab] = r.array::<6>()?;
        let off = i32::from_be_bytes([o0, o1, o2, o3]);
        if off.checked_abs().is_none_or(|o| o > MAX_OFFSET) || isdst > 1 {
            return Err(bad("TZif: an offset out of range, or isdst not 0 or 1"));
        }
        raw.push((off, isdst == 1, usize::from(ab)));
        raw_abbr.push(usize::from(ab));
    }
    let chars = r.take(n.chars)?;
    let mut types = Vec::with_capacity(n.types);
    for (offset, is_dst, ab) in raw {
        let tail = chars
            .get(ab..)
            .ok_or(bad("TZif: an abbreviation past the characters"))?;
        let end = tail
            .iter()
            .position(|&b| b == 0)
            .ok_or(bad("TZif: an abbreviation without its NUL"))?;
        let abbr = core::str::from_utf8(tail.get(..end).unwrap_or_default())
            .ok()
            .filter(|a| is_abbr(a))
            .ok_or(bad("TZif: an abbreviation not of RFC 9636's characters"))?;
        types.push(TypeRow {
            offset,
            is_dst,
            abbr: abbr.into(),
        });
    }
    let isstd = r.take(n.isstd)?;
    let isut = r.take(n.isut)?;
    if isstd.iter().chain(isut).any(|&b| b > 1)
        || isut
            .iter()
            .zip(isstd)
            .any(|(&ut, &std)| ut == 1 && std == 0)
    {
        return Err(bad("TZif: indicators not 0 or 1, or UT without standard"));
    }
    let rest = bytes.get(r.at..).unwrap_or_default();
    let footer = rest
        .strip_prefix(b"\n")
        .and_then(|f| f.strip_suffix(b"\n"))
        .filter(|f| !f.contains(&b'\n'))
        .and_then(|f| core::str::from_utf8(f).ok())
        .ok_or(bad("TZif: a footer that is not one line between newlines"))?;
    let transitions = times.into_iter().zip(idx.iter().copied()).collect();
    let first_abbr = layout(&types, chars, &raw_abbr);
    Ok(ZoneRules {
        version,
        transitions,
        types,
        footer: parse_footer(footer)?,
        first_abbr,
    })
}

/// The type whose abbreviation `zic` added first, such that `zic`'s layout of `types` gives
/// exactly the file's characters and indices; 0 if none does.
fn layout(types: &[TypeRow], chars: &[u8], at: &[usize]) -> u8 {
    for first in 0..types.len() {
        let order = (0..types.len()).map(|k| match k {
            0 => first,
            k if k == first => 0,
            k => k,
        });
        let mut chs = Vec::new();
        for k in order {
            if let Some(t) = types.get(k) {
                super::render::add_abbr(&mut chs, t.abbr.as_bytes());
            }
        }
        if chs == chars
            && types
                .iter()
                .zip(at)
                .all(|(t, &a)| super::render::add_abbr(&mut chs, t.abbr.as_bytes()) == a)
        {
            return u8::try_from(first).unwrap_or(0);
        }
    }
    0
}

/// A cursor over a footer string.
struct Text<'s> {
    b: &'s [u8],
    at: usize,
}

impl Text<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.at).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        let hit = self.peek() == Some(c);
        if hit {
            self.at += 1;
        }
        hit
    }

    /// An abbreviation: `<…>` of letters, digits, `+` and `-`, or bare letters; 3 or more.
    fn name(&mut self) -> Result<String, Error> {
        let start = self.at;
        let quoted = self.eat(b'<');
        let ok = |b: u8| {
            if quoted {
                b.is_ascii_alphanumeric() || b == b'+' || b == b'-'
            } else {
                b.is_ascii_alphabetic()
            }
        };
        let from = self.at;
        while self.peek().is_some_and(ok) {
            self.at += 1;
        }
        let text = self.b.get(from..self.at).unwrap_or_default();
        if quoted && !self.eat(b'>') || text.len() < 3 || text.len() > 16 {
            self.at = start;
            return Err(bad(
                "TZ string: an abbreviation of fewer than 3 characters or unclosed",
            ));
        }
        Ok(text.iter().map(|&b| char::from(b)).collect())
    }

    /// Digits, at most `max` of them.
    fn number(&mut self, max: usize) -> Result<i64, Error> {
        let from = self.at;
        let mut v: i64 = 0;
        while let Some(d) = self.peek().filter(u8::is_ascii_digit) {
            if self.at - from == max {
                return Err(bad("TZ string: a number with too many digits"));
            }
            v = v * 10 + i64::from(d - b'0');
            self.at += 1;
        }
        if self.at == from {
            return Err(bad("TZ string: a number expected"));
        }
        Ok(v)
    }

    /// `[+-]hh[:mm[:ss]]` with at most `hours` hour digits, in seconds.
    fn hms(&mut self, hours: usize) -> Result<i64, Error> {
        let sign = if self.eat(b'-') {
            -1
        } else {
            self.eat(b'+');
            1
        };
        let mut v = self.number(hours)? * 3_600;
        for unit in [60, 1] {
            if !self.eat(b':') {
                break;
            }
            let start = self.at;
            let n = self.number(2)?;
            if self.at - start != 2 || n > 59 {
                return Err(bad("TZ string: minutes or seconds not two digits below 60"));
            }
            v += n * unit;
        }
        Ok(sign * v)
    }

    /// A UT offset in POSIX's sign (west positive), hours 0–24, as east positive seconds.
    fn offset(&mut self) -> Result<i32, Error> {
        let posix = self.hms(2)?;
        if posix.abs() > 24 * 3_600 + 59 * 60 + 59 {
            return Err(bad("TZ string: an offset beyond 24:59:59"));
        }
        i32::try_from(-posix).map_err(|_| bad("TZ string: an offset beyond 24:59:59"))
    }

    /// `date[/time]`: `Jn`, `n` or `Mm.w.d`, and a time within ±167 hours (default 2:00).
    fn rule(&mut self) -> Result<RuleTime, Error> {
        let date = if self.eat(b'J') {
            let n = self.number(3)?;
            RuleDate::Julian1(
                u16::try_from(n)
                    .ok()
                    .filter(|n| (1..=365).contains(n))
                    .ok_or(bad("TZ string: Jn outside 1..365"))?,
            )
        } else if self.eat(b'M') {
            let m = self.number(2)?;
            let w = if self.eat(b'.') { self.number(1)? } else { -1 };
            let d = if self.eat(b'.') { self.number(1)? } else { -1 };
            if !(1..=12).contains(&m) || !(1..=5).contains(&w) || !(0..=6).contains(&d) {
                return Err(bad("TZ string: Mm.w.d out of range"));
            }
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "checked to 1..=12, 1..=5 and 0..=6 above"
            )]
            RuleDate::Month {
                month: m as u8,
                week: w as u8,
                weekday: d as u8,
            }
        } else {
            let n = self.number(3)?;
            RuleDate::Julian0(
                u16::try_from(n)
                    .ok()
                    .filter(|&n| n <= 365)
                    .ok_or(bad("TZ string: n outside 0..365"))?,
            )
        };
        let time = if self.eat(b'/') { self.hms(3)? } else { 7_200 };
        if time.abs() > i64::from(MAX_RULE_TIME) {
            return Err(bad("TZ string: a rule time beyond 167 hours"));
        }
        Ok(RuleTime {
            date,
            time: i32::try_from(time)
                .map_err(|_| bad("TZ string: a rule time beyond 167 hours"))?,
        })
    }
}

/// Parses a TZif footer (a POSIX TZ string with RFC 9636's extensions); `None` for an empty
/// one. Strict: a zone with DST must state its rules (zic always does), and nothing may follow.
///
/// # Errors
/// [`Error::Input`] naming the first problem.
pub fn parse_footer(s: &str) -> Result<Option<Footer>, Error> {
    if s.is_empty() {
        return Ok(None);
    }
    let mut p = Text {
        b: s.as_bytes(),
        at: 0,
    };
    let std_abbr = p.name()?;
    let std_offset = p.offset()?;
    let dst = if p.peek().is_none() {
        None
    } else {
        let abbr = p.name()?;
        let offset = if p.peek() == Some(b',') {
            std_offset
                .checked_add(3_600)
                .ok_or(bad("TZ string: an offset beyond 24:59:59"))?
        } else {
            p.offset()?
        };
        if !p.eat(b',') {
            return Err(bad("TZ string: DST without its rules"));
        }
        let start = p.rule()?;
        if !p.eat(b',') {
            return Err(bad("TZ string: one DST rule, two expected"));
        }
        let end = p.rule()?;
        Some(Dst {
            offset,
            abbr,
            start,
            end,
        })
    };
    if p.peek().is_some() {
        return Err(bad("TZ string: characters after the end"));
    }
    if dst.as_ref().is_some_and(|d| d.offset.abs() > MAX_OFFSET) || std_offset.abs() > MAX_OFFSET {
        return Err(bad("TZ string: an offset out of range"));
    }
    Ok(Some(Footer {
        std_offset,
        std_abbr,
        dst,
    }))
}

const fn werr(problem: &'static str) -> Error {
    Error::Write(problem)
}

fn biased(offset: i32) -> Result<u32, Error> {
    if offset.abs() > MAX_OFFSET {
        return Err(werr("a UT offset beyond 93,599 seconds"));
    }
    u32::try_from(i64::from(offset) + i64::from(OFFSET_BIAS)).map_err(|_| werr("an offset"))
}

fn rule_values(r: RuleTime) -> Result<[u32; 2], Error> {
    if r.time.abs() > MAX_RULE_TIME || RuleDate::from_code(r.date.code()) != Some(r.date) {
        return Err(werr("a rule date or time out of range"));
    }
    let t = u32::try_from(i64::from(r.time) + i64::from(TIME_BIAS)).map_err(|_| werr("a rule"))?;
    Ok([r.date.code(), t])
}

fn index(i: usize) -> Result<u32, Error> {
    u32::try_from(i).map_err(|_| werr("more than 2^32 entries"))
}

/// Text and its end offsets, zero-padded to a multiple of 4.
fn text_column<'s>(items: impl Iterator<Item = &'s str>) -> Result<(Vec<u8>, Vec<u32>), Error> {
    let (mut text, mut ends) = (Vec::new(), Vec::new());
    for s in items {
        text.extend_from_slice(s.as_bytes());
        ends.push(index(text.len())?);
    }
    text.resize(text.len().div_ceil(4) * 4, 0);
    Ok((text, ends))
}

/// The position of `x` in `v`, appended if new (numbering by first use).
fn intern<'a, T: PartialEq + ?Sized>(v: &mut Vec<&'a T>, x: &'a T) -> usize {
    v.iter().position(|y| *y == x).unwrap_or_else(|| {
        v.push(x);
        v.len() - 1
    })
}

/// The zones in name order, refused if empty, a name repeats or is not tzdb's spelling, or
/// two zones share a place.
fn ordered<'z, 'a>(zones: &'z [ZoneInput<'a>]) -> Result<Vec<&'z ZoneInput<'a>>, Error> {
    let mut order: Vec<&ZoneInput<'_>> = zones.iter().collect();
    order.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
    if order.is_empty() {
        return Err(werr("no zones"));
    }
    if order
        .windows(2)
        .any(|w| w.first().map(|a| a.name) == w.get(1).map(|b| b.name))
        || order.iter().any(|z| !is_zone_name(z.name))
    {
        return Err(werr("a repeated zone name, or one tzdb would not spell"));
    }
    let mut places = BTreeSet::new();
    if order
        .iter()
        .filter_map(|z| z.place)
        .any(|p| !places.insert(p))
    {
        return Err(werr("two zones with one timezone area"));
    }
    Ok(order)
}

/// Refuses a set the columns cannot hold.
fn check_set(s: &ZoneRules) -> Result<(), Error> {
    if !(2..=4).contains(&s.version) || s.types.is_empty() || s.types.len() > 256 {
        return Err(werr(
            "a TZif version outside 2–4, or no types, or more than 256",
        ));
    }
    if s.transitions.windows(2).any(|w| match w {
        [a, b] => a.0 >= b.0,
        _ => false,
    }) || s
        .transitions
        .iter()
        .any(|t| usize::from(t.1) >= s.types.len())
    {
        return Err(werr("transitions not increasing, or a type past the set"));
    }
    if usize::from(s.first_abbr) >= s.types.len() {
        return Err(werr("a first abbreviation past the types"));
    }
    Ok(())
}

fn abbr_of(abbrs: &[&str], a: &str) -> Result<u32, Error> {
    abbrs
        .binary_search(&a)
        .map_err(|_| werr("an abbreviation"))
        .and_then(index)
}

/// The per-set, per-transition and per-type columns.
#[derive(Default)]
struct SetColumns {
    trans_ends: Vec<u32>,
    type_ends: Vec<u32>,
    tzif: Vec<u32>,
    minute: Vec<u32>,
    second: Vec<u32>,
    ttype: Vec<u32>,
    offsets: Vec<u32>,
    flags: Vec<u32>,
}

fn set_columns(sets: &[&ZoneRules], abbrs: &[&str], base: i64) -> Result<SetColumns, Error> {
    let mut c = SetColumns::default();
    for s in sets {
        for &(t, k) in &s.transitions {
            c.minute.push(
                u32::try_from(t.div_euclid(60) - base)
                    .map_err(|_| werr("transitions spanning more than 2^32 minutes"))?,
            );
            c.second
                .push(u32::try_from(t.rem_euclid(60)).map_err(|_| werr("a second"))?);
            c.ttype.push(u32::from(k));
        }
        for t in &s.types {
            c.offsets.push(biased(t.offset)?);
            c.flags
                .push(u32::from(t.is_dst) | (abbr_of(abbrs, &t.abbr)? << 1));
        }
        c.trans_ends.push(index(c.minute.len())?);
        c.type_ends.push(index(c.offsets.len())?);
        c.tzif
            .push(u32::from(s.version - 2) | (u32::from(s.first_abbr) << 2));
    }
    Ok(c)
}

/// `FOOT_STD`, `FOOT_DST` and `FOOT_RULES`.
fn foot_columns(feet: &[&Footer], abbrs: &[&str]) -> Result<[Vec<u32>; 3], Error> {
    let (mut std, mut dst, mut rules) = (Vec::new(), Vec::new(), Vec::new());
    for f in feet {
        std.extend([biased(f.std_offset)?, abbr_of(abbrs, &f.std_abbr)?]);
        match &f.dst {
            None => {
                dst.extend([0, 0]);
                rules.extend([0; 4]);
            }
            Some(d) => {
                dst.extend([abbr_of(abbrs, &d.abbr)? + 1, biased(d.offset)?]);
                rules.extend(rule_values(d.start)?);
                rules.extend(rule_values(d.end)?);
            }
        }
    }
    Ok([std, dst, rules])
}

/// Encodes a `TZ_RULES` payload ([`Encoding::TABLE`](crate::Encoding::TABLE)); its item count
/// is `zones.len()`. Zones are stored in name order; zones with equal rules share one set.
/// The payload is opened and checked before it is returned.
///
/// # Errors
/// [`Error::Write`] for no zones, a repeated or invalid name, two zones with one place, or
/// rules out of range; [`Error::Payload`] if the result fails its check (a footer that
/// disagrees with its last transition).
pub fn encode(zones: &[ZoneInput<'_>], backzone: bool) -> Result<Vec<u8>, Error> {
    let order = ordered(zones)?;
    let mut sets: Vec<&ZoneRules> = Vec::new();
    let name_set = order
        .iter()
        .map(|z| index(intern(&mut sets, z.rules)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut abbrs: BTreeSet<&str> = BTreeSet::new();
    let mut feet: Vec<&Footer> = Vec::new();
    let mut set_footer = Vec::with_capacity(sets.len());
    for s in &sets {
        check_set(s)?;
        abbrs.extend(s.types.iter().map(|t| t.abbr.as_str()));
        let Some(f) = &s.footer else {
            set_footer.push(0);
            continue;
        };
        abbrs.insert(&f.std_abbr);
        abbrs.extend(f.dst.iter().map(|d| d.abbr.as_str()));
        set_footer.push(index(intern(&mut feet, f) + 1)?);
    }
    if abbrs.iter().any(|a| !is_abbr(a)) {
        return Err(werr("an abbreviation not of RFC 9636's characters"));
    }
    let abbrs: Vec<&str> = abbrs.into_iter().collect();
    let base = sets
        .iter()
        .flat_map(|s| s.transitions.iter())
        .map(|t| t.0.div_euclid(60))
        .min()
        .unwrap_or(0);
    let c = set_columns(&sets, &abbrs, base)?;
    let [f_std, f_dst, f_rules] = foot_columns(&feet, &abbrs)?;
    let (name_text, name_ends) = text_column(order.iter().map(|z| z.name))?;
    let (abbr_text, abbr_ends) = text_column(abbrs.iter().copied())?;
    let place: Vec<u32> = order
        .iter()
        .map(|z| {
            z.place
                .map_or(Ok(0), |p| p.checked_add(1).ok_or(werr("a place row")))
        })
        .collect::<Result<_, _>>()?;
    let b = base.cast_unsigned();
    #[allow(clippy::cast_possible_truncation, reason = "the two halves of a u64")]
    let meta = [
        b as u32,
        (b >> 32) as u32,
        if backzone { FLAG_BACKZONE } else { 0 },
    ];
    let auto = encode_auto;
    let cols: Vec<(u16, Codec, Vec<u8>)> = alloc::vec![
        (META, Codec::Bitpack, auto(&meta)),
        (NAME_TEXT, Codec::Bytes, name_text),
        // Binary searches read these: O(1) codings only.
        (NAME_ENDS, Codec::Bitpack, encode_direct(&name_ends)),
        (NAME_SET, Codec::Bitpack, auto(&name_set)),
        (NAME_PLACE, Codec::Bitpack, auto(&place)),
        (SET_TRANS_ENDS, Codec::Bitpack, auto(&c.trans_ends)),
        (SET_TYPE_ENDS, Codec::Bitpack, auto(&c.type_ends)),
        (SET_FOOTER, Codec::Bitpack, auto(&set_footer)),
        (TRANS_MINUTE, Codec::Bitpack, encode_direct(&c.minute)),
        (TRANS_SECOND, Codec::Bitpack, auto(&c.second)),
        (TRANS_TYPE, Codec::Bitpack, auto(&c.ttype)),
        (TYPE_OFFSET, Codec::Bitpack, auto(&c.offsets)),
        (TYPE_FLAGS, Codec::Bitpack, auto(&c.flags)),
        (ABBR_TEXT, Codec::Bytes, abbr_text),
        (ABBR_ENDS, Codec::Bitpack, auto(&abbr_ends)),
        (FOOT_STD, Codec::Bitpack, auto(&f_std)),
        (FOOT_DST, Codec::Bitpack, auto(&f_dst)),
        (FOOT_RULES, Codec::Bitpack, auto(&f_rules)),
        (SET_TZIF, Codec::Bitpack, auto(&c.tzif)),
    ];
    let refs: Vec<(u16, Codec, &[u8])> = cols.iter().map(|(i, c, b)| (*i, *c, &b[..])).collect();
    let bytes = encode_with(&refs).map_err(|_| werr("the zone rules table"))?;
    let s = Section::unpacked(Kind::TZ_RULES, order.len() as u64, &bytes);
    TzRules::open(&s)?.check()?;
    Ok(bytes)
}
