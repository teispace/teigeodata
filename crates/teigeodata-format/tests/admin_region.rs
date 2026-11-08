//! `ADMIN_REGION`: REGION's index on the ARCS vertices. A grid of cells and their parent on
//! shared arcs, some longer than a 128-vertex block and some on a coarser grid (an arc scale),
//! against the even–odd oracle; every value its check owns, moved by one, is refused.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::many_single_char_names,
    reason = "test code: model values are small, indices come from the same vectors"
)]

use teigeodata_core::bitpack::{Packed, encode_direct};
use teigeodata_core::table::{Table, encode as table};
use teigeodata_core::{E7, Point};
use teigeodata_format::arcs::{self, ArcColumns, Arcs};
use teigeodata_format::region::{
    self, MAX_SET, Options, RUN_EDGE, RUN_LEN, RUN_SCALE, RUN_X, RUN_Y, Region, X,
};
use teigeodata_format::{Encoding, Error, Kind, Pack, Writer};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: i32) -> i32 {
        (self.next() % n as u64) as i32
    }
}

/// Arcs of a `w` × `h` grid of cells, and per region the arcs it uses: region 0 is the whole
/// grid, region `1 + j·w + i` cell (i, j). Edges bend; every seventh is long (more than two
/// blocks), and with `coarse` every vertex is on a 1,000-unit grid (an arc scale of 3).
/// Each region is its ring as `(arc, reversed)` in walking order.
type Refs = Vec<Vec<(u32, bool)>>;

fn grid(w: usize, h: usize, coarse: bool, seed: u64) -> (Vec<Vec<[i32; 2]>>, Refs) {
    let mut r = Rng(seed);
    let size = 2_000_000;
    let (x0, y0) = (850_000_000, 270_000_000);
    let node = |i: usize, j: usize| [x0 + i as i32 * size, y0 + j as i32 * size];
    let snap = |v: i32| if coarse { v / 1_000 * 1_000 } else { v };
    let mut arcs: Vec<Vec<[i32; 2]>> = Vec::new();
    let mut edge = |a: [i32; 2], b: [i32; 2], arcs: &mut Vec<Vec<[i32; 2]>>| {
        let bends = if arcs.len() % 7 == 3 { 300 } else { r.below(6) };
        let mut pts = vec![a];
        for k in 1..=bends {
            let t = (i64::from(k) * i64::from(size) / i64::from(bends + 1)) as i32;
            let off = snap(r.below(size / 10) - size / 20);
            let p = if a[1] == b[1] {
                [a[0] + snap(t), a[1] + off]
            } else {
                [a[0] + off, a[1] + snap(t)]
            };
            if pts.last() != Some(&p) {
                pts.push(p);
            }
        }
        if pts.last() != Some(&b) {
            pts.push(b);
        }
        arcs.push(pts);
        arcs.len() as u32 - 1
    };
    let hz: Vec<Vec<u32>> = (0..=h)
        .map(|j| {
            (0..w)
                .map(|i| edge(node(i, j), node(i + 1, j), &mut arcs))
                .collect()
        })
        .collect();
    let vt: Vec<Vec<u32>> = (0..=w)
        .map(|i| {
            (0..h)
                .map(|j| edge(node(i, j), node(i, j + 1), &mut arcs))
                .collect()
        })
        .collect();
    // Counter-clockwise: south edge east, east edge north, north edge west, west edge south.
    let mut outer = Vec::new();
    outer.extend((0..w).map(|i| (hz[0][i], false)));
    outer.extend((0..h).map(|j| (vt[w][j], false)));
    outer.extend((0..w).rev().map(|i| (hz[h][i], true)));
    outer.extend((0..h).rev().map(|j| (vt[0][j], true)));
    let mut regions = vec![outer];
    for j in 0..h {
        for i in 0..w {
            regions.push(vec![
                (hz[j][i], false),
                (vt[i + 1][j], false),
                (hz[j + 1][i], true),
                (vt[i][j], true),
            ]);
        }
    }
    (arcs, regions)
}

/// The arcs each region uses.
fn uses(regions: &Refs) -> Vec<Vec<u32>> {
    regions
        .iter()
        .map(|r| r.iter().map(|x| x.0).collect())
        .collect()
}

/// Each region's ring, walked from its arcs, for the oracle.
fn rings(arcs: &[Vec<[i32; 2]>], regions: &Refs) -> Vec<Vec<Vec<[i32; 2]>>> {
    regions
        .iter()
        .map(|list| {
            let mut ring: Vec<[i32; 2]> = Vec::new();
            for &(a, rev) in list {
                let mut v = arcs[a as usize].clone();
                if rev {
                    v.reverse();
                }
                let skip = usize::from(!ring.is_empty());
                ring.extend(v.into_iter().skip(skip));
            }
            vec![ring]
        })
        .collect()
}

