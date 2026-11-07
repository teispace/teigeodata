//! SEARCH against brute force over its input: exact keys, prefix ranges, best-first ranges,
//! fuzzy walks, list order and importance; then the structural refusals and determinism.
#![cfg(feature = "write")]
#![allow(
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::panic,
    clippy::needless_range_loop,
    clippy::range_plus_one,
    reason = "tests: inputs are cut from a 64-bit generator; a failure panics with its message"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

use teigeodata_core::bitpack::{Coding, encode as pack_col, encode_direct};
use teigeodata_core::table::{self, Codec};
use teigeodata_format::search::fuzzy::Fuzzy;
use teigeodata_format::search::{self, EXACT, LOOSE, Search};
use teigeodata_format::{Encoding, Error, Kind, Pack, Writer};

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

type Keys = BTreeMap<Vec<u8>, BTreeSet<u32>>;

/// Places with names built from shared parts (many names repeat), each in the exact map and,
/// without spaces, the loose map.
fn input(r: &mut Rng, rows: usize) -> (Keys, Vec<u8>) {
    const PARTS: [&str; 12] = [
        "ram",
        "pur",
        "kath",
        "mandu",
        "nagar",
        " ",
        "gaun",
        "a",
        "क",
        "पुर",
        "ि",
        "bhaktapur",
    ];
    let mut keys = Keys::new();
    let importance: Vec<u8> = (0..rows).map(|_| r.next() as u8).collect();
    for row in 0..rows as u32 {
        for _ in 0..1 + r.below(2) {
            let name: String = (0..1 + r.below(3))
                .map(|_| PARTS[r.below(PARTS.len())])
                .collect();
            let name = name.trim().to_string();
            if name.is_empty() {
                continue;
            }
            for (tag, k) in [(EXACT, name.clone()), (LOOSE, name.replace(' ', ""))] {
                let mut key = vec![tag];
                key.extend_from_slice(k.as_bytes());
                keys.entry(key).or_default().insert(row);
            }
        }
    }
    (keys, importance)
}

fn with_body(body: &[u8], rows: u64) -> Vec<u8> {
    Writer::new("np", "search", "2026.11.0")
        .unwrap()
        .pack(&[(Kind::SEARCH, Encoding::TABLE, rows, body)], None)
        .unwrap()
}

fn open(bytes: &[u8]) -> Result<Search<'_>, Error> {
    let pack = Pack::open(bytes)?;
    Search::open(&pack.section(Kind::SEARCH).unwrap())
}

fn osa(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0_usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let mut v = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + usize::from(a[i - 1] != b[j - 1]));
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    d[a.len()][b.len()]
}

/// A list as the writer orders it.
fn ordered(rows: &BTreeSet<u32>, importance: &[u8]) -> Vec<u32> {
    let mut v: Vec<u32> = rows.iter().copied().collect();
    v.sort_by(|a, b| {
        importance[*b as usize]
            .cmp(&importance[*a as usize])
            .then(a.cmp(b))
    });
    v
}

#[test]
fn queries_agree_with_brute_force() {
    let mut r = Rng(17);
    for rows in [1, 7, 300, 3000] {
        let (keys, importance) = input(&mut r, rows);
        let body = search::encode(&keys, &importance).unwrap();
        let bytes = with_body(&body, rows as u64);
        let ix = open(&bytes).unwrap();
        ix.check().unwrap();
        assert_eq!(ix.keys() as usize, keys.len());
        let all: Vec<(&Vec<u8>, &BTreeSet<u32>)> = keys.iter().collect();
        // Every key: its ordinal, its list in order, the key maximum.
        for (o, (k, set)) in all.iter().enumerate() {
            let text = std::str::from_utf8(&k[1..]).unwrap();
            assert_eq!(ix.get(k[0], text), Some(o as u32));
            let mut got = Vec::new();
            ix.rows_of(o as u32, |row| {
                got.push(row);
                ControlFlow::Continue(())
            });
            let want = ordered(set, &importance);
            assert_eq!(got, want);
            assert_eq!(ix.key_max(o as u32), importance[want[0] as usize]);
        }
        for (row, &imp) in importance.iter().enumerate() {
            assert_eq!(ix.importance(row as u32), imp);
        }
        // Prefix ranges, and best-first scans of them against a full scan.
        for p in ["", "r", "ram", "kath", "क", "zz", "bhaktapur", "pura"] {
            for tag in [EXACT, LOOSE] {
                let range = ix.range(tag, p);
                let lo = all.partition_point(|(k, _)| {
                    k[0] < tag || (k[0] == tag && &k[1..] < p.as_bytes())
                });
                let hi = lo
                    + all[lo..]
                        .iter()
                        .take_while(|(k, _)| k[0] == tag && k[1..].starts_with(p.as_bytes()))
                        .count();
                assert_eq!(range, lo as u32..hi as u32, "{tag} {p:?}");
                for floor in [-1, 100, 200, 254] {
                    let mut seen = Vec::new();
                    ix.best_in(
                        range.clone(),
                        || floor,
                        |o| {
                            seen.push(o);
                            ControlFlow::Continue(())
                        },
                    );
                    let want: Vec<u32> = range
                        .clone()
                        .filter(|&o| i32::from(ix.key_max(o)) > floor)
                        .collect();
                    assert_eq!(seen, want, "{p:?} floor {floor}");
                }
            }
        }
        // Fuzzy walks against brute force over the exact map.
        for q in ["rampur", "kathmandu", "pura", "कपुर", "bhaktpur", "nagr"] {
            for edits in 0..=2 {
                let fz = Fuzzy::new(q, edits).unwrap();
                let mut got = Vec::new();
                ix.walk(EXACT, &fz, None, usize::MAX, |o, e| {
                    got.push((o, e));
                    ControlFlow::Continue(())
                });
                let want: Vec<(u32, u8)> = all
                    .iter()
                    .enumerate()
                    .filter(|(_, (k, _))| k[0] == EXACT)
                    .filter_map(|(o, (k, _))| {
                        let d = osa(q, std::str::from_utf8(&k[1..]).unwrap());
                        (d <= usize::from(edits)).then_some((o as u32, d as u8))
                    })
                    .collect();
                assert_eq!(got, want, "{q} {edits}");
            }
        }
    }
}

