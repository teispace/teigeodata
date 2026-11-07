//! FSST string compression (B3, D-077): a table of up to 255 symbols of 1 to 8 bytes; a code
//! byte names a symbol, code 255 escapes one literal byte. Every string decodes on its own, so a
//! name is read without touching its neighbours (Boncz, Neumann and Leis, PVLDB 2020).
//!
//! The decoder is safe and allocates nothing. The trainer and encoder (feature `alloc`) follow
//! the paper's generational training, made deterministic: no hashing, every tie broken by the
//! symbol's bytes, so the same strings give the same table on every platform.
//!
//! Table layout, little-endian:
//! ```text
//! symbols u8 (0 to 255), reserved [u8; 7] (0)
//! lens    [u8; symbols], zero padding to a multiple of 8
//! bytes   [[u8; 8]; symbols]   each symbol's bytes, zero past its length
//! ```

use core::fmt;

const HEADER: usize = 8;
/// The escape code: the next byte is a literal.
pub const ESCAPE: u8 = 255;
/// The most symbols a table holds.
pub const MAX_SYMBOLS: usize = 255;

/// Why bytes are not a symbol table, or a code string does not decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FsstError {
    /// Not as long as the header says.
    Length,
    /// Reserved or padding bytes are not zero, a length is not 1 to 8, or a symbol has bytes
    /// past its length.
    Table,
    /// A code names no symbol, or the string ends inside an escape.
    Code,
    /// The output buffer is too small.
    Space,
}

impl fmt::Display for FsstError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Length => "the symbol table is not as long as its header says",
            Self::Table => "the symbol table is invalid",
            Self::Code => "the compressed string is invalid",
            Self::Space => "the output buffer is too small",
        })
    }
}

impl core::error::Error for FsstError {}

/// A validated symbol table, borrowing its bytes.
#[derive(Clone, Copy, Debug)]
pub struct Symbols<'a> {
    lens: &'a [u8],
    bytes: &'a [[u8; 8]],
}

const fn pad8(n: usize) -> usize {
    n.div_ceil(8) * 8
}

impl<'a> Symbols<'a> {
    /// Parses and validates a table: O(symbols).
    ///
    /// # Errors
    /// [`FsstError::Length`] or [`FsstError::Table`].
    pub fn parse(bytes: &'a [u8]) -> Result<Self, FsstError> {
        let (&[n, reserved @ ..], rest) = bytes
            .split_first_chunk::<HEADER>()
            .ok_or(FsstError::Length)?;
        if reserved != [0; HEADER - 1] {
            return Err(FsstError::Table);
        }
        let n = usize::from(n);
        if rest.len() != pad8(n) + 8 * n {
            return Err(FsstError::Length);
        }
        let (lens_padded, syms) = rest.split_at(pad8(n));
        let (lens, padding) = lens_padded.split_at(n);
        let syms = syms.as_chunks::<8>().0;
        if padding.iter().any(|&b| b != 0) {
            return Err(FsstError::Table);
        }
        for (&len, sym) in lens.iter().zip(syms) {
            let used = usize::from(len);
            if !(1..=8).contains(&used)
                || sym.get(used..).is_some_and(|t| t.iter().any(|&b| b != 0))
            {
                return Err(FsstError::Table);
            }
        }
        Ok(Self { lens, bytes: syms })
    }