struct Built {
    arcs_body: Vec<u8>,
    region_body: Vec<u8>,
    n_arcs: u64,
    n_regions: u64,
}

fn build(arcs: &[Vec<[i32; 2]>], regions: &[Vec<u32>], opts: &Options) -> Built {
    let pts: Vec<Vec<Point<E7>>> = arcs
        .iter()
        .map(|a| {
            a.iter()
                .map(|p| Point::<E7>::from_units(p[1], p[0]).unwrap())
                .collect()
        })
        .collect();
    let refs: Vec<&[Point<E7>]> = pts.iter().map(Vec::as_slice).collect();
    let arcs_body = arcs::encode(&ArcColumns {
        arcs: &refs,
        ..ArcColumns::default()
    })
    .unwrap();
    let open = Arcs::open(&teigeodata_format::Section::unpacked(
        Kind::ARCS,
        arcs.len() as u64,
        &arcs_body,
    ))
    .unwrap();
    let scales: Vec<u32> = if open.has_scale() {
        (0..arcs.len()).map(|a| open.scale(a)).collect()
    } else {
        Vec::new()
    };
    let region_body = region::encode_on_arcs(arcs, &scales, regions, opts).unwrap();
    Built {
        arcs_body,
        region_body,
        n_arcs: arcs.len() as u64,
        n_regions: regions.len() as u64,
    }
}

fn pack(b: &Built, region_body: &[u8]) -> Vec<u8> {
    let mut w = Writer::new("np", "places", "2026.11.0").unwrap();
    w.section_bytes(Kind::ARCS, Encoding::TABLE, b.n_arcs, &b.arcs_body)
        .unwrap();
    w.section_bytes(
        Kind::ADMIN_REGION,
        Encoding::TABLE,
        b.n_regions,
        region_body,
    )
    .unwrap();
    w.layout(false)
        .unwrap()
        .assemble(&[&b.arcs_body, region_body])
        .unwrap()
}

fn with_open<T>(bytes: &[u8], f: impl FnOnce(Result<Region<'_>, Error>, &Arcs<'_>) -> T) -> T {
    let p = Pack::open(bytes).unwrap();
    let a = Arcs::open(&p.section(Kind::ARCS).unwrap()).unwrap();
    let r = Region::open_on_arcs(&p.section(Kind::ADMIN_REGION).unwrap(), &a);
    f(r, &a)
}

/// Probes on vertices, on vertex latitudes and near vertices, and uniform over the grid.
fn probes(arcs: &[Vec<[i32; 2]>], seed: u64) -> Vec<Point<E7>> {
    let mut r = Rng(seed);
    let all: Vec<[i32; 2]> = arcs.iter().flatten().copied().collect();
    (0..3_000)
        .map(|k| {
            let v = all[r.below(all.len() as i32) as usize];
            let (dx, dy) = match k % 4 {
                0 => (0, 0),
                1 => (r.below(200_001) - 100_000, 0),
                2 => (r.below(2_001) - 1_000, r.below(2_001) - 1_000),
                _ => (
                    r.below(30_000_001) - 15_000_000,
                    r.below(30_000_001) - 15_000_000,
                ),
            };
            Point::<E7>::from_units(v[1] + dy, v[0] + dx).unwrap()
        })
        .collect()
}

#[test]
fn answers_match_even_odd() {
    for (w, h, coarse, leaf_max) in [(6, 5, false, 32), (6, 5, true, 8), (3, 2, true, 1)] {
        let (arcs, regions) = grid(w, h, coarse, 7 + w as u64);
        let b = build(&arcs, &uses(&regions), &Options { leaf_max, top: 8 });
        let bytes = pack(&b, &b.region_body);
        let oracle = rings(&arcs, &regions);
        with_open(&bytes, |r, a| {
            let r = r.unwrap();
            assert!(r.on_arcs());
            // Unbent edges lie on the node grid, so even the fine grid has some scaled arcs.
            assert!(a.has_scale(), "some arcs are scaled");
            assert!(!a.is_empty() && arcs.iter().any(|v| v.len() > 256));
            r.check().unwrap();
            let mut out = [0_u32; MAX_SET];
            for at in probes(&arcs, 3) {
                let k = r.regions_at(at, &mut out);
                assert_eq!(
                    out[..k].to_vec(),
                    region::even_odd(&oracle, at),
                    "{w}x{h} coarse {coarse} at {at}"
                );
            }
        });
    }
}

/// The table's columns as values, and back.
fn columns(body: &[u8]) -> Vec<(u16, Vec<u32>)> {
    let t = Table::parse(body).unwrap();
    let all = |c: Packed<'_>| (0..c.len()).map(|i| c.get(i).unwrap()).collect::<Vec<_>>();
    t.ids().map(|id| (id, all(t.column(id).unwrap()))).collect()
}