#[test]
fn the_encoding_is_deterministic_and_refuses_bad_input() {
    let mut r = Rng(3);
    let (keys, importance) = input(&mut r, 200);
    assert_eq!(
        search::encode(&keys, &importance).unwrap(),
        search::encode(&keys, &importance).unwrap()
    );
    let mut empty = keys.clone();
    empty.insert(vec![EXACT, b'z'], BTreeSet::new());
    assert!(search::encode(&empty, &importance).is_err());
    let mut past = keys.clone();
    past.insert(vec![EXACT, b'z'], BTreeSet::from([200]));
    assert!(search::encode(&past, &importance).is_err());
}

/// Rebuilds a body from columns: (id, codec, bytes).
fn rebuild(cols: &[(u16, Codec, Vec<u8>)]) -> Vec<u8> {
    let refs: Vec<(u16, Codec, &[u8])> = cols
        .iter()
        .map(|(i, c, b)| (*i, *c, b.as_slice()))
        .collect();
    table::encode_with(&refs).unwrap()
}

/// The columns of a body, decoded.
struct Cols {
    length: Vec<u32>,
    automaton: Vec<u8>,
    starts: Vec<u32>,
    ids: Vec<u32>,
    imp: Vec<u32>,
    bmax: Vec<u32>,
}

impl Cols {
    fn of(body: &[u8]) -> Self {
        let t = table::Table::parse(body).unwrap();
        let decode = |id: u16| {
            let c = t.column(id).unwrap();
            let mut v = vec![0; c.len()];
            c.decode_all(&mut v);
            v
        };
        Self {
            length: decode(search::LENGTH),
            automaton: t.bytes(search::AUTOMATON).unwrap().to_vec(),
            starts: decode(search::STARTS),
            ids: decode(search::IDS),
            imp: decode(search::IMPORTANCE),
            bmax: decode(search::BLOCK_MAX),
        }
    }

    fn body(&self) -> Vec<u8> {
        rebuild(&[
            (search::LENGTH, Codec::Bitpack, encode_direct(&self.length)),
            (search::AUTOMATON, Codec::Bytes, self.automaton.clone()),
            (
                search::STARTS,
                Codec::Bitpack,
                pack_col(&self.starts, Coding::Delta),
            ),
            (search::IDS, Codec::Bitpack, encode_direct(&self.ids)),
            (search::IMPORTANCE, Codec::Bitpack, encode_direct(&self.imp)),
            (search::BLOCK_MAX, Codec::Bitpack, encode_direct(&self.bmax)),
        ])
    }
}

