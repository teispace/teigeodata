//! Fuzzy matching of a query against keys read one character at a time, for walks over a
//! sorted key dictionary: the optimal string alignment (OSA) distance, which is Levenshtein
//! plus one transposition of adjacent characters, up to [`MAX_EDITS`].
//!
//! The automaton is a bit-parallel NFA (Wu and Manber's shift-and with errors, Baeza-Yates and
//! Navarro's layout): one 64-bit word per error count, bit `j` set when the first `j` query
//! characters align with the key read so far. A step is a handful of word operations,
//! whatever the query length, and a [`State`] is a few words that a walk keeps per depth.
//!
//! Distances count characters, not bytes, so an edit costs the same in every script
//! (principle P16): a Devanagari letter is three UTF-8 bytes and one edit.

/// The most edits a query allows.
pub const MAX_EDITS: u8 = 2;
/// The longest query in characters: bit `len` of a 64-bit word is the accepting position.
pub const MAX_CHARS: usize = 63;
/// The distinct non-ASCII characters a query may hold; ASCII has its own table.
const OTHER: usize = MAX_CHARS;

/// A query compiled for matching: the positions of each of its characters.
#[derive(Clone, Debug)]
pub struct Fuzzy {
    ascii: [u64; 128],
    other: [(u32, u64); OTHER],
    others: usize,
    len: u32,
    edits: u8,
}

/// The automaton's state after some characters of a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    /// `r[e]` bit `j`: the first `j` query characters align with `e` edits.
    r: [u64; 3],
    /// `t[e]` bit `j`: as `r[e]` before the last character, which was query character `j + 1`,
    /// so query character `j` next completes a transposition.
    t: [u64; 2],
}

// The step is written out for each error level up to two.
const _: () = assert!(MAX_EDITS == 2);

impl Fuzzy {
    /// Compiles `query` for at most `edits` edits (capped at [`MAX_EDITS`]); `None` if it has
    /// more than [`MAX_CHARS`] characters.
    #[must_use]
    pub fn new(query: &str, edits: u8) -> Option<Self> {
        let mut f = Self {
            ascii: [0; 128],
            other: [(0, 0); OTHER],
            others: 0,
            len: 0,
            edits: edits.min(MAX_EDITS),
        };
        for (i, c) in query.chars().enumerate() {
            if i >= MAX_CHARS {
                return None;
            }
            // Bit i + 1: matching this character advances from position i.
            let bit = 2_u64 << i;
            let code = u32::from(c);
            if let Some(slot) = f.ascii.get_mut(code as usize) {
                *slot |= bit;
            } else if let Some(e) = f.other.iter_mut().take(f.others).find(|e| e.0 == code) {
                e.1 |= bit;
            } else {
                // At most MAX_CHARS distinct characters, so a slot is free.
                if let Some(e) = f.other.get_mut(f.others) {
                    *e = (code, bit);
                    f.others += 1;
                }
            }
            f.len += 1;
        }
        Some(f)
    }

    /// The query length in characters.
    #[must_use]
    pub const fn len(&self) -> u32 {
        self.len
    }

    /// Whether the query is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The edits allowed.
    #[must_use]
    pub const fn edits(&self) -> u8 {
        self.edits
    }

    const fn all(&self) -> u64 {
        // Bits 0..=len, len ≤ 63.
        u64::MAX >> (63 - self.len)
    }

    fn positions(&self, c: char) -> u64 {
        let code = u32::from(c);
        match self.ascii.get(code as usize) {
            Some(&m) => m,
            None => self
                .other
                .iter()
                .take(self.others)
                .find(|e| e.0 == code)
                .map_or(0, |e| e.1),
        }
    }

    /// The state before any key character: position 0, and deletions of query characters.
    #[must_use]
    pub fn start(&self) -> State {
        // `r[e]` holds every position within `e` edits, so it contains `r[e - 1]` (one step
        // restores that anyway, through the substitution move).
        let all = self.all();
        let r1 = if self.edits >= 1 { 3 & all } else { 0 };
        let r2 = if self.edits >= 2 { 7 & all } else { 0 };
        State {
            r: [1, r1, r2],
            t: [0, 0],
        }
    }