fn body_of(cols: &[(u16, Vec<u32>)]) -> Vec<u8> {
    let enc: Vec<(u16, Vec<u8>)> = cols.iter().map(|(id, v)| (*id, encode_direct(v))).collect();
    let refs: Vec<(u16, &[u8])> = enc.iter().map(|(id, b)| (*id, b.as_slice())).collect();
    table(&refs).unwrap()
}

#[test]
fn check_refuses_wrong_runs() {
    let (arcs, regions) = grid(6, 5, true, 21);
    let b = build(
        &arcs,
        &uses(&regions),
        &Options {
            leaf_max: 8,
            top: 8,
        },
    );
    let cols = columns(&b.region_body);
    assert_eq!(
        body_of(&cols),
        b.region_body,
        "a decoded round trip is the writer's bytes"
    );
    let col = |id: u16| cols.iter().position(|c| c.0 == id).unwrap();
    let mut r = Rng(5);
    let runs = cols[col(RUN_X)].1.len();
    // Every value of the run columns, moved by one at random runs, is refused; a run made one
    // edge longer too (a shorter run is not: the check does not re-derive the sets, in either
    // vertex mode, and the signature guards them).
    for id in [RUN_X, RUN_Y, RUN_SCALE, RUN_EDGE, RUN_LEN] {
        for _ in 0..12 {
            let at = r.below(runs as i32) as usize;
            let mut bad = cols.clone();
            let v = &mut bad[col(id)].1[at];
            *v = if id == RUN_LEN {
                *v + 1
            } else if *v == 0 {
                1
            } else {
                *v - 1
            };
            let bytes = pack(&b, &body_of(&bad));
            let err = with_open(&bytes, |r, _| r.and_then(|r| r.check()).err());
            // The run's own vertex and scale are caught by their own rule, not by a later one.
            let named = match err {
                Some(Error::Payload { column, .. }) => {
                    // One rule checks the whole point and names RUN_X.
                    let want = if id == RUN_Y { RUN_X } else { id };
                    ![RUN_X, RUN_Y, RUN_SCALE].contains(&id) || column == want
                }
                _ => false,
            };
            assert!(named, "column {id}, run {at}: {err:?}");
        }
    }
    // A run stretched past its arc's last vertex: refused even where the next arc continues.
    let ends: Vec<usize> = {
        let mut s = 0;
        arcs.iter()
            .map(|a| {
                s += a.len();
                s - 1
            })
            .collect()
    };
    let (edges, lens) = (&cols[col(RUN_EDGE)].1, &cols[col(RUN_LEN)].1);
    let at = (0..runs)
        .find(|&k| ends.contains(&((edges[k] + lens[k]) as usize)))
        .unwrap();
    let mut bad = cols.clone();
    bad[col(RUN_LEN)].1[at] += 1;
    let bytes = pack(&b, &body_of(&bad));
    let err = with_open(&bytes, |r, _| r.and_then(|r| r.check()).err());
    assert!(
        matches!(
            err,
            Some(Error::Payload {
                column: RUN_LEN,
                ..
            })
        ),
        "{err:?}"
    );
    // Without its scales, or opened without the arcs, it does not open.
    let no_scale: Vec<(u16, Vec<u32>)> =
        cols.iter().filter(|c| c.0 != RUN_SCALE).cloned().collect();
    let bytes = pack(&b, &body_of(&no_scale));
    assert!(with_open(&bytes, |r, _| r.is_err()));
    let bytes = pack(&b, &b.region_body);
    let p = Pack::open(&bytes).unwrap();
    assert!(Region::open(&p.section(Kind::ADMIN_REGION).unwrap()).is_err());
    // Own vertices next to run vertices: refused.
    let mut mixed = cols.clone();
    mixed.push((X, vec![0; 4]));
    mixed.sort_by_key(|c| c.0);
    let bytes = pack(&b, &body_of(&mixed));
    assert!(with_open(&bytes, |r, _| r.is_err()));
}

#[test]
fn an_arc_used_twice_by_one_region_cancels() {
    // A slit: region 0 is the square, and region 1 is the square plus a cut arc walked twice
    // (there and back), which borders nothing.
    let sq = 10_000_000;
    let arcs = vec![
        vec![[0, 0], [sq, 0], [sq, sq], [0, sq], [0, 0]],
        vec![[0, 0], [sq / 2, sq / 2]],
    ];
    let regions = vec![vec![0], vec![0, 1, 1]];
    let b = build(&arcs, &regions, &Options::default());
    let bytes = pack(&b, &b.region_body);
    with_open(&bytes, |r, _| {
        let r = r.unwrap();
        r.check().unwrap();
        let mut out = [0_u32; 4];
        let on_cut = Point::<E7>::from_units(sq / 4 + 1, sq / 4).unwrap();
        let k = r.regions_at(on_cut, &mut out);
        assert_eq!(&out[..k], &[0, 1]);
    });
}