/// The problem `open` then `check` report, if any.
fn problem(body: &[u8], rows: u64) -> Option<&'static str> {
    match open(&with_body(body, rows)).and_then(|ix| ix.check()) {
        Ok(()) => None,
        Err(Error::Payload { problem, .. }) => Some(problem),
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn bad_columns_are_refused() {
    let mut r = Rng(9);
    // An input whose automaton length is not a multiple of 4, so it has padding.
    let (keys, importance, body) = (300..400)
        .map(|n| {
            let (k, i) = input(&mut r, n);
            let b = search::encode(&k, &i).unwrap();
            (k, i, b)
        })
        .find(|(_, _, b)| !Cols::of(b).length[0].is_multiple_of(4))
        .unwrap();
    let rows = importance.len() as u64;
    let good = Cols::of(&body);
    assert_eq!(problem(&good.body(), rows), None);
    let with = |f: &dyn Fn(&mut Cols)| {
        let mut c = Cols::of(&body);
        f(&mut c);
        problem(&c.body(), rows)
    };
    // A list of two rows of different importance, swapped.
    let two = (0..good.starts.len() - 1)
        .find(|&o| {
            let (a, b) = (good.starts[o] as usize, good.starts[o + 1] as usize);
            b - a >= 2 && good.imp[good.ids[a] as usize] != good.imp[good.ids[a + 1] as usize]
        })
        .unwrap();
    let a = good.starts[two] as usize;
    let order = Some("a list is not in importance order");
    assert_eq!(with(&|c| c.ids.swap(a, a + 1)), order);
    // Rows of equal importance must be in row order: give the two the same importance.
    assert_eq!(
        with(&|c| {
            let (x, y) = (c.ids[a] as usize, c.ids[a + 1] as usize);
            c.imp[y] = c.imp[x];
            c.ids.swap(a, a + 1);
        }),
        order
    );
    assert_eq!(
        with(&|c| c.ids[0] = rows as u32),
        Some("a list names a row past the end")
    );
    assert_eq!(
        with(&|c| c.imp[0] = 256),
        Some("an importance is above 255")
    );
    assert_eq!(with(&|c| c.bmax[0] += 1), Some("a block maximum is wrong"));
    // Too low drops results; too high only costs reads; both are refused.
    assert!(good.bmax[0] > 0);
    assert_eq!(with(&|c| c.bmax[0] -= 1), Some("a block maximum is wrong"));
    if let Some(b) = good.bmax.iter().position(|&m| m < 255) {
        assert_eq!(with(&|c| c.bmax[b] += 1), Some("a block maximum is wrong"));
    }
    // An empty list: the last key's list ends where it starts, its entry dropped.
    assert_eq!(
        with(&|c| {
            let k = c.starts.len() - 2;
            let s = c.starts[k] as usize;
            c.ids.truncate(s);
            c.starts[k + 1] = c.starts[k];
        }),
        Some("a list is empty or the starts decrease")
    );
    assert_eq!(
        with(&|c| c.length[0] += 4),
        Some("the automaton length differs from its column")
    );
    assert_eq!(
        with(&|c| *c.automaton.last_mut().unwrap() = 1),
        Some("the automaton is empty or its padding is not zero")
    );
    let mut refused = 0;
    for _ in 0..200 {
        let i = r.below(good.length[0] as usize);
        let bit = 1 << r.below(8);
        refused += usize::from(with(&|c| c.automaton[i] ^= bit).is_some());
    }
    // A flipped label bit can leave a valid automaton of other keys in the same order: no
    // structural check sees that, the signature guards it. Most flips break the structure.
    assert!(
        refused > 100,
        "{refused} of 200 automaton bit flips refused"
    );
    let _ = keys;
}

#[test]
fn loose_keys_keep_only_rows_their_exact_twin_lacks() {
    let set = |v: &[u32]| v.iter().copied().collect::<BTreeSet<u32>>();
    let mut m = search::Maps::default();
    m.exact.insert("pokhara".into(), set(&[0, 1]));
    m.loose.insert("pokhara".into(), set(&[0, 1, 2]));
    m.loose.insert("lahan".into(), set(&[1]));
    m.loose.insert(String::new(), set(&[0]));
    m.phonetic.insert("pkr".into(), set(&[0, 2]));
    let body = search::encode_maps(&m, &[3, 2, 1]).unwrap();
    let file = with_body(&body, 3);
    let ix = open(&file).unwrap();
    let rows = |tag, key| {
        let mut out = Vec::new();
        if let Some(o) = ix.get(tag, key) {
            ix.rows_of(o, |r| {
                out.push(r);
                ControlFlow::Continue(())
            });
        }
        out
    };
    assert_eq!(rows(EXACT, "pokhara"), [0, 1]);
    assert_eq!(
        rows(LOOSE, "pokhara"),
        [2],
        "the twin's rows are not repeated"
    );
    assert_eq!(rows(LOOSE, "lahan"), [1]);
    assert_eq!(rows(search::PHONETIC, "pkr"), [0, 2]);
    assert_eq!(ix.keys(), 4, "the empty key is skipped");
}
