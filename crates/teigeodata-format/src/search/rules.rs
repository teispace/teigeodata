//! A country's query rules, as data in the SEARCH section ([`RULES`](super::RULES), ADR-0032:
//! the reader holds no country's rules).
//!
//! A sequence of entries, then a zero byte, zero-padded to a multiple of 4. An entry is a kind
//! byte, then two length-prefixed UTF-8 fields `a` and `b` (a `u8` length each):
//!
//! | Kind | Rule | `a` | `b` |
//! |---:|---|---|---|
//! | 1 | [`Rule::Phonetic`] | the phonetic key's name (`indic-v0`) | empty |
//! | 2 | [`Rule::Unit`] | a unit type's abbreviation, folded (`rm`) | one byte: the unit type, 1 to 255 |
//! | 3 | [`Rule::Drop`] | a droppable suffix, folded (`dham`) | empty |
//! | 4 | [`Rule::Native`] | a generic term in a native script, folded (`गाउँ` folded) | one Latin spelling, folded (`gaun`) |
//! | 5 | [`Rule::Spelling`] | a letter run a query may write, folded (`v`) | the run the names write instead, folded (`b`) |
//!
//! A unit type is the value of column [`UNIT`](super::UNIT) for the rows of that type; type
//! [`LEVEL_WORD`] is a level's type word (`जिल्ला`), which no row carries: a query ending in it
//! drops it and favours no row (D-243). At most one phonetic entry. Reads never allocate and never panic; [`Rules::check`] proves the layout.

use crate::Error;

/// The unit type of a level's type word: no row carries it, so a query ending in the word drops
/// it and favours no row (D-243). A profile has at most 254 unit types.
pub const LEVEL_WORD: u8 = u8::MAX;

/// Entry kind: the phonetic key.
pub const PHONETIC: u8 = 1;
/// Entry kind: a unit type abbreviation.
pub const UNIT: u8 = 2;
/// Entry kind: a droppable suffix.
pub const DROP: u8 = 3;
/// Entry kind: a native generic and one Latin spelling.
pub const NATIVE: u8 = 4;
/// Entry kind: a spelling convention, one letter run for another (D-233).
pub const SPELLING: u8 = 5;

/// One query rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule<'a> {
    /// The phonetic key the PHONETIC map was written with.
    Phonetic(&'a str),
    /// A query ending in `abbreviation` (after a space) names unit type `unit`.
    Unit {
        /// The abbreviation, folded.
        abbreviation: &'a str,
        /// The unit type, as in column `UNIT`.
        unit: u8,
    },
    /// A suffix a query may carry that the names lack ("Janakpurdham" for Janakpur).
    Drop(&'a str),
    /// A generic in a native script, which a mixed-script query may end with, and one of its
    /// Latin spellings.
    Native {
        /// The native term, folded.
        native: &'a str,
        /// One Latin spelling, folded.
        spelling: &'a str,
    },
    /// A spelling convention of the country's names: a query may write `from` where the names
    /// write `to` (Nepali व as "v" in "Viratnagar", the names' "b" in "Biratnagar"). A query
    /// with every `from` replaced by `to` matches as loosely as one differing in case.
    Spelling {
        /// The run a query may write, folded.
        from: &'a str,
        /// The run the names write, folded.
        to: &'a str,
    },
}

/// A view of the rules bytes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Rules<'a> {
    bytes: &'a [u8],
}

impl<'a> Rules<'a> {
    /// Views `bytes` (unchecked: [`Rules::check`] proves them; a malformed tail ends the
    /// iteration).
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// No rules.
    #[must_use]
    pub const fn empty() -> Self {
        Self { bytes: &[] }
    }

