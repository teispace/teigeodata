//! TZif from the columns (feature `alloc`): a zone's rules written back as the slim TZif file
//! `zic -b slim` wrote, byte for byte, so a consumer that wants TZif bytes (for `jiff`, ICU or
//! its own provider) gets them.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

use super::{Foot, RuleDate, RuleTime, Zone};
use crate::Error;

/// `zic`'s `addabbr`: adds `abbr` to the characters `chs` (NUL-terminated strings), sharing a
/// suffix of an existing abbreviation, or growing one that is a suffix of `abbr`; its index.
pub(super) fn add_abbr(chs: &mut Vec<u8>, abbr: &[u8]) -> usize {
    let abbr_len = abbr.len();
    let mut incr = abbr_len + 1;
    let mut i = 0;
    while i < chs.len() {
        let rest = chs.get(i..).unwrap_or_default();
        let chars_len = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        if abbr_len <= chars_len {
            let isuff = i + (chars_len - abbr_len);
            if chs.get(isuff..isuff + abbr_len) == Some(abbr) {
                return isuff;
            }
        } else if chs.get(i..i + chars_len) == abbr.get(abbr_len - chars_len..) {
            incr = abbr_len - chars_len;
            break;
        }
        i += chars_len + 1;
    }
    let mut add: Vec<u8> = abbr.to_vec();
    add.push(0);
    add.truncate(incr);
    let at = i.min(chs.len());
    chs.splice(at..at, add);
    at
}

/// `zic`'s `stringoffset`: `[-]h[:mm[:ss]]`.
fn hms(out: &mut String, v: i64) {
    let a = v.unsigned_abs();
    if v < 0 {
        out.push('-');
    }
    let _ = write!(out, "{}", a / 3_600);
    if !a.is_multiple_of(3_600) {
        let _ = write!(out, ":{:02}", a / 60 % 60);
        if !a.is_multiple_of(60) {
            let _ = write!(out, ":{:02}", a % 60);
        }
    }
}

/// `zic`'s abbreviation: bare when all letters, else in `<…>`.
fn abbr(out: &mut String, a: &str) {
    if a.bytes().all(|b| b.is_ascii_alphabetic()) {
        out.push_str(a);
    } else {
        out.push('<');
        out.push_str(a);
        out.push('>');
    }
}

fn rule(out: &mut String, r: RuleTime) {
    match r.date {
        RuleDate::Julian1(n) => {
            let _ = write!(out, "J{n}");
        }
        RuleDate::Julian0(n) => {
            let _ = write!(out, "{n}");
        }
        RuleDate::Month {
            month,
            week,
            weekday,
        } => {
            let _ = write!(out, "M{month}.{week}.{weekday}");
        }
    }
    if r.time != 7_200 {
        out.push('/');
        hms(out, i64::from(r.time));
    }
}

/// The footer as `zic` writes it.
fn footer(z: &Zone<'_>, f: &Foot) -> Result<String, Error> {
    let mut s = String::new();
    abbr(&mut s, z.abbr_text(f.std_abbr)?);
    hms(&mut s, -i64::from(f.std));
    if let Some(d) = &f.dst {
        abbr(&mut s, z.abbr_text(d.abbr)?);
        if i64::from(d.offset) != i64::from(f.std) + 3_600 {
            hms(&mut s, -i64::from(d.offset));
        }
        s.push(',');
        rule(&mut s, d.start);
        s.push(',');
        rule(&mut s, d.end);
    }
    Ok(s)
}

fn header(out: &mut Vec<u8>, version: u8, counts: [usize; 6]) -> Result<(), Error> {
    out.extend_from_slice(b"TZif");
    out.push(b'0' + version);
    out.extend_from_slice(&[0; 15]);
    for c in counts {
        let c = u32::try_from(c).map_err(|_| Error::Write("a TZif count"))?;
        out.extend_from_slice(&c.to_be_bytes());
    }
    Ok(())
}

impl Zone<'_> {
    /// The zone's rules as a slim TZif file (RFC 9636), as `zic -b slim` wrote it: an empty
    /// version 1 block, then 64-bit transitions, the types, the abbreviations and the footer.
    ///
    /// # Errors
    /// [`Error::Payload`] on broken data; [`Error::Write`] past TZif's limits.
    pub fn tzif(&self) -> Result<Vec<u8>, Error> {
        let types: Vec<_> = self.types().collect::<Result<_, _>>()?;
        let trans: Vec<(i64, u32)> = self.raw_transitions().collect::<Result<_, _>>()?;
        let mut chars = Vec::new();
        let first = self.first_abbr();
        for k in 0..types.len() {
            let k = match k {
                0 => first,
                k if k == first => 0,
                k => k,
            };
            if let Some(t) = types.get(k) {
                add_abbr(&mut chars, t.abbr.as_bytes());
            }
        }
        let mut out = Vec::new();
        header(&mut out, self.version(), [0, 0, 0, 0, 1, 1])?;
        out.extend_from_slice(&[0; 7]);
        header(
            &mut out,
            self.version(),
            [0, 0, 0, trans.len(), types.len(), chars.len()],
        )?;
        for (t, _) in &trans {
            out.extend_from_slice(&t.to_be_bytes());
        }
        for (_, k) in &trans {
            out.push(u8::try_from(*k).map_err(|_| Error::Write("a TZif type index"))?);
        }
        for t in &types {
            let at = add_abbr(&mut chars, t.abbr.as_bytes());
            out.extend_from_slice(&t.offset.to_be_bytes());
            out.push(u8::from(t.is_dst));
            out.push(u8::try_from(at).map_err(|_| Error::Write("TZif abbreviations"))?);
        }
        out.extend_from_slice(&chars);
        out.push(b'\n');
        if let Some(f) = self.foot() {
            out.extend_from_slice(footer(self, f)?.as_bytes());
        }
        out.push(b'\n');
        Ok(out)
    }
}
