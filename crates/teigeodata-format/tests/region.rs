//! REGION against an independent oracle: an even–odd ray cast over the original rings (not the
//! arcs), with the documented rule for points on a border. Inputs: a jittered grid partition
//! whose neighbours share borders exactly, overlapping star polygons with holes, a ring repeated
//! (it cancels), regions on the domain edges, and a world background. Query points: uniform, on
//! vertices, on edges, 1 unit beside edges, and on the domain edges. Then the structural check,
//! its refusals, and determinism.
#![cfg(feature = "write")]
#![allow(
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::unwrap_used,
    clippy::many_single_char_names,
    clippy::panic,
    reason = "tests: inputs are cut from a 64-bit generator; a failure panics with its message"
)]

use teigeodata_core::table;
use teigeodata_core::{E7, Point};
use teigeodata_format::region::{self, Options, Region};
use teigeodata_format::{Encoding, Error, Kind, Pack, Writer, blocks};

type Rings = Vec<Vec<[i32; 2]>>;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }
}

const X0: i64 = -1_800_000_000;
const X1: i64 = 1_800_000_000;
const Y0: i64 = -900_000_000;
const Y1: i64 = 900_000_000;

/// A grid partition of the box `[x, x + w] × [y, y + h]` into `m × m` regions. The lattice is
/// jittered and every lattice edge gets the same extra vertices for both its regions, so
/// neighbours share their borders exactly.
fn partition(r: &mut Rng, (x, y, w, h): (i64, i64, i64, i64), m: usize) -> Vec<Rings> {
    let jit = |r: &mut Rng, s: i64| r.range(-s / 4, s / 4);
    let (sx, sy) = (w / m as i64, h / m as i64);
    let lat: Vec<Vec<(i64, i64)>> = (0..=m)
        .map(|j| {
            (0..=m)
                .map(|i| {
                    let inner = |k: usize| k > 0 && k < m;
                    let dx = if inner(i) { jit(r, sx) } else { 0 };
                    let dy = if inner(j) { jit(r, sy) } else { 0 };
                    (x + i as i64 * sx + dx, y + j as i64 * sy + dy)
                })
                .collect()
        })
        .collect();
    // Extra vertices along a lattice edge, from its first end to its second.
    let mut extra = std::collections::HashMap::new();
    let mut along = |r: &mut Rng, a: (i64, i64), b: (i64, i64)| -> Vec<(i64, i64)> {
        let key = if a < b { (a, b) } else { (b, a) };
        let pts = extra
            .entry(key)
            .or_insert_with(|| {
                (1..r.range(1, 4))
                    .map(|k| {
                        let t = k as f64 / 4.0;
                        let (px, py) = (
                            key.0.0 + ((key.1.0 - key.0.0) as f64 * t) as i64,
                            key.0.1 + ((key.1.1 - key.0.1) as f64 * t) as i64,
                        );
                        (
                            (px + r.range(-50, 50)).clamp(X0, X1),
                            (py + r.range(-50, 50)).clamp(Y0, Y1),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .clone();
        if a < b {
            pts
        } else {
            pts.into_iter().rev().collect()
        }
    };
    let mut out = Vec::new();
    for j in 0..m {
        for i in 0..m {
            let c = [lat[j][i], lat[j][i + 1], lat[j + 1][i + 1], lat[j + 1][i]];
            let mut ring = Vec::new();
            for k in 0..4 {
                let (a, b) = (c[k], c[(k + 1) % 4]);
                ring.push([a.0 as i32, a.1 as i32]);
                for p in along(r, a, b) {
                    ring.push([p.0 as i32, p.1 as i32]);
                }
            }
            ring.push(ring[0]);
            out.push(vec![ring]);
        }
    }
    out
}

/// A star polygon around `(cx, cy)` with radius up to `s`, optionally with a hole.
fn star(r: &mut Rng, cx: i64, cy: i64, s: i64, hole: bool) -> Rings {
    let ring = |r: &mut Rng, s: i64| -> Vec<[i32; 2]> {
        let n = r.range(3, 12) as usize;
        (0..n)
            .map(|k| {
                let a = k as f64 / n as f64 * std::f64::consts::TAU;
                let d = r.range(s / 3, s) as f64;
                [
                    (cx + (d * a.cos()) as i64).clamp(X0, X1) as i32,
                    (cy + (d * a.sin()) as i64).clamp(Y0, Y1) as i32,
                ]
            })
            .collect()
    };
    let mut rings = vec![ring(r, s)];
    if hole {
        rings.push(ring(r, s / 5));
    }
    rings
}

fn pack(regions: &[Rings], opts: &Options) -> Vec<u8> {
    let body = region::encode(regions, opts).unwrap();
    with_body(&body, regions.len() as u64)
}

fn with_body(body: &[u8], items: u64) -> Vec<u8> {
    let mut w = Writer::new("np", "regions", "2026.11.0").unwrap();
    w.section_bytes(Kind::REGION, Encoding::TABLE, items, body)
        .unwrap();
    // Sections of 1 MiB or more carry block hashes (D-078).
    let list = blocks::encode(&[(Kind::REGION, body)]).unwrap();
    if let Some(l) = &list {
        w.section_bytes(Kind::BLOCK_HASHES, Encoding::RAW, blocks::item_count(l), l)
            .unwrap();
    }
    let layout = w.layout(false).unwrap();
    let refs: Vec<&[u8]> = layout
        .placed()
        .iter()
        .map(|p| {
            if p.kind == Kind::REGION {
                body
            } else {
                list.as_deref().unwrap_or(&[])
            }
        })
        .collect();
    layout.assemble(&refs).unwrap()
}

fn open(bytes: &[u8]) -> Result<Region<'_>, Error> {
    let pack = Pack::open(bytes)?;
    Region::open(&pack.section(Kind::REGION).unwrap())
}

/// Query points of every kind for these regions.
fn points(regions: &[Rings], r: &mut Rng, n: usize) -> Vec<Point<E7>> {
    let mut v = Vec::new();
    let p =
        |x: i64, y: i64| Point::<E7>::from_units(y.clamp(Y0, Y1) as i32, x.clamp(X0, X1) as i32);
    let verts: Vec<[i32; 2]> = regions.iter().flatten().flatten().copied().collect();
    for _ in 0..n {
        v.extend(p(r.range(X0, X1), r.range(Y0, Y1)));
        if verts.len() >= 2 {
            let a = verts[(r.next() % verts.len() as u64) as usize];
            let b = verts[(r.next() % verts.len() as u64) as usize];
            // On a vertex, beside it, and on (or by) the segment towards another vertex.
            v.extend(p(a[0].into(), a[1].into()));
            v.extend(p(
                i64::from(a[0]) + r.range(-1, 1),
                i64::from(a[1]) + r.range(-1, 1),
            ));
            let (mx, my) = (
                i64::midpoint(i64::from(a[0]), i64::from(b[0])),
                i64::midpoint(i64::from(a[1]), i64::from(b[1])),
            );
            v.extend(p(mx, my));
        }
    }
    // Ring edges: midpoints, exactly on the edge when the deltas are even, and 1 unit off.
    for ring in regions.iter().flatten().take(200) {
        for w in ring.windows(2).take(20) {
            let (mx, my) = (
                i64::midpoint(i64::from(w[0][0]), i64::from(w[1][0])),
                i64::midpoint(i64::from(w[0][1]), i64::from(w[1][1])),
            );
            for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                v.extend(p(mx + dx, my + dy));
            }
        }
    }
    // The domain's edges and corners.
    for (x, y) in [
        (X0, Y0),
        (X1, Y1),
        (X0, Y1),
        (X1, Y0),
        (X1, 0),
        (X0, 0),
        (0, Y1),
        (0, Y0),
    ] {
        v.extend(p(x, y));
    }
    v
}

fn check(regions: &[Rings], opts: &Options, seed: u64, n: usize) {
    let bytes = pack(regions, opts);
    let ix = open(&bytes).unwrap();
    ix.check().unwrap();
    assert_eq!(ix.regions(), regions.len());
    let mut r = Rng(seed);
    let mut out = [0_u32; 16];
    for at in points(regions, &mut r, n) {
        let want = region::even_odd(regions, at);
        let got = ix.regions_at(at, &mut out);
        assert_eq!(&out[..got.min(16)], &want[..], "at {at:?} with {opts:?}");
        assert_eq!(ix.region(at), want.first().copied());
    }
}

fn world() -> Rings {
    vec![vec![
        [X0 as i32, Y0 as i32],
        [X1 as i32, Y0 as i32],
        [X1 as i32, Y1 as i32],
        [X0 as i32, Y1 as i32],
    ]]
}

#[test]
fn a_partition_shares_its_borders() {
    let mut r = Rng(7);
    // A small area (deep leaves) and the whole world.
    for (bx, m) in [
        ((850_000_000, 270_000_000, 2_000_000, 1_500_000), 4),
        ((X0, Y0, X1 - X0, Y1 - Y0), 6),
    ] {
        let mut regions = partition(&mut r, bx, m);
        regions.push(world());
        for opts in [
            Options {
                leaf_max: 1,
                top: 8,
            },
            Options {
                leaf_max: 4,
                top: 10,
            },
            Options::default(),
        ] {
            check(&regions, &opts, 11, 400);
        }
    }
}

#[test]
fn overlaps_holes_and_cancelling_rings() {
    let mut r = Rng(9);
    let mut regions: Vec<Rings> = (0..12)
        .map(|i| {
            let (cx, cy) = (
                r.range(840_000_000, 880_000_000),
                r.range(260_000_000, 300_000_000),
            );
            star(&mut r, cx, cy, 20_000_000, i % 3 == 0)
        })
        .collect();
    // A ring given twice cancels to nothing.
    let twice = star(&mut r, 860_000_000, 280_000_000, 5_000_000, false);
    regions.push(vec![twice[0].clone(), twice[0].clone()]);
    // On the domain's edges: the antimeridian and both poles.
    regions.push(star(&mut r, X1, 0, 100_000_000, false));
    regions.push(star(&mut r, X0, 0, 100_000_000, true));
    regions.push(star(&mut r, 0, Y1, 200_000_000, false));
    regions.push(star(&mut r, 0, Y0, 200_000_000, false));
    regions.push(world());
    for opts in [
        Options {
            leaf_max: 2,
            top: 8,
        },
        Options::default(),
    ] {
        check(&regions, &opts, 13, 600);
    }
}

#[test]
fn empty_and_tiny_inputs() {
    for regions in [vec![], vec![vec![]], vec![world()]] {
        check(
            &regions,
            &Options {
                leaf_max: 32,
                top: 8,
            },
            3,
            50,
        );
    }
}

#[test]
fn the_encoding_is_deterministic() {
    let mut r = Rng(21);
    let regions = partition(&mut r, (850_000_000, 270_000_000, 2_000_000, 1_500_000), 3);
    let opts = Options {
        leaf_max: 2,
        top: 8,
    };
    assert_eq!(
        region::encode(&regions, &opts).unwrap(),
        region::encode(&regions, &opts).unwrap()
    );
}

/// FNV-1a, 64 bits: a fixed digest of an encoding.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The writer and the reader share the cell, Morton, reference-point and crossing code, so a
/// change to one of them can keep the round trip exact while changing the format. These packs
/// pin the format: their bytes and shape may change only with a recorded format change.
#[test]
fn the_encoding_is_pinned() {
    let mut r = Rng(21);
    let mut part = partition(&mut r, (850_000_000, 270_000_000, 2_000_000, 1_500_000), 3);
    part.push(world());
    let mut r = Rng(9);
    let mut stars: Vec<Rings> = (0..6)
        .map(|i| star(&mut r, 860_000_000, 280_000_000, 20_000_000, i % 2 == 0))
        .collect();
    stars.push(star(&mut r, X1, 0, 100_000_000, false));
    stars.push(star(&mut r, 0, Y1, 200_000_000, false));
    let cases = [
        (
            &part,
            Options {
                leaf_max: 2,
                top: 8,
            },
        ),
        (&part, Options::default()),
        (
            &stars,
            Options {
                leaf_max: 4,
                top: 12,
            },
        ),
    ];
    let mut got = Vec::new();
    for (regions, opts) in cases {
        let body = region::encode(regions, &opts).unwrap();
        let bytes = with_body(&body, regions.len() as u64);
        let ix = open(&bytes).unwrap();
        ix.check().unwrap();
        got.push((
            fnv(&body),
            ix.top_level(),
            ix.leaves(),
            ix.runs(),
            ix.vertices(),
        ));
    }
    let want = [
        (5_119_189_209_052_931_683, 8, 163, 107, 85),
        (6_041_092_929_143_503_048, 10, 28, 37, 85),
        (3_687_082_912_245_267_465, 12, 163, 279, 86),
    ];
    assert_eq!(got, want);
}

fn column_in(cols: &[(u16, Vec<u32>)], id: u16) -> usize {
    cols.iter().position(|(c, _)| *c == id).unwrap()
}

#[test]
fn the_writer_refuses_bad_options_and_input() {
    let opts = Options::default();
    assert!(
        region::encode(
            &[],
            &Options {
                leaf_max: 0,
                top: 10
            }
        )
        .is_err()
    );
    assert!(
        region::encode(
            &[],
            &Options {
                leaf_max: 8,
                top: 7
            }
        )
        .is_err()
    );
    assert!(
        region::encode(
            &[],
            &Options {
                leaf_max: 8,
                top: 13
            }
        )
        .is_err()
    );
    let outside = vec![vec![vec![[0, 0], [0, 900_000_001], [1, 1]]]];
    assert!(region::encode(&outside, &opts).is_err());
}

#[test]
fn refusals() {
    // A wrong column fails `open` or `check`; every edit of a valid pack's columns is caught by
    // one of them or leaves the answers unchanged.
    let mut r = Rng(5);
    let regions = partition(&mut r, (850_000_000, 270_000_000, 2_000_000, 1_500_000), 3);
    let body = region::encode(
        &regions,
        &Options {
            leaf_max: 2,
            top: 8,
        },
    )
    .unwrap();
    let t = table::Table::parse(&body).unwrap();
    let ids: Vec<u16> = t.ids().collect();
    let cols: Vec<(u16, Vec<u32>)> = ids
        .iter()
        .map(|&id| {
            let c = t.column(id).unwrap();
            let mut v = vec![0; c.len()];
            c.decode_all(&mut v);
            (id, v)
        })
        .collect();
    let rebuild = |cols: &[(u16, Vec<u32>)]| -> Vec<u8> {
        let enc: Vec<(u16, Vec<u8>)> = cols
            .iter()
            .map(|(id, v)| (*id, teigeodata_core::bitpack::encode_direct(v)))
            .collect();
        let refs: Vec<(u16, &[u8])> = enc.iter().map(|(id, b)| (*id, b.as_slice())).collect();
        table::encode(&refs).unwrap()
    };
    // Unchanged columns re-encoded still pass.
    let same = with_body(&rebuild(&cols), regions.len() as u64);
    open(&same).unwrap().check().unwrap();
    // Every edit of a structural column is refused. Edits of a set ID or a set's bounds, a
    // run's edge or a reference candidate can keep the pack well-formed and change answers:
    // the signature, not the check, guards those (see the module documentation).
    let structural = [
        region::TOP,
        region::DIR,
        region::KEY,
        region::LEVEL,
        region::RUNS,
    ];
    for (c, (id, v)) in cols.iter().enumerate() {
        if !structural.contains(id) || v.is_empty() {
            continue;
        }
        for _ in 0..20 {
            let mut edited = cols.clone();
            let j = (r.next() % v.len() as u64) as usize;
            let delta = [1_u32, u32::MAX, 7][(r.next() % 3) as usize];
            edited[c].1[j] = v[j].wrapping_add(delta);
            let bytes = with_body(&rebuild(&edited), regions.len() as u64);
            assert!(
                open(&bytes).and_then(|ix| ix.check()).is_err(),
                "column {id} value {j} + {delta} passed"
            );
        }
    }
    // Set 0 must be the single region 0: a wrong member, then an empty set.
    for (id, j, v) in [(region::SET_ZONES, 0, 1), (region::SET_START, 1, 0)] {
        let mut bad = cols.clone();
        let c = column_in(&bad, id);
        bad[c].1[j] = v;
        let bytes = with_body(&rebuild(&bad), regions.len() as u64);
        assert!(
            open(&bytes).and_then(|ix| ix.check()).is_err(),
            "column {id} value {j} = {v} passed"
        );
    }
    // A reference candidate past the last one tried by the writer.
    let column = |id: u16| cols.iter().position(|(c, _)| *c == id);
    let runs = &cols[column(region::RUNS).unwrap()].1;
    let leaf = (0..runs.len() - 1)
        .find(|&l| runs[l] < runs[l + 1])
        .unwrap();
    let mut far = cols.clone();
    let k = column(region::NUDGE).unwrap_or_else(|| {
        far.push((region::NUDGE, vec![0; runs.len() - 1]));
        far.sort_by_key(|(id, _)| *id);
        column_in(&far, region::NUDGE)
    });
    far[k].1[leaf] = 4096;
    let bytes = with_body(&rebuild(&far), regions.len() as u64);
    assert!(open(&bytes).and_then(|ix| ix.check()).is_err());
    // An unknown column.
    let mut extra = cols.clone();
    extra.push((99, vec![0]));
    assert!(open(&with_body(&rebuild(&extra), regions.len() as u64)).is_err());
    let mut zero = cols.clone();
    zero.insert(0, (0, vec![0]));
    assert!(open(&with_body(&rebuild(&zero), regions.len() as u64)).is_err());
}