    /// Every rule, in stored order.
    #[must_use]
    pub const fn iter(&self) -> Iter<'a> {
        Iter { rest: self.bytes }
    }

    /// The phonetic key's name, if any.
    #[must_use]
    pub fn phonetic(&self) -> Option<&'a str> {
        self.iter().find_map(|r| match r {
            Rule::Phonetic(n) => Some(n),
            _ => None,
        })
    }

    /// Checks the layout: known kinds, UTF-8 fields of the right shape, at most one phonetic
    /// entry, the terminator and zero padding. Returns the highest unit type named.
    ///
    /// # Errors
    /// [`Error::Payload`] (column RULES) on any violation.
    pub fn check(&self) -> Result<u8, Error> {
        let fail = |problem| Error::Payload {
            kind: crate::Kind::SEARCH.0,
            column: super::RULES,
            problem,
        };
        if !self.bytes.len().is_multiple_of(4) || self.bytes.is_empty() {
            return Err(fail("the rules are not padded to a multiple of 4"));
        }
        let mut rest = self.bytes;
        let mut phonetic = 0;
        let mut units = 0_u8;
        loop {
            let Some((&kind, tail)) = rest.split_first() else {
                return Err(fail("the rules have no terminator"));
            };
            if kind == 0 {
                if tail.iter().any(|&b| b != 0) {
                    return Err(fail("the rules' padding is not zero"));
                }
                return Ok(units);
            }
            let (a, tail) = field(tail).ok_or(fail("a rule field runs past the end"))?;
            let (b, tail) = field(tail).ok_or(fail("a rule field runs past the end"))?;
            rest = tail;
            text(a).ok_or(fail("a rule's text is empty or not UTF-8"))?;
            match kind {
                PHONETIC if b.is_empty() => {
                    phonetic += 1;
                    if phonetic > 1 {
                        return Err(fail("more than one phonetic key"));
                    }
                }
                UNIT => match b {
                    [u] if *u > 0 => units = units.max(*u),
                    _ => return Err(fail("a unit rule's type is not one byte from 1")),
                },
                DROP if b.is_empty() => {}
                NATIVE => {
                    text(b).ok_or(fail("a native rule's spelling is empty or not UTF-8"))?;
                }
                SPELLING => {
                    let to =
                        text(b).ok_or(fail("a spelling rule's target is empty or not UTF-8"))?;
                    if Some(to) == text(a) {
                        return Err(fail("a spelling rule maps a run to itself"));
                    }
                }
                _ => return Err(fail("an unknown rule, or a field where none belongs")),
            }
        }
    }
}

impl<'a> IntoIterator for &Rules<'a> {
    type Item = Rule<'a>;
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Iter<'a> {
        self.iter()
    }
}

/// The rules in stored order.
#[derive(Clone, Debug)]
pub struct Iter<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for Iter<'a> {
    type Item = Rule<'a>;

    fn next(&mut self) -> Option<Rule<'a>> {
        loop {
            let (&kind, tail) = self.rest.split_first()?;
            if kind == 0 {
                self.rest = &[];
                return None;
            }
            let parsed = field(tail).and_then(|(a, t)| field(t).map(|(b, t)| (a, b, t)));
            let Some((a, b, tail)) = parsed else {
                self.rest = &[];
                return None;
            };
            self.rest = tail;
            let Ok(a) = core::str::from_utf8(a) else {
                continue;
            };
            let rule = match (kind, b) {
                (PHONETIC, _) => Rule::Phonetic(a),
                (UNIT, [u]) => Rule::Unit {
                    abbreviation: a,
                    unit: *u,
                },
                (DROP, _) => Rule::Drop(a),
                (NATIVE, b) => match core::str::from_utf8(b) {
                    Ok(spelling) => Rule::Native {
                        native: a,
                        spelling,
                    },
                    Err(_) => continue,
                },
                (SPELLING, b) => match core::str::from_utf8(b) {
                    Ok(to) => Rule::Spelling { from: a, to },
                    Err(_) => continue,
                },
                _ => continue,
            };
            return Some(rule);
        }
    }
}

/// A non-empty UTF-8 field as text.
fn text(f: &[u8]) -> Option<&str> {
    core::str::from_utf8(f).ok().filter(|s| !s.is_empty())
}

/// A `u8`-length field and the bytes after it.
fn field(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let (&n, rest) = bytes.split_first()?;
    let n = usize::from(n);
    (rest.len() >= n).then(|| rest.split_at(n))
}

