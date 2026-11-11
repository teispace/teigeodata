//! ISO 3166 codes of countries and their subdivisions (D-197, [ISO 3166
//! codes](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/iso-3166-codes.md)):
//! an ISO 3166-1 alpha-2 code (`NP`) or an ISO 3166-2 code, the alpha-2 code, a hyphen and one
//! to three characters of `A`–`Z` and `0`–`9` (`NP-P3`, `IN-TN`, `FR-75C`).
//!
//! An [`IsoCode`] holds its text inline, so [`IsoCode::as_str`] borrows it with no allocation.
//! Packs store a code as 28 bits of a `u32` ([`IsoCode::packed`]):
//!
//! ```text
//! bits 27..23 alpha 1 | 22..18 alpha 2 | 17..12 sub 1 | 11..6 sub 2 | 5..0 sub 3
//! ```
//!
//! Letters of the alpha-2 code are 1–26; a subdivision character is 0 (none), 1–10 for `0`–`9`,
//! 11–36 for `A`–`Z`. The value is never 0, a country is a code with sub 1 = 0, and integer
//! order equals the codes' string order (`NP` < `NP-P1` < `NP-P7` < `NR`).
//!
//! User-assigned alpha-2 codes (`AA`, `QM`–`QZ`, `XA`–`XZ`, `ZZ`) are not ISO codes and are
//! refused, `XK` (Kosovo, used by the EU) among them.

use core::fmt;
use core::str::FromStr;

/// The longest code: two letters, a hyphen and three characters.
const LEN: usize = 6;

/// An ISO 3166-1 alpha-2 or ISO 3166-2 code, upper case. See the [module](self).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IsoCode([u8; LEN]);

/// Why a text or a packed value is not an ISO 3166 code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum IsoError {
    /// Not two letters, optionally a hyphen and one to three letters or digits.
    Form,
    /// A user-assigned alpha-2 code (`AA`, `QM`–`QZ`, `XA`–`XZ`, `ZZ`).
    UserAssigned,
}

impl fmt::Display for IsoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Form => {
                "an ISO 3166 code is two letters, optionally a hyphen and 1 to 3 letters or digits"
            }
            Self::UserAssigned => "a user-assigned alpha-2 code is not an ISO 3166 code",
        })
    }
}

impl core::error::Error for IsoError {}

/// Whether the upper-case pair `a b` is user-assigned.
const fn user_assigned(a: u8, b: u8) -> bool {
    matches!((a, b), (b'A', b'A') | (b'Z', b'Z') | (b'X', _)) || (a == b'Q' && b >= b'M')
}

/// The 6-bit value of a subdivision character, 0 for none.
const fn sub_value(c: u8) -> u32 {
    match c {
        b'0'..=b'9' => (c - b'0') as u32 + 1,
        b'A'..=b'Z' => (c - b'A') as u32 + 11,
        _ => 0,
    }
}

/// The character of a subdivision value; 0 for 0 or a value past `Z`.
const fn sub_char(v: u32) -> u8 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "v is at most 36 in both arms"
    )]
    match v {
        1..=10 => b'0' + (v - 1) as u8,
        11..=36 => b'A' + (v - 11) as u8,
        _ => 0,
    }
}

impl IsoCode {
    /// The code of `text`, in either case.
    ///
    /// # Errors
    /// [`IsoError::Form`] for anything but two ASCII letters, optionally followed by a hyphen
    /// and one to three ASCII letters or digits; [`IsoError::UserAssigned`] for a
    /// user-assigned alpha-2 code.
    pub fn from_bytes(text: &[u8]) -> Result<Self, IsoError> {
        if !(text.len() == 2 || (4..=LEN).contains(&text.len())) {
            return Err(IsoError::Form);
        }
        let mut out = [0_u8; LEN];
        for (i, (slot, &raw)) in out.iter_mut().zip(text).enumerate() {
            let c = raw.to_ascii_uppercase();
            let ok = match i {
                0 | 1 => c.is_ascii_uppercase(),
                2 => c == b'-',
                _ => c.is_ascii_uppercase() || c.is_ascii_digit(),
            };
            if !ok {
                return Err(IsoError::Form);
            }
            *slot = c;
        }
        if user_assigned(out[0], out[1]) {
            return Err(IsoError::UserAssigned);
        }
        Ok(Self(out))
    }