    /// One error level: from `r` (this level), `old` and `lower` (the level below, before the
    /// step) and `low` (the level below, after it).
    #[inline]
    const fn level(r: u64, old: u64, lower: u64, low: u64, b: u64, all: u64) -> u64 {
        (((r << 1) & b)
            // `c` is an extra key character, or replaces the next query character.
            | old
            | (old << 1)
            // `c` completes a transposition.
            | ((lower << 2) & (b << 1))
            // Deleting the next query character.
            | (low << 1))
            & all
    }

    /// The state after key character `c`.
    #[must_use]
    pub fn step(&self, s: &State, c: char) -> State {
        let b = self.positions(c);
        let all = self.all();
        let [r0, r1, r2] = s.r;
        let [t0, t1] = s.t;
        let n0 = (r0 << 1) & b;
        // Bit j + 2 of `b`: `c` is query character j + 1, so a transposition may follow.
        let (n1, m0) = if self.edits >= 1 {
            (Self::level(r1, r0, t0, n0, b, all), r0 & (b >> 2))
        } else {
            (0, 0)
        };
        let (n2, m1) = if self.edits >= 2 {
            (Self::level(r2, r1, t1, n1, b, all), r1 & (b >> 2))
        } else {
            (0, 0)
        };
        State {
            r: [n0, n1, n2],
            t: [m0, m1],
        }
    }

    /// The edits with which the key read so far matches the whole query, if any.
    #[must_use]
    pub fn distance(&self, s: &State) -> Option<u8> {
        let end = 1_u64 << self.len;
        let [r0, r1, r2] = s.r;
        if r0 & end != 0 {
            Some(0)
        } else if r1 & end != 0 {
            Some(1)
        } else if r2 & end != 0 {
            Some(2)
        } else {
            None
        }
    }

    /// Whether some continuation of the key read so far can still match. A pending
    /// transposition needs no test of its own: `t[e]` comes from the previous `r[e]`, and the
    /// insertion move carries that `r[e]` into the new `r[e + 1]`.
    #[must_use]
    pub fn alive(&self, s: &State) -> bool {
        s.r[0] | s.r[1] | s.r[2] != 0
    }

    /// The OSA distance of `key` to the query if within the edits: the automaton run on it.
    #[must_use]
    pub fn matches(&self, key: &str) -> Option<u8> {
        let mut s = self.start();
        for c in key.chars() {
            s = self.step(&s, c);
            if !self.alive(&s) {
                return None;
            }
        }
        self.distance(&s)
    }
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::many_single_char_names,
    clippy::needless_range_loop,
    clippy::manual_strip,
    clippy::range_plus_one,
    reason = "tests: the oracles mirror the textbook dynamic program; inputs are cut from a 64-bit generator"
)]
mod tests {
    use std::string::String;
    use std::vec::Vec;

    use super::{Fuzzy, MAX_CHARS};

    /// The OSA distance by dynamic programming, on characters.
    fn osa(a: &str, b: &str) -> usize {
        let a: Vec<char> = a.chars().collect();
        let b: Vec<char> = b.chars().collect();
        let (n, m) = (a.len(), b.len());
        let mut d = std::vec![std::vec![0_usize; m + 1]; n + 1];
        for (i, row) in d.iter_mut().enumerate() {
            row[0] = i;
        }
        for j in 0..=m {
            d[0][j] = j;
        }
        for i in 1..=n {
            for j in 1..=m {
                let cost = usize::from(a[i - 1] != b[j - 1]);
                let mut v = (d[i - 1][j] + 1)
                    .min(d[i][j - 1] + 1)
                    .min(d[i - 1][j - 1] + cost);
                if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                    v = v.min(d[i - 2][j - 2] + 1);
                }
                d[i][j] = v;
            }
        }
        d[n][m]
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    /// A short word over a small alphabet (so edits collide), mixing ASCII and Devanagari.
    fn word(r: &mut Rng, max: usize) -> String {
        const ABC: [char; 6] = ['a', 'b', 'c', 'क', 'ख', 'ा'];
        (0..r.below(max + 1))
            .map(|_| ABC[r.below(ABC.len())])
            .collect()
    }

