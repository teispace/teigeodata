//! B9: REGION on timezone-boundary-builder 2026f with oceans (D-083). Sizes per column, build
//! time, exactness against an even–odd ray cast over the original rings, and lookups against
//! `tzf-rs` 2.3.0 with its full data (`tzf-dist` 2026f `full.tzb`, coordinates at 1e-5°).
//!
//! Inputs: `data/cache/tbb-2026f/combined-with-oceans.json`,
//! `data/cache/tzf-dist-2026f/full.tzb` (SHA-256 `75513a16…d8a3`), and `GeoNames` `NP.txt` if
//! present. Run in release: `cargo run --release -p teigeodata-bench --example b9-region`.
#![allow(
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "a report printed for the results page; inputs checked by the run"
)]

use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use serde::Deserialize;
use teigeodata_bench::{Rng, geonames_path, pack_with};
use teigeodata_core::table::Table;
use teigeodata_core::{E7, Point};
use teigeodata_format::region::{self, Options, Region};
use teigeodata_format::{Encoding, Kind, Pack};

#[derive(Deserialize)]
struct Collection {
    features: Vec<Feature>,
}

#[derive(Deserialize)]
struct Feature {
    properties: Props,
    geometry: Geometry,
}

#[derive(Deserialize)]
struct Props {
    tzid: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", content = "coordinates")]
enum Geometry {
    Polygon(Vec<Vec<[f64; 2]>>),
    MultiPolygon(Vec<Vec<Vec<[f64; 2]>>>),
}

type Rings = Vec<Vec<[i32; 2]>>;

/// The zones in precedence order (Q-013, D-083): the Nepal perspective first (Asia/Kathmandu
/// over Asia/Kolkata where the source overlaps), then the source's order.
fn load(path: &Path) -> (Vec<String>, Vec<Rings>) {
    let fc: Collection =
        serde_json::from_slice(&std::fs::read(path).expect("the GeoJSON")).unwrap();
    let q = |v: f64| (v * 1e7).round() as i32;
    let mut zones: Vec<(String, Rings)> = fc
        .features
        .into_iter()
        .map(|f| {
            let polys = match f.geometry {
                Geometry::Polygon(p) => vec![p],
                Geometry::MultiPolygon(m) => m,
            };
            let rings = polys
                .into_iter()
                .flatten()
                .map(|r| r.iter().map(|c| [q(c[0]), q(c[1])]).collect())
                .collect();
            (f.properties.tzid, rings)
        })
        .collect();
    let first = ["Asia/Kathmandu"];
    zones.sort_by_key(|(name, _)| !first.contains(&name.as_str()));
    zones.into_iter().unzip()
}

struct Oracle {
    rings: Vec<(u32, [i32; 4], Vec<[i32; 2]>)>,
}

impl Oracle {
    fn new(zones: &[Rings]) -> Self {
        let mut rings = Vec::new();
        for (z, rs) in zones.iter().enumerate() {
            for r in rs {
                let mut b = [i32::MAX, i32::MIN, i32::MAX, i32::MIN];
                for p in r {
                    b = [
                        b[0].min(p[0]),
                        b[1].max(p[0]),
                        b[2].min(p[1]),
                        b[3].max(p[1]),
                    ];
                }
                rings.push((z as u32, b, r.clone()));
            }
        }
        Self { rings }
    }