    /// The code of a packed value ([`IsoCode::packed`]); `None` for a value that is not one:
    /// 0, bits above 27, a letter or character out of range, a gap between subdivision
    /// characters, or a user-assigned alpha-2 code.
    #[must_use]
    pub fn from_packed(value: u32) -> Option<Self> {
        if value >> 28 != 0 {
            return None;
        }
        let letter = |shift: u32| {
            let v = (value >> shift) & 31;
            #[allow(clippy::cast_possible_truncation, reason = "v is 1 to 26")]
            (1..=26).contains(&v).then(|| b'A' + (v - 1) as u8)
        };
        let (first, second) = (letter(23)?, letter(18)?);
        if user_assigned(first, second) {
            return None;
        }
        let mut out = [first, second, 0, 0, 0, 0];
        let mut ended = false;
        for (slot, shift) in out.iter_mut().skip(3).zip([12, 6, 0]) {
            let v = (value >> shift) & 63;
            match sub_char(v) {
                0 if v != 0 => return None,
                0 => ended = true,
                _ if ended => return None,
                c => *slot = c,
            }
        }
        if out[3] != 0 {
            out[2] = b'-';
        }
        Some(Self(out))
    }

    /// The 28-bit value packs store; never 0. See the [module](self).
    #[must_use]
    pub const fn packed(self) -> u32 {
        let [first, second, _, sub1, sub2, sub3] = self.0;
        (first.saturating_sub(b'A' - 1) as u32) << 23
            | (second.saturating_sub(b'A' - 1) as u32) << 18
            | sub_value(sub1) << 12
            | sub_value(sub2) << 6
            | sub_value(sub3)
    }

    /// The code's text, upper case: `NP`, `NP-P3`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        let n = self.0.iter().position(|&c| c == 0).unwrap_or(LEN);
        // Always ASCII: built only from checked letters, digits and the hyphen.
        self.0
            .get(..n)
            .and_then(|t| core::str::from_utf8(t).ok())
            .unwrap_or_default()
    }

    /// The ISO 3166-1 alpha-2 part, upper case: `NP` for `NP-P3`.
    #[must_use]
    pub const fn country(self) -> [u8; 2] {
        [self.0[0], self.0[1]]
    }

    /// The country's own code: `NP` for `NP-P3` and for `NP`.
    #[must_use]
    pub const fn country_code(self) -> Self {
        Self([self.0[0], self.0[1], 0, 0, 0, 0])
    }

    /// Whether it is an ISO 3166-1 alpha-2 code (a country), not a subdivision's.
    #[must_use]
    pub const fn is_country(self) -> bool {
        self.0[3] == 0
    }
}

impl FromStr for IsoCode {
    type Err = IsoError;

    /// As [`IsoCode::from_bytes`].
    fn from_str(s: &str) -> Result<Self, IsoError> {
        Self::from_bytes(s.as_bytes())
    }
}