    /// The number of symbols.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.lens.len()
    }

    /// Whether the table has no symbols (every byte is escaped).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.lens.is_empty()
    }

    /// Symbol `code`: its bytes (zero past the length) and its length.
    #[must_use]
    pub fn symbol(&self, code: u8) -> Option<([u8; 8], usize)> {
        let i = usize::from(code);
        Some((*self.bytes.get(i)?, usize::from(*self.lens.get(i)?)))
    }

    /// Decodes `codes` into the start of `out`, returning the decoded length. Each symbol is
    /// written as a whole 8-byte store while 8 bytes of room remain, which is what makes the
    /// loop fast; near the end of `out` only its own bytes are written.
    ///
    /// # Errors
    /// [`FsstError::Code`] for a code with no symbol or a final lone escape;
    /// [`FsstError::Space`] if `out` is too small for the result.
    pub fn decode(&self, codes: &[u8], out: &mut [u8]) -> Result<usize, FsstError> {
        let mut at = 0;
        let mut it = codes.iter();
        while let Some(&c) = it.next() {
            if c == ESCAPE {
                let &b = it.next().ok_or(FsstError::Code)?;
                *out.get_mut(at).ok_or(FsstError::Space)? = b;
                at += 1;
                continue;
            }
            let i = usize::from(c);
            let (&sym, &len) = self
                .bytes
                .get(i)
                .zip(self.lens.get(i))
                .ok_or(FsstError::Code)?;
            let len = usize::from(len);
            if let Some(dst) = out.get_mut(at..at + 8) {
                dst.copy_from_slice(&sym);
            } else {
                let dst = out.get_mut(at..at + len).ok_or(FsstError::Space)?;
                dst.copy_from_slice(sym.get(..len).ok_or(FsstError::Code)?);
            }
            at += len;
        }
        Ok(at)
    }

    /// An upper bound of the decoded length of `codes`: 8 bytes per code.
    #[must_use]
    pub const fn max_decoded_len(codes: &[u8]) -> usize {
        codes.len().saturating_mul(8)
    }
}

/// Training and encoding (feature `alloc`).
#[cfg(feature = "alloc")]
#[allow(
    clippy::indexing_slicing,
    reason = "builder side; every index is bounded by construction: codes below the table length, \
              pair keys below 65,536, count arrays of 512 codes, symbol bytes below 8"
)]
pub mod build {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::{ESCAPE, HEADER, MAX_SYMBOLS, pad8};

    /// Generations of training, as in the paper.
    const GENERATIONS: usize = 5;
    /// The training sample: about this many bytes of the input, taken at an even stride.
    const SAMPLE: usize = 1 << 20;

