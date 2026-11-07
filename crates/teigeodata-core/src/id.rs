//! The stable ID (ADR-0008, D-070).
//!
//! Bits 63–48 are the allocation namespace, bits 47–0 the serial:
//! - namespace 0 is "no country" (oceans, seas, the world), written `world`;
//! - namespaces 1–676 are two-letter country codes, `(c1 − 'a') × 26 + (c2 − 'a') + 1`, so any
//!   two letters work without a table;
//! - namespaces 677–65,535 are reserved, and an ID using one is invalid;
//! - serial 0 is invalid, so the u64 0 is never an ID and can mean "none" in a column.
//!
//! The namespace is the country that *allocated* the ID, not where the place is now. IDs sort by
//! namespace, then serial, so a country's IDs share their high bits.

use core::fmt;
use core::num::NonZeroU64;
use core::str::FromStr;

/// The largest serial: 2⁴⁸ − 1.
pub const SERIAL_MAX: u64 = (1 << 48) - 1;

const COUNTRIES: u16 = 26 * 26;
const PREFIX: &str = "tgd:";
const WORLD: &str = "world";
/// The longest text form: `tgd:world/` and ten base32 digits.
const TEXT_MAX: usize = 20;

/// The allocation namespace of an ID: a country, or [`Namespace::WORLD`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Namespace(u16);

impl Namespace {
    /// No country: oceans, seas, continents, the world.
    pub const WORLD: Self = Self(0);

    /// The namespace of a two-letter country code, in either case: `np`, `IN`.
    #[must_use]
    pub const fn from_country(code: [u8; 2]) -> Option<Self> {
        let (a, b) = (code[0].to_ascii_lowercase(), code[1].to_ascii_lowercase());
        if a.is_ascii_lowercase() && b.is_ascii_lowercase() {
            // Both are at most 25 after the subtraction, so this is at most 676.
            Some(Self((a - b'a') as u16 * 26 + (b - b'a') as u16 + 1))
        } else {
            None
        }
    }

    /// The country code, lower case; `None` for [`Namespace::WORLD`].
    #[must_use]
    pub const fn country(self) -> Option<[u8; 2]> {
        if self.0 == 0 {
            return None;
        }
        let n = self.0 - 1;
        #[allow(
            clippy::cast_possible_truncation,
            reason = "n < 676: both parts are below 26"
        )]
        let code = [b'a' + (n / 26) as u8, b'a' + (n % 26) as u8];
        Some(code)
    }

    /// The raw namespace number.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    const fn from_raw(n: u16) -> Option<Self> {
        if n <= COUNTRIES { Some(Self(n)) } else { None }
    }
}

/// A stable place ID: allocated once, never reused (ADR-0008).
///
/// `Option<PlaceId>` is 8 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlaceId(NonZeroU64);

impl PlaceId {
    /// The ID with this namespace and serial; `None` if the serial is 0 or above [`SERIAL_MAX`].
    #[must_use]
    pub const fn new(namespace: Namespace, serial: u64) -> Option<Self> {
        if serial == 0 || serial > SERIAL_MAX {
            return None;
        }
        match NonZeroU64::new(((namespace.0 as u64) << 48) | serial) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    /// The ID stored as this u64; `None` if it is not a valid ID (serial 0, reserved namespace).
    #[must_use]
    pub const fn from_raw(raw: u64) -> Option<Self> {
        match Namespace::from_raw((raw >> 48) as u16) {
            Some(ns) => Self::new(ns, raw & SERIAL_MAX),
            None => None,
        }
    }

    /// The u64 as stored in packs.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }

    /// The namespace that allocated this ID.
    #[must_use]
    pub const fn namespace(self) -> Namespace {
        Namespace((self.0.get() >> 48) as u16)
    }

    /// The serial within its namespace, 1 to [`SERIAL_MAX`].
    #[must_use]
    pub const fn serial(self) -> u64 {
        self.0.get() & SERIAL_MAX
    }

    /// Writes the text form into `buf` without allocating and returns it.
    pub fn encode(self, buf: &mut [u8; TEXT_MAX]) -> &str {
        let mut n = 0;
        let mut put = |b: u8, buf: &mut [u8; TEXT_MAX]| {
            if let Some(slot) = buf.get_mut(n) {
                *slot = b;
                n += 1;
            }
        };
        for &b in PREFIX.as_bytes() {
            put(b, buf);
        }
        match self.namespace().country() {
            Some([a, b]) => {
                put(a, buf);
                put(b, buf);
            }
            None => {
                for &b in WORLD.as_bytes() {
                    put(b, buf);
                }
            }
        }
        put(b'/', buf);
        let serial = self.serial();
        // Number of base32 digits: serial ≥ 1, so at least one.
        let digits = serial.bit_width().div_ceil(5);
        for i in (0..digits).rev() {
            #[allow(clippy::cast_possible_truncation, reason = "masked to 5 bits")]
            let v = (serial >> (5 * i)) as u8 & 31;
            put(digit(v), buf);
        }
        // Every byte written is ASCII, and n never exceeds the buffer.
        buf.get(..n)
            .and_then(|b| core::str::from_utf8(b).ok())
            .unwrap_or_default()
    }
}

/// The Crockford base32 digit for `v` (below 32), lower case: `0-9 a-h j k m n p-t v-z`.
const fn digit(v: u8) -> u8 {
    match v {
        0..=9 => b'0' + v,
        10..=17 => b'a' + v - 10,
        18..=19 => b'j' + v - 18,
        20..=21 => b'm' + v - 20,
        22..=26 => b'p' + v - 22,
        _ => b'v' + v - 27,
    }
}

/// The value of a Crockford base32 digit, either case; `None` for anything else, including the
/// excluded letters `i l o u` (strict: one text per ID).
const fn value(c: u8) -> Option<u8> {
    let c = c.to_ascii_lowercase();
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'h' => Some(c - b'a' + 10),
        b'j'..=b'k' => Some(c - b'j' + 18),
        b'm'..=b'n' => Some(c - b'm' + 20),
        b'p'..=b't' => Some(c - b'p' + 22),
        b'v'..=b'z' => Some(c - b'v' + 27),
        _ => None,
    }
}