    /// A key derived from `q` by up to three random edits, so near misses are common.
    fn near(r: &mut Rng, q: &str) -> String {
        let mut v: Vec<char> = q.chars().collect();
        for _ in 0..r.below(4) {
            let i = r.below(v.len() + 1);
            match r.below(4) {
                0 => v.insert(i, 'b'),
                1 if i < v.len() => {
                    v.remove(i);
                }
                2 if i < v.len() => v[i] = 'ख',
                3 if i + 1 < v.len() => v.swap(i, i + 1),
                _ => {}
            }
        }
        v.into_iter().collect()
    }

    #[test]
    fn agrees_with_the_dynamic_program() {
        let mut r = Rng(0x9e37_79b9_7f4a_7c15);
        let cases = if cfg!(miri) { 300 } else { 200_000 };
        for i in 0..cases {
            let q = word(&mut r, 9);
            let key = if i % 2 == 0 {
                near(&mut r, &q)
            } else {
                word(&mut r, 9)
            };
            let d = osa(&q, &key);
            for edits in 0..=2_u8 {
                let f = Fuzzy::new(&q, edits).unwrap();
                let want = (d <= usize::from(edits)).then_some(d as u8);
                assert_eq!(f.matches(&key), want, "{q:?} {key:?} edits {edits}");
            }
        }
    }

    #[test]
    fn pruning_never_drops_a_match() {
        // A dead state has no matching continuation: check by extending dead prefixes.
        let mut r = Rng(7);
        let cases = if cfg!(miri) { 100 } else { 20_000 };
        for _ in 0..cases {
            let q = word(&mut r, 6);
            let f = Fuzzy::new(&q, 1).unwrap();
            let key = word(&mut r, 8);
            let mut s = f.start();
            let mut dead_at = None;
            for (i, c) in key.chars().enumerate() {
                s = f.step(&s, c);
                if !f.alive(&s) {
                    dead_at = Some(i);
                    break;
                }
            }
            if let Some(i) = dead_at {
                let prefix: String = key.chars().take(i + 1).collect();
                for _ in 0..8 {
                    let ext = std::format!("{prefix}{}", word(&mut r, 4));
                    assert!(osa(&q, &ext) > 1, "{q:?} {ext:?}");
                }
            }
        }
    }

    #[test]
    fn edges() {
        let long: String = core::iter::repeat_n('a', MAX_CHARS).collect();
        let f = Fuzzy::new(&long, 2).unwrap();
        assert_eq!(f.matches(&long), Some(0));
        assert_eq!(f.matches(&long[1..]), Some(1));
        assert!(Fuzzy::new(&std::format!("{long}a"), 1).is_none());
        let empty = Fuzzy::new("", 1).unwrap();
        assert!(empty.is_empty());
        assert_eq!(empty.matches(""), Some(0));
        assert_eq!(empty.matches("x"), Some(1));
        assert_eq!(empty.matches("xy"), None);
        assert_eq!(Fuzzy::new("ab", 5).unwrap().edits(), 2);
        // A transposition is one edit; across a Devanagari letter too.
        assert_eq!(
            Fuzzy::new("kispang", 1).unwrap().matches("ksipang"),
            Some(1)
        );
        assert_eq!(
            Fuzzy::new("काठमाडौं", 1).unwrap().matches("काठमाडौ"),
            Some(1)
        );
        assert_eq!(Fuzzy::new("काठमाडौं", 1).unwrap().matches("काठमाडो"), None);
    }
}