    /// A symbol as bytes and length.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct Sym {
        bytes: [u8; 8],
        len: u8,
    }

    impl Sym {
        fn of(s: &[u8]) -> Self {
            let mut bytes = [0; 8];
            let len = s.len().min(8);
            if let (Some(dst), Some(src)) = (bytes.get_mut(..len), s.get(..len)) {
                dst.copy_from_slice(src);
            }
            #[allow(clippy::cast_possible_truncation, reason = "len is at most 8")]
            Self {
                bytes,
                len: len as u8,
            }
        }

        fn concat(a: Self, b: Self) -> Self {
            let (la, lb) = (usize::from(a.len), usize::from(b.len));
            let mut joined = [0; 16];
            joined[..la].copy_from_slice(&a.bytes[..la]);
            joined[la..la + lb].copy_from_slice(&b.bytes[..lb]);
            Self::of(&joined[..(la + lb).min(8)])
        }
    }

    /// A trained table, ready to encode with and to write.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Encoder {
        syms: Vec<Sym>,
        /// For each first byte pair, the codes of symbols of length 2 or more starting with it,
        /// longest first: `by_pair[start[p]..start[p + 1]]`.
        start: Vec<u32>,
        by_pair: Vec<u8>,
        /// The code of the one-byte symbol for each byte, or `ESCAPE`.
        single: [u8; 256],
    }

    impl Encoder {
        fn new(mut syms: Vec<Sym>) -> Self {
            // Canonical code order: longest first, then by bytes.
            syms.sort_by(|a, b| b.len.cmp(&a.len).then(a.bytes.cmp(&b.bytes)));
            syms.truncate(MAX_SYMBOLS);
            let mut single = [ESCAPE; 256];
            let mut pairs: Vec<(u16, u8)> = Vec::new();
            for (code, s) in syms.iter().enumerate() {
                #[allow(clippy::cast_possible_truncation, reason = "at most 255 symbols")]
                let code = code as u8;
                if s.len == 1 {
                    single[usize::from(s.bytes[0])] = code;
                } else {
                    pairs.push((u16::from_le_bytes([s.bytes[0], s.bytes[1]]), code));
                }
            }
            // Codes are numbered longest first, so (pair, code) keeps that order within a pair.
            pairs.sort_unstable();
            let mut start = vec![0_u32; 65_537];
            for &(p, _) in &pairs {
                start[usize::from(p) + 1] += 1;
            }
            for i in 1..start.len() {
                start[i] += start[i - 1];
            }
            Self {
                syms,
                start,
                by_pair: pairs.iter().map(|p| p.1).collect(),
                single,
            }
        }

        /// The longest symbol at the start of `s` (non-empty): its code and length, or `None`
        /// when the first byte must be escaped.
        fn longest(&self, s: &[u8]) -> Option<(u8, usize)> {
            if let [b0, b1, ..] = *s {
                let p = usize::from(u16::from_le_bytes([b0, b1]));
                let (lo, hi) = (self.start[p] as usize, self.start[p + 1] as usize);
                for &code in &self.by_pair[lo..hi] {
                    let sym = &self.syms[usize::from(code)];
                    let len = usize::from(sym.len);
                    if s.len() >= len && s[..len] == sym.bytes[..len] {
                        return Some((code, len));
                    }
                }
            }
            let code = self.single[usize::from(*s.first()?)];
            (code != ESCAPE).then_some((code, 1))
        }

        /// Appends the codes of `s` to `out`.
        pub fn encode(&self, s: &[u8], out: &mut Vec<u8>) {
            let mut at = 0;
            while let Some(rest) = s.get(at..).filter(|r| !r.is_empty()) {
                if let Some((code, len)) = self.longest(rest) {
                    out.push(code);
                    at += len;
                } else {
                    out.extend_from_slice(&[ESCAPE, rest[0]]);
                    at += 1;
                }
            }
        }

        /// The encoded length of `s`, without writing it.
        #[must_use]
        pub fn encoded_len(&self, s: &[u8]) -> usize {
            let (mut at, mut n) = (0, 0);
            while let Some(rest) = s.get(at..).filter(|r| !r.is_empty()) {
                let (len, codes) = self.longest(rest).map_or((1, 2), |(_, len)| (len, 1));
                (at, n) = (at + len, n + codes);
            }
            n
        }

        /// The table in its byte layout, for [`Symbols::parse`](super::Symbols::parse).
        #[must_use]
        pub fn to_bytes(&self) -> Vec<u8> {
            let n = self.syms.len();
            let mut out = vec![0; HEADER + pad8(n) + 8 * n];
            #[allow(clippy::cast_possible_truncation, reason = "at most 255 symbols")]
            {
                out[0] = n as u8;
            }
            for (i, s) in self.syms.iter().enumerate() {
                out[HEADER + i] = s.len;
                let at = HEADER + pad8(n) + 8 * i;
                out[at..at + 8].copy_from_slice(&s.bytes);
            }
            out
        }

        /// One training generation: compress the sample, count symbols and adjacent pairs, and
        /// keep the 255 candidates of highest gain (count × length).
        fn next_generation(&self, sample: &[&[u8]]) -> Self {
            // Codes 0..255 are symbols, 256 + b the literal byte b.
            const CODES: usize = 512;
            let mut one = vec![0_u64; CODES];
            let mut two = vec![0_u64; CODES * CODES];
            for s in sample {
                let (mut at, mut prev) = (0, None::<usize>);
                while let Some(rest) = s.get(at..).filter(|r| !r.is_empty()) {
                    let (code, len) = match self.longest(rest) {
                        Some((c, len)) => (usize::from(c), len),
                        None => (256 + usize::from(rest[0]), 1),
                    };
                    one[code] += 1;
                    if let Some(p) = prev {
                        two[p * CODES + code] += 1;
                    }
                    prev = Some(code);
                    at += len;
                }
            }
            let sym_of = |code: usize| -> Sym {
                if code >= 256 {
                    #[allow(clippy::cast_possible_truncation, reason = "a literal byte")]
                    Sym::of(&[(code - 256) as u8])
                } else {
                    self.syms[code]
                }
            };
            let mut gains: Vec<(Sym, u64)> = Vec::new();
            for (code, &n) in one.iter().enumerate() {
                if n > 0 {
                    let s = sym_of(code);
                    gains.push((s, n * u64::from(s.len)));
                }
            }
            for (i, &n) in two.iter().enumerate() {
                if n > 0 {
                    let s = Sym::concat(sym_of(i / CODES), sym_of(i % CODES));
                    gains.push((s, n * u64::from(s.len)));
                }
            }
            // Merge equal symbols, then take the best: highest gain, ties by bytes.
            gains.sort_unstable_by_key(|g| g.0);
            let mut merged: Vec<(Sym, u64)> = Vec::with_capacity(gains.len());
            for (s, g) in gains {
                match merged.last_mut() {
                    Some(last) if last.0 == s => last.1 += g,
                    _ => merged.push((s, g)),
                }
            }
            merged.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            merged.truncate(MAX_SYMBOLS);
            Self::new(merged.into_iter().map(|m| m.0).collect())
        }
    }

    /// Trains a table on `strings`: the paper's five generations on a sample of about 1 MiB
    /// taken at an even stride. Deterministic: the same strings in the same order give the same
    /// table.
    #[must_use]
    pub fn train<S: AsRef<[u8]>>(strings: &[S]) -> Encoder {
        let total: usize = strings.iter().map(|s| s.as_ref().len()).sum();
        let stride = total.div_ceil(SAMPLE).max(1);
        let sample: Vec<&[u8]> = strings
            .iter()
            .step_by(stride)
            .map(AsRef::as_ref)
            .filter(|s| !s.is_empty())
            .collect();
        let mut enc = Encoder::new(Vec::new());
        for _ in 0..GENERATIONS {
            enc = enc.next_generation(&sample);
        }
        enc
    }
}

