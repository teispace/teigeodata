//! `ARCS_OVERVIEW` (D-102): the coarse levels of ARCS alone. Written from arcs with levels (some
//! longer than a block, some on a coarser grid, some closed), it reads like ARCS at its level
//! and coarser, in both directions; every edit of a value its checks own is refused.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::many_single_char_names,
    reason = "test code: model values are small, indices come from the same vectors"
)]

use teigeodata_core::bitpack::{Coding, Packed, encode as pack};
use teigeodata_core::table::{Table, encode as table};
use teigeodata_core::{E7, Point};
use teigeodata_format::arcs::{self, ArcColumns, Arcs, DX, DY, FROM, LEVEL, PINNED, TOLERANCE};
use teigeodata_format::{Kind, Section};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const TOLERANCES: [u32; 6] = [500, 2_000, 10_000, 50_000, 200_000, 500_000];

/// Arcs of random walks with random levels, ends pinned: every fifth longer than two blocks,
/// every seventh on a 1,000-unit grid, every ninth closed with a pinned vertex inside.
fn model(seed: u64) -> (Vec<Vec<Point<E7>>>, Vec<Vec<u8>>) {
    let mut r = Rng(seed);
    let (mut arcs, mut levels) = (Vec::new(), Vec::new());
    for a in 0..40 {
        let n = if a % 5 == 0 {
            300
        } else {
            2 + r.below(40) as usize
        };
        let step = if a % 7 == 0 { 1_000 } else { 1 };
        let (mut x, mut y) = (850_000_000 + a * 1_000_000, 270_000_000);
        let mut pts = Vec::new();
        while pts.len() < n {
            x += (r.below(2_001) as i32 - 1_000) * step;
            y += (r.below(2_001) as i32 - 1_000) * step;
            let p = Point::<E7>::from_units(y, x).unwrap();
            if pts.last() != Some(&p) {
                pts.push(p);
            }
        }
        let mut lv: Vec<u8> = (0..n).map(|_| r.below(7) as u8).collect();
        if a % 9 == 4 {
            pts.push(pts[0]);
            lv.push(0);
            lv[n / 2] = PINNED;
        }
        let last = lv.len() - 1;
        lv[0] = PINNED;
        lv[last] = PINNED;
        arcs.push(pts);
        levels.push(lv);
    }
    (arcs, levels)
}

fn full_body() -> Vec<u8> {
    let (pts, lvs) = model(7);
    let pr: Vec<&[Point<E7>]> = pts.iter().map(Vec::as_slice).collect();
    let lr: Vec<&[u8]> = lvs.iter().map(Vec::as_slice).collect();
    arcs::encode(&ArcColumns {
        arcs: &pr,
        levels: &lr,
        tolerances_cm: &TOLERANCES,
    })
    .unwrap()
}

fn open(kind: Kind, items: usize, body: &[u8]) -> Result<Arcs<'_>, teigeodata_format::Error> {
    Arcs::open(&Section::unpacked(kind, items as u64, body))
}

#[test]
fn an_overview_reads_like_its_arcs() {
    let body = full_body();
    let full = open(Kind::ARCS, 40, &body).unwrap();
    full.check().unwrap();
    assert_eq!(full.overview_from(), None);
    for from in 1..PINNED {
        let ob = arcs::overview(&full, from).unwrap();
        let ov = open(Kind::ARCS_OVERVIEW, 40, &ob).unwrap();
        ov.check().unwrap();
        ov.check_overview(&full).unwrap();
        assert_eq!(ov.overview_from(), Some(from));
        let kept: usize = (0..40).map(|a| full.count(a, from)).sum();
        assert_eq!(ov.vertices(), kept, "level {from}");
        for a in 0..40 {
            for lod in from..PINNED {
                for reversed in [false, true] {
                    assert!(
                        full.points(a, reversed, lod)
                            .eq(ov.points(a, reversed, lod)),
                        "arc {a} level {lod}"
                    );
                }
            }
        }
    }
    // Not an overview of an overview, nor one at level 0 or 7; an ARCS is no overview.
    let ob = arcs::overview(&full, 3).unwrap();
    let ov = open(Kind::ARCS_OVERVIEW, 40, &ob).unwrap();
    assert!(arcs::overview(&ov, 4).is_err());
    assert!(arcs::overview(&full, 0).is_err());
    assert!(arcs::overview(&full, PINNED).is_err());
    assert!(full.check_overview(&full).is_err());
    assert!(ov.check_overview(&ov).is_err());
    // An overview's FROM is refused in ARCS, and required in an overview.
    assert!(open(Kind::ARCS, 40, &ob).is_err());
    assert!(open(Kind::ARCS_OVERVIEW, 40, &body).is_err());
}