/// Encodes `rules` (feature `write`).
///
/// # Errors
/// [`Error::Write`] if a field is empty where text belongs, longer than 255 bytes, or a unit
/// type is 0.
#[cfg(feature = "write")]
pub fn encode(rules: &[Rule<'_>]) -> Result<alloc::vec::Vec<u8>, Error> {
    let mut out = alloc::vec::Vec::new();
    let mut put = |kind: u8, a: &[u8], b: &[u8]| -> Result<(), Error> {
        let len = |f: &[u8]| u8::try_from(f.len()).map_err(|_| Error::Write("a rule is too long"));
        if a.is_empty() {
            return Err(Error::Write("a rule's text is empty"));
        }
        out.push(kind);
        out.push(len(a)?);
        out.extend_from_slice(a);
        out.push(len(b)?);
        out.extend_from_slice(b);
        Ok(())
    };
    for r in rules {
        match *r {
            Rule::Phonetic(n) => put(PHONETIC, n.as_bytes(), &[])?,
            Rule::Unit { abbreviation, unit } => {
                if unit == 0 {
                    return Err(Error::Write("a unit type is 0"));
                }
                put(UNIT, abbreviation.as_bytes(), &[unit])?;
            }
            Rule::Drop(d) => put(DROP, d.as_bytes(), &[])?,
            Rule::Native { native, spelling } => {
                if spelling.is_empty() {
                    return Err(Error::Write("a native rule's spelling is empty"));
                }
                put(NATIVE, native.as_bytes(), spelling.as_bytes())?;
            }
            Rule::Spelling { from, to } => {
                if to.is_empty() || from == to {
                    return Err(Error::Write(
                        "a spelling rule's target is empty or its own run",
                    ));
                }
                put(SPELLING, from.as_bytes(), to.as_bytes())?;
            }
        }
    }
    out.push(0);
    out.resize(out.len().next_multiple_of(4), 0);
    Ok(out)
}

#[cfg(all(test, feature = "write"))]
mod tests {
    use super::{Rule, Rules, encode};

    #[test]
    fn rules_round_trip_and_bad_bytes_are_refused() {
        let rules = [
            Rule::Phonetic("indic-v0"),
            Rule::Unit {
                abbreviation: "rm",
                unit: 4,
            },
            Rule::Drop("dham"),
            Rule::Native {
                native: "गाउँ",
                spelling: "gaun",
            },
            Rule::Spelling { from: "v", to: "b" },
        ];
        let b = encode(&rules).unwrap();
        assert_eq!(b.len() % 4, 0);
        let r = Rules::new(&b);
        assert_eq!(r.check().unwrap(), 4);
        assert_eq!(r.iter().collect::<alloc::vec::Vec<_>>(), rules);
        assert_eq!(r.phonetic(), Some("indic-v0"));
        assert_eq!(Rules::new(&encode(&[]).unwrap()).check().unwrap(), 0);

        // Two phonetic keys, a unit type 0, an unknown kind, a cut field, no terminator,
        // non-zero padding: each is refused.
        let two = encode(&[Rule::Phonetic("a"), Rule::Phonetic("b")]).unwrap();
        assert!(Rules::new(&two).check().is_err());
        assert!(
            encode(&[Rule::Unit {
                abbreviation: "x",
                unit: 0
            }])
            .is_err()
        );
        assert!(encode(&[Rule::Spelling { from: "v", to: "v" }]).is_err());
        let mut to_itself = encode(&[Rule::Spelling { from: "v", to: "b" }]).unwrap();
        to_itself[4] = b'v';
        assert!(Rules::new(&to_itself).check().is_err());
        let mut unknown = b.clone();
        unknown[0] = 9;
        assert!(Rules::new(&unknown).check().is_err());
        assert!(Rules::new(&b[..6]).check().is_err());
        let end = b.iter().rposition(|&x| x != 0).unwrap();
        assert!(Rules::new(&b[..=end]).check().is_err());
        let mut pad = b.clone();
        *pad.last_mut().unwrap() = 1;
        assert!(Rules::new(&pad).check().is_err());
        // The iteration never panics on any prefix.
        for n in 0..b.len() {
            let _ = Rules::new(&b[..n]).iter().count();
        }
    }
}