#[cfg(test)]
#[allow(
    clippy::cast_possible_truncation,
    clippy::indexing_slicing,
    reason = "test inputs are cut from a 64-bit generator on purpose; buffers sized by the test"
)]
mod tests {
    use super::build::train;
    use super::{FsstError, Symbols};
    use std::string::String;
    use std::vec::Vec;

    fn rng(s: &mut u64) -> u64 {
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        *s
    }

    fn corpus() -> Vec<String> {
        let parts = [
            "Kath",
            "mandu",
            "Pokh",
            "ara",
            "Lalit",
            "pur",
            "Bhakta",
            "Chowk",
            "Tole",
            "Bazar",
            "Gaun",
            "पालिका",
            "नगर",
            "गाउँ",
            "Khola",
            "Danda",
            " ",
            "-",
        ];
        let mut s = 5;
        (0..4000)
            .map(|_| {
                (0..=rng(&mut s) % 4)
                    .map(|_| parts[(rng(&mut s) % parts.len() as u64) as usize])
                    .collect()
            })
            .collect()
    }

    fn decode_all(t: &Symbols<'_>, codes: &[u8]) -> Vec<u8> {
        let mut out = std::vec![0; Symbols::max_decoded_len(codes)];
        let n = t.decode(codes, &mut out).unwrap_or_else(|e| panic!("{e}"));
        out.truncate(n);
        out
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "trains tables on a corpus twice: hours under Miri; smaller tests here and libFuzzer cover it"
    )]
    fn round_trips_and_compresses() {
        let strings = corpus();
        let enc = train(&strings);
        assert_eq!(enc, train(&strings), "training is deterministic");
        let bytes = enc.to_bytes();
        let t = Symbols::parse(&bytes).unwrap_or_else(|e| panic!("{e}"));
        assert!(t.len() > 50 && t.len() <= 255);
        let (mut raw, mut packed) = (0, 0);
        // The corpus, unseen text, every byte value, and the empty string.
        let mut others: Vec<Vec<u8>> = strings.iter().map(|s| s.as_bytes().to_vec()).collect();
        others.push((0..=255).collect());
        others.push(Vec::new());
        others.push("Zürich Ünïcödé ∑".as_bytes().to_vec());
        for s in &others {
            let mut codes = Vec::new();
            enc.encode(s, &mut codes);
            assert_eq!(codes.len(), enc.encoded_len(s));
            assert_eq!(decode_all(&t, &codes), *s);
            // An exact-size buffer works too (the short tail path).
            let mut exact = std::vec![0; s.len()];
            assert_eq!(t.decode(&codes, &mut exact), Ok(s.len()));
            assert_eq!(&exact, s);
            if !s.is_empty() {
                let mut short = std::vec![0; s.len() - 1];
                assert_eq!(t.decode(&codes, &mut short), Err(FsstError::Space));
            }
            raw += s.len();
            packed += codes.len();
        }
        // A golden size: training is deterministic, so any change to it shows here. A worse
        // trainer (a lost count, a lost candidate) codes this corpus longer.
        assert_eq!((packed, raw), (11_731, 55_291));
        // A table with no symbols escapes everything.
        let empty = train::<&[u8]>(&[]);
        let eb = empty.to_bytes();
        let et = Symbols::parse(&eb).unwrap_or_else(|e| panic!("{e}"));
        assert!(et.is_empty());
        let mut codes = Vec::new();
        empty.encode(b"abc", &mut codes);
        assert_eq!(codes, [255, b'a', 255, b'b', 255, b'c']);
        assert_eq!(decode_all(&et, &codes), b"abc");
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "exhaustive sizes: hours under Miri; smaller tests here and libFuzzer cover it"
    )]
    fn rejects() {
        let enc = train(&corpus());
        let good = enc.to_bytes();
        let n = usize::from(good[0]);
        assert!(Symbols::parse(&good).is_ok());
        assert_eq!(Symbols::parse(&good[..7]).err(), Some(FsstError::Length));
        assert_eq!(
            Symbols::parse(&good[..good.len() - 1]).err(),
            Some(FsstError::Length)
        );
        let syms = 8 + n.div_ceil(8) * 8;
        let mut longer = good.clone();
        longer.extend_from_slice(&[0; 8]);
        assert_eq!(Symbols::parse(&longer).err(), Some(FsstError::Length));
        // A symbol of length 0 whose bytes are all zero: still refused.
        let mut zero = good.clone();
        zero[8] = 0;
        zero[syms..syms + 8].fill(0);
        assert_eq!(Symbols::parse(&zero).err(), Some(FsstError::Table));
        let mut cases = Vec::new();
        cases.push((3, 1)); // reserved
        cases.push((8, 0)); // length 0
        cases.push((8, 9)); // length 9
        if n % 8 != 0 {
            cases.push((8 + n, 1)); // padding
        }
        // A byte past the length of the shortest symbol (the last one).
        let last_len = usize::from(good[8 + n - 1]);
        cases.push((syms + 8 * (n - 1) + last_len, 1));
        for (at, v) in cases {
            let mut b = good.clone();
            b[at] = v;
            assert_eq!(
                Symbols::parse(&b).err(),
                Some(FsstError::Table),
                "byte {at}"
            );
        }
        let t = Symbols::parse(&good).unwrap_or_else(|e| panic!("{e}"));
        let mut out = [0; 64];
        assert_eq!(t.decode(&[255], &mut out), Err(FsstError::Code));
        if n < 255 {
            assert_eq!(t.decode(&[n as u8], &mut out), Err(FsstError::Code));
        }
        assert_eq!(t.symbol(n as u8), None);
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "exhaustive sizes: hours under Miri; smaller tests here and libFuzzer cover it"
    )]
    fn never_panics() {
        let enc = train(&corpus());
        let base = enc.to_bytes();
        let mut s = 77;
        let mut out = [0; 300];
        for _ in 0..crate::cases(20_000) {
            let mut b = base.clone();
            let at = rng(&mut s) as usize % b.len();
            b[at] = rng(&mut s) as u8;
            if let Ok(t) = Symbols::parse(&b) {
                let codes: Vec<u8> = (0..rng(&mut s) % 40).map(|_| rng(&mut s) as u8).collect();
                let len = rng(&mut s) as usize % out.len();
                let _ = t.decode(&codes, &mut out[..len]);
            }
        }
    }
}