impl fmt::Display for IsoCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for IsoCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IsoCode({})", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{IsoCode, IsoError};
    use std::string::ToString;
    use std::vec::Vec;

    fn code(s: &str) -> IsoCode {
        s.parse().unwrap_or_else(|e| panic!("{s} is refused: {e}"))
    }

    #[test]
    fn codes_parse_in_either_case_and_read_back() {
        for (text, want) in [
            ("NP", "NP"),
            ("np-p3", "NP-P3"),
            ("IN-TN", "IN-TN"),
            ("fr-75c", "FR-75C"),
            ("GB-ENG", "GB-ENG"),
            ("CN-TW", "CN-TW"),
            ("ES-M", "ES-M"),
        ] {
            let c = code(text);
            assert_eq!(c.as_str(), want);
            assert_eq!(c.to_string(), want);
            assert_eq!(IsoCode::from_packed(c.packed()), Some(c), "{want}");
            assert_eq!(c.country(), [want.as_bytes()[0], want.as_bytes()[1]]);
            assert_eq!(c.is_country(), want.len() == 2);
            assert_eq!(c.country_code().as_str(), &want[..2]);
        }
        assert_eq!(std::format!("{:?}", code("np-p1")), "IsoCode(NP-P1)");
    }

    #[test]
    fn malformed_and_user_assigned_codes_are_refused() {
        for bad in [
            "", "N", "NPL", "NP-", "NP-P123", "NP_P1", "N1", "1N", "NP-P-", "NP--1", "NP-é",
            "NPP1", "NP-p 1",
        ] {
            assert_eq!(bad.parse::<IsoCode>(), Err(IsoError::Form), "{bad:?}");
        }
        for user in ["AA", "QM", "QZ", "XA", "XK", "xk-01", "ZZ"] {
            assert_eq!(
                user.parse::<IsoCode>(),
                Err(IsoError::UserAssigned),
                "{user}"
            );
        }
        // QA (Qatar) and QL (not user-assigned, unassigned) are not refused as user-assigned.
        assert!("QA".parse::<IsoCode>().is_ok());
    }

    /// The packing is a bijection on valid codes, and integer order is string order.
    #[test]
    fn packing_round_trips_and_keeps_string_order() {
        let alphabet = b"0129AZ";
        let mut codes: Vec<IsoCode> = Vec::new();
        for a in *b"ABNZ" {
            for b in *b"DPRY" {
                if a == b'Z' && b == b'Z' {
                    continue;
                }
                codes.push(IsoCode::from_bytes(&[a, b]).unwrap_or_else(|e| panic!("{e}")));
                for &x in alphabet {
                    codes.push(code(&std::format!(
                        "{}{}-{}", a as char, b as char, x as char
                    )));
                    for &y in alphabet {
                        let two =
                            std::format!("{}{}-{}{}", a as char, b as char, x as char, y as char);
                        codes.push(code(&two));
                        for &z in &alphabet[..2] {
                            codes.push(code(&std::format!("{two}{}", z as char)));
                        }
                    }
                }
            }
        }
        for c in &codes {
            assert_ne!(c.packed(), 0);
            assert!(c.packed() < 1 << 28);
            assert_eq!(IsoCode::from_packed(c.packed()), Some(*c));
        }
        let mut by_text = codes.clone();
        by_text.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));
        let mut by_value = codes;
        by_value.sort_unstable_by_key(|c| c.packed());
        assert_eq!(by_text, by_value);
        // Derived order is string order too.
        let mut derived = by_value.clone();
        derived.sort_unstable();
        assert_eq!(derived, by_value);
    }

    #[test]
    fn values_that_are_not_codes_are_refused() {
        let np = code("NP-P1").packed();
        for bad in [
            0,
            1 << 28 | np,
            27 << 23 | 1 << 18,              // alpha 1 past Z
            1 << 23,                         // alpha 2 is 0
            1 << 23 | 1 << 18 | 37 << 12,    // a character past Z
            1 << 23 | 1 << 18 | 5 << 6,      // sub 2 without sub 1
            1 << 23 | 1 << 18 | 5 << 12 | 5, // sub 3 without sub 2
        ] {
            assert_eq!(IsoCode::from_packed(bad), None, "{bad:#x}");
        }
        // A user-assigned pair packs to a valid layout but is not a code.
        let xk = (24 << 23) | (11 << 18);
        assert_eq!(IsoCode::from_packed(xk), None);
    }
}