impl fmt::Display for PlaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.encode(&mut [0; TEXT_MAX]))
    }
}

/// Why a text is not an ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdError {
    /// It does not start with `tgd:`.
    Prefix,
    /// The namespace is neither two letters nor `world`.
    Namespace,
    /// No `/` after the namespace.
    Separator,
    /// The serial is empty, has a leading zero, a character outside Crockford base32, is 0, or
    /// is above [`SERIAL_MAX`].
    Serial,
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Prefix => "an ID starts with `tgd:`",
            Self::Namespace => "the ID namespace is not a two-letter country code or `world`",
            Self::Separator => "the ID has no `/` after its namespace",
            Self::Serial => {
                "the ID serial is not 1 to 2^48-1 in Crockford base32 without leading zeros"
            }
        })
    }
}

impl core::error::Error for IdError {}

impl FromStr for PlaceId {
    type Err = IdError;

    /// Parses the text form, in either case. Exactly one text (ignoring case) parses to each ID.
    fn from_str(s: &str) -> Result<Self, IdError> {
        let b = s.as_bytes();
        let rest = b
            .get(..PREFIX.len())
            .filter(|p| p.eq_ignore_ascii_case(PREFIX.as_bytes()))
            .and_then(|_| b.get(PREFIX.len()..))
            .ok_or(IdError::Prefix)?;
        let slash = rest
            .iter()
            .position(|&c| c == b'/')
            .ok_or(IdError::Separator)?;
        let (ns, serial) = (
            rest.get(..slash).unwrap_or_default(),
            rest.get(slash + 1..).unwrap_or_default(),
        );
        let namespace = match ns {
            [a, b] => Namespace::from_country([*a, *b]).ok_or(IdError::Namespace)?,
            _ if ns.eq_ignore_ascii_case(WORLD.as_bytes()) => Namespace::WORLD,
            _ => return Err(IdError::Namespace),
        };
        // At most ten digits (50 bits), and no leading zero.
        if serial.is_empty() || serial.len() > 10 || serial.first() == Some(&b'0') {
            return Err(IdError::Serial);
        }
        let mut v: u64 = 0;
        for &c in serial {
            v = (v << 5) | u64::from(value(c).ok_or(IdError::Serial)?);
        }
        Self::new(namespace, v).ok_or(IdError::Serial)
    }
}

#[cfg(test)]
mod tests {
    use super::{IdError, Namespace, PlaceId, SERIAL_MAX, TEXT_MAX, digit, value};
    use std::string::ToString;

    fn id(cc: [u8; 2], serial: u64) -> PlaceId {
        Namespace::from_country(cc)
            .and_then(|ns| PlaceId::new(ns, serial))
            .unwrap_or_else(|| unreachable!())
    }

    #[test]
    fn alphabet_is_crockford() {
        let all: std::vec::Vec<u8> = (0..32).map(digit).collect();
        assert_eq!(all, b"0123456789abcdefghjkmnpqrstvwxyz");
        for v in 0..32 {
            assert_eq!(value(digit(v)), Some(v));
            assert_eq!(value(digit(v).to_ascii_uppercase()), Some(v));
        }
        for c in [
            b'i', b'l', b'o', b'u', b'I', b'L', b'O', b'U', b'-', b' ', 0, 0xff,
        ] {
            assert_eq!(value(c), None, "{c}");
        }
    }