type Column = (u16, Vec<u32>);

fn columns(body: &[u8]) -> Vec<Column> {
    let t = Table::parse(body).unwrap();
    t.ids()
        .map(|id| {
            let c: Packed<'_> = t.column(id).unwrap();
            let mut v = vec![0; c.len()];
            c.decode_all(&mut v);
            (id, v)
        })
        .collect()
}

fn rebuild(cols: &[Column]) -> Vec<u8> {
    let enc: Vec<(u16, Vec<u8>)> = cols
        .iter()
        .map(|(id, v)| (*id, pack(v, Coding::For)))
        .collect();
    let refs: Vec<(u16, &[u8])> = enc.iter().map(|(id, b)| (*id, b.as_slice())).collect();
    table(&refs).unwrap()
}

#[test]
fn every_edit_of_an_overview_is_refused() {
    let body = full_body();
    let full = open(Kind::ARCS, 40, &body).unwrap();
    let from = 3;
    let cols = columns(&arcs::overview(&full, from).unwrap());
    let refused = |cols: &[Column]| {
        let b = rebuild(cols);
        open(Kind::ARCS_OVERVIEW, 40, &b)
            .and_then(|ov| ov.check().and_then(|()| ov.check_overview(&full)))
            .is_err()
    };
    assert!(!refused(&cols), "the unedited overview passes");
    let at = |id: u16| cols.iter().position(|(c, _)| *c == id).unwrap();
    // Each delta moved (a vertex moved, and the checkpoints after it kept), each tolerance.
    for id in [DX, DY, TOLERANCE] {
        let c = at(id);
        for j in 0..cols[c].1.len() {
            let mut bad = cols.clone();
            bad[c].1[j] ^= 2;
            assert!(refused(&bad), "column {id} value {j} passed");
        }
    }
    // Each level: one finer than the overview's (below FROM), or another coarse one.
    let c = at(LEVEL);
    for j in 0..cols[c].1.len() {
        for v in [u32::from(from) - 1, (cols[c].1[j] + 1) % 7 + 1] {
            if v == cols[c].1[j] {
                continue;
            }
            let mut bad = cols.clone();
            bad[c].1[j] = v;
            assert!(refused(&bad), "level {j} = {v} passed");
        }
    }
    // A level finer than FROM is refused by the overview's own check, without ARCS.
    let mut fine = cols.clone();
    let j = fine[c]
        .1
        .iter()
        .position(|&l| l != u32::from(PINNED))
        .unwrap();
    fine[c].1[j] = u32::from(from) - 1;
    let b = rebuild(&fine);
    assert!(open(Kind::ARCS_OVERVIEW, 40, &b).unwrap().check().is_err());
    // Another FROM; 0 and 7 are refused on opening.
    for f in [from - 1, from + 1] {
        let mut bad = cols.clone();
        bad[at(FROM)].1[0] = u32::from(f);
        assert!(refused(&bad), "FROM {f} passed");
    }
    for f in [0, PINNED] {
        let mut bad = cols.clone();
        bad[at(FROM)].1[0] = u32::from(f);
        assert!(
            open(Kind::ARCS_OVERVIEW, 40, &rebuild(&bad)).is_err(),
            "FROM {f} opened"
        );
    }
}