    /// Every zone holding the point, by the module's rule.
    fn at(&self, p: Point<E7>, zones: usize) -> Vec<u32> {
        let x = if p.lon() >= 1_800_000_000 {
            -1_800_000_000
        } else {
            p.lon()
        };
        let y = if p.lat() >= 900_000_000 {
            899_999_999
        } else {
            p.lat()
        };
        let mut parity = vec![false; zones];
        for (z, b, ring) in &self.rings {
            if y < b[2] || y > b[3] || x > b[1] {
                continue;
            }
            let n = ring.len();
            let mut inside = false;
            for k in 0..n {
                let (a, c) = (ring[k], ring[(k + 1) % n]);
                if a == c || (a[1] > y) == (c[1] > y) {
                    continue;
                }
                let lhs =
                    (i128::from(x) - i128::from(a[0])) * (i128::from(c[1]) - i128::from(a[1]));
                let rhs =
                    (i128::from(c[0]) - i128::from(a[0])) * (i128::from(y) - i128::from(a[1]));
                if if c[1] > a[1] { lhs < rhs } else { lhs > rhs } {
                    inside = !inside;
                }
            }
            if inside {
                parity[*z as usize] ^= true;
            }
        }
        (0..zones as u32).filter(|&z| parity[z as usize]).collect()
    }
}

fn pack(body: &[u8], zones: usize) -> Vec<u8> {
    pack_with(
        "timezones",
        1,
        &[(Kind::REGION, Encoding::TABLE, zones as u64, body)],
        false,
    )
    .unwrap()
}

/// Bytes per column of a REGION body.
fn columns(body: &[u8]) -> String {
    let t = Table::parse(body).unwrap();
    let names = [
        "",
        "top",
        "dir",
        "key",
        "level",
        "zones",
        "nudge",
        "runs",
        "run edge",
        "run len",
        "run zones",
        "x",
        "y",
        "set start",
        "set zones",
        "top zones",
    ];
    t.ids()
        .map(|id| {
            let b = t.column_bytes(id).map_or(0, <[u8]>::len);
            format!("{} {:.2}", names[id as usize], b as f64 / 1e6)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn time<F: FnMut(usize) -> u64>(n: usize, mut f: F) -> f64 {
    let mut runs: Vec<f64> = (0..5)
        .map(|_| {
            let t = Instant::now();
            let mut acc = 0;
            for i in 0..n {
                acc += f(i);
            }
            black_box(acc);
            t.elapsed().as_nanos() as f64 / n as f64
        })
        .collect();
    runs.sort_by(f64::total_cmp);
    runs[2]
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/cache");
    let t = Instant::now();
    let (names, zones) = load(&root.join("tbb-2026f/combined-with-oceans.json"));
    let edges: usize = zones.iter().flatten().map(Vec::len).sum();
    println!(
        "B9 REGION on tbb 2026f with oceans: {} zones, {edges} ring vertices; load {:.1} s\n",
        names.len(),
        t.elapsed().as_secs_f64()
    );

    // A/B: leaf size, then the top level.
    println!("| Leaf max | Top | Build s | MB | Leaves | Runs | Vertices | Columns (MB) |");
    println!("|---:|---:|---:|---:|---:|---:|---:|---|");
    let mut built = Vec::new();
    for (leaf_max, top) in [(8, 10), (16, 10), (32, 10), (16, 8), (16, 12)] {
        let t = Instant::now();
        let body = region::encode(&zones, &Options { leaf_max, top }).unwrap();
        let secs = t.elapsed().as_secs_f64();
        let file = pack(&body, zones.len());
        let p = Pack::open(&file).unwrap();
        let ix = Region::open(&p.section(Kind::REGION).unwrap()).unwrap();
        ix.check().unwrap();
        println!(
            "| {leaf_max} | {top} | {secs:.1} | {:.1} | {} | {} | {} | {} |",
            body.len() as f64 / 1e6,
            ix.leaves(),
            ix.runs(),
            ix.vertices(),
            columns(&body)
        );
        built.push(((leaf_max, top), file));
    }

    // Query sets.
    let mut r = Rng::new(0x9e37_79b9_7f4a_7c15);
    let range =
        |r: &mut Rng, lo: i64, hi: i64| (lo + (r.next_u64() % (hi - lo) as u64) as i64) as i32;
    let uniform: Vec<Point<E7>> = (0..1_000_000)
        .map(|_| {
            Point::from_units(
                range(&mut r, -900_000_000, 900_000_000),
                range(&mut r, -1_800_000_000, 1_800_000_000),
            )
            .unwrap()
        })
        .collect();
    let all: Vec<[i32; 2]> = zones.iter().flatten().flatten().copied().collect();
    let near: Vec<Point<E7>> = (0..20_000)
        .filter_map(|_| {
            let i = (r.next_u64() % (all.len() as u64 - 1)) as usize;
            let (a, b) = (all[i], all[i + 1]);
            let m = |u: i32, v: i32| i32::midpoint(u, v);
            let d = |r: &mut Rng| (r.next_u64() % 3) as i32 - 1;
            Point::from_units(m(a[1], b[1]) + d(&mut r), m(a[0], b[0]) + d(&mut r))
        })
        .collect();
    let places: Vec<Point<E7>> = std::fs::read_to_string(geonames_path())
        .map(|text| {
            text.lines()
                .filter_map(|l| {
                    let f: Vec<&str> = l.split('\t').collect();
                    Point::<E7>::from_degrees(f.get(4)?.parse().ok()?, f.get(5)?.parse().ok()?).ok()
                })
                .collect()
        })
        .unwrap_or_default();

    // Exactness on the default build.
    let (_, file) = built.iter().find(|(o, _)| *o == (16, 10)).unwrap();
    let p = Pack::open(file).unwrap();
    let ix = Region::open(&p.section(Kind::REGION).unwrap()).unwrap();
    let oracle = Oracle::new(&zones);
    println!("\n| Exactness set | Points | In 2+ zones | Mismatches |\n|---|---:|---:|---:|");
    for (label, set) in [
        ("uniform world", &uniform[..20_000]),
        ("1 unit from an edge", &near[..]),
        ("GeoNames Nepal places", &places[..]),
    ] {
        let t = Instant::now();
        let res: Vec<(usize, usize)> = std::thread::scope(|s| {
            let chunk = set.len().div_ceil(8).max(1);
            let hs: Vec<_> = set
                .chunks(chunk)
                .map(|part| {
                    let (ix, oracle, count) = (&ix, &oracle, names.len());
                    s.spawn(move || {
                        let mut out = [0_u32; 16];
                        let (mut multi, mut bad) = (0, 0);
                        for &q in part {
                            let want = oracle.at(q, count);
                            let n = ix.regions_at(q, &mut out);
                            multi += usize::from(want.len() > 1);
                            if out[..n.min(16)] != want[..] {
                                bad += 1;
                            }
                        }
                        (multi, bad)
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let (multi, bad) = res.iter().fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
        println!(
            "| {label} | {} | {multi} | **{bad}** ({:.0} s) |",
            set.len(),
            t.elapsed().as_secs_f64()
        );
    }

    // Lookups.
    let tzb = std::fs::read(root.join("tzf-dist-2026f/full.tzb")).expect("full.tzb");
    let finder = tzf_rs::DefaultFinder::from_tzb(&tzb).unwrap();
    println!(
        "\n| Lookup | Uniform world ns | Nepal places ns | Agreement with tzf-rs (uniform / Nepal) |\n|---|---:|---:|---:|"
    );
    for ((leaf_max, top), file) in &built {
        let p = Pack::open(file).unwrap();
        let ix = Region::open(&p.section(Kind::REGION).unwrap()).unwrap();
        let ours = |set: &[Point<E7>]| {
            time(set.len(), |i| {
                u64::from(ix.region(black_box(set[i])).unwrap_or(0))
            })
        };
        let (u, n) = (
            ours(&uniform),
            if places.is_empty() {
                0.0
            } else {
                ours(&places)
            },
        );
        let agree = |set: &[Point<E7>]| {
            let same = set
                .iter()
                .filter(|q| {
                    let theirs = finder.get_tz_name(q.lon_degrees(), q.lat_degrees());
                    ix.region(**q).map(|z| names[z as usize].as_str()) == Some(theirs)
                })
                .count();
            100.0 * same as f64 / set.len().max(1) as f64
        };
        println!(
            "| ours, leaf {leaf_max}, top {top} | {u:.0} | {n:.0} | {:.4}% / {:.4}% |",
            agree(&uniform[..100_000]),
            agree(&places)
        );
    }
    let theirs = |set: &[Point<E7>]| {
        time(set.len(), |i| {
            black_box(finder.get_tz_name(set[i].lon_degrees(), set[i].lat_degrees())).len() as u64
        })
    };
    println!(
        "| `tzf-rs` 2.3.0, full data, `get_tz_name` | {:.0} | {:.0} | — |",
        theirs(&uniform),
        if places.is_empty() {
            0.0
        } else {
            theirs(&places)
        }
    );
}