    #[test]
    fn every_namespace_round_trips() {
        assert_eq!(Namespace::WORLD.country(), None);
        let mut seen = std::collections::BTreeSet::new();
        for a in b'a'..=b'z' {
            for b in b'a'..=b'z' {
                let ns = Namespace::from_country([a, b]).unwrap_or_else(|| unreachable!());
                assert!((1..=676).contains(&ns.get()));
                assert_eq!(ns.country(), Some([a, b]));
                assert_eq!(
                    Namespace::from_country([a.to_ascii_uppercase(), b]),
                    Some(ns)
                );
                assert!(seen.insert(ns.get()));
            }
        }
        assert_eq!(seen.len(), 676);
        for bad in [*b"n1", *b"-p", *b"n ", [0, 0], [0xc3, 0xa9]] {
            assert_eq!(Namespace::from_country(bad), None);
        }
    }

    #[test]
    fn edges() {
        assert_eq!(PlaceId::new(Namespace::WORLD, 0), None);
        assert_eq!(PlaceId::new(Namespace::WORLD, SERIAL_MAX + 1), None);
        assert_eq!(PlaceId::from_raw(0), None);
        // Namespace 677 is reserved.
        assert_eq!(PlaceId::from_raw((677 << 48) | 1), None);
        assert_eq!(PlaceId::from_raw(u64::MAX), None);

        let one = PlaceId::new(Namespace::WORLD, 1).unwrap_or_else(|| unreachable!());
        assert_eq!(one.get(), 1);
        assert_eq!(one.to_string(), "tgd:world/1");
        let max = id(*b"zz", SERIAL_MAX);
        assert_eq!(max.to_string(), "tgd:zz/7zzzzzzzzz");
        assert_eq!(max.to_string().len(), 17);
        let longest = PlaceId::new(Namespace::WORLD, SERIAL_MAX).unwrap_or_else(|| unreachable!());
        assert_eq!(longest.to_string().len(), TEXT_MAX);
        assert_eq!(id(*b"np", 32).to_string(), "tgd:np/10");
        assert_eq!(id(*b"np", 31).to_string(), "tgd:np/z");
        assert_eq!(id(*b"aa", 1).get(), (1 << 48) | 1);
        assert_eq!(size_of::<Option<PlaceId>>(), 8);
    }

    #[test]
    fn ids_sort_by_namespace_then_serial() {
        assert!(id(*b"in", SERIAL_MAX) < id(*b"np", 1));
        assert!(id(*b"np", 1) < id(*b"np", 2));
        assert!(PlaceId::new(Namespace::WORLD, SERIAL_MAX) < Some(id(*b"aa", 1)));
    }

    #[test]
    fn parse_rejects() {
        let cases: &[(&str, IdError)] = &[
            ("", IdError::Prefix),
            ("tgd", IdError::Prefix),
            ("gd:np/1", IdError::Prefix),
            ("tgd:np", IdError::Separator),
            ("tgd:/1", IdError::Namespace),
            ("tgd:n/1", IdError::Namespace),
            ("tgd:npl/1", IdError::Namespace),
            ("tgd:n1/1", IdError::Namespace),
            ("tgd:worlds/1", IdError::Namespace),
            ("tgd:np/", IdError::Serial),
            ("tgd:np/0", IdError::Serial),
            ("tgd:np/01", IdError::Serial),
            ("tgd:np/1i", IdError::Serial),
            ("tgd:np/1o", IdError::Serial),
            ("tgd:np/1/2", IdError::Serial),
            ("tgd:np/ 1", IdError::Serial),
            ("tgd:np/8000000000", IdError::Serial),
            ("tgd:np/zzzzzzzzzzz", IdError::Serial),
            ("tgd:np/é", IdError::Serial),
        ];
        for (text, err) in cases {
            assert_eq!(text.parse::<PlaceId>(), Err(*err), "{text}");
        }
    }

    #[test]
    fn parse_accepts_either_case() {
        let want = id(*b"np", 0x3_a5f);
        for text in ["tgd:np/3kq", "TGD:NP/3KQ", "Tgd:Np/3kQ"] {
            let got: Result<PlaceId, _> = text.parse();
            assert_eq!(
                got.map(PlaceId::serial),
                Ok(3 * 1024 + 19 * 32 + 23),
                "{text}"
            );
        }
        assert_eq!("tgd:WORLD/1".parse::<PlaceId>().map(PlaceId::get), Ok(1));
        assert_eq!(want.serial(), 0x3_a5f);
    }

    #[test]
    fn random_round_trips() {
        let mut s = 0x9e37_79b9_7f4a_7c15_u64;
        for _ in 0..crate::cases(200_000) {
            s = s.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = s;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^= z >> 31;
            // Vary the serial's length: shift by 0 to 47 bits.
            let serial = (z & SERIAL_MAX) >> (z >> 58).min(47);
            let ns = u16::try_from((z >> 48) % 677).unwrap_or_default();
            let raw = (u64::from(ns) << 48) | serial;
            match PlaceId::from_raw(raw) {
                None => assert_eq!(serial, 0),
                Some(i) => {
                    assert_eq!(i.get(), raw);
                    let text = i.to_string();
                    assert_eq!(text.parse::<PlaceId>(), Ok(i), "{text}");
                    assert_eq!(text.to_uppercase().parse::<PlaceId>(), Ok(i), "{text}");
                }
            }
        }
    }
}
