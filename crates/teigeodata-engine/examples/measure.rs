//! Measures the engine on a real pack: open time (mapped), the first query of each kind (which
//! verifies the sections it reads), and warm latency through the engine with 1 and 3 layers
//! against direct section calls.
//!
//! ```text
//! cargo run --release -p teigeodata-engine --example measure -- data/build/pack/np/np-places-main.tgd
//! ```
//!
//! Timing on a shared machine is noisy: each figure is the minimum of several rounds, and the
//! configurations alternate within a round.

#![allow(
    clippy::print_stdout,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "a measuring tool: prints its table and panics on bad input"
)]

#[path = "../tests/common/mod.rs"]
mod common;

use std::hint::black_box;
use std::time::Instant;

use common::{Model, Rng};
use teigeodata_core::{E6, E7, Point};
use teigeodata_engine::{Engine, Layer, LayerOptions, Scratch};
use teigeodata_format::Kind;
use teigeodata_format::Pack;
use teigeodata_format::arcs::Arcs;
use teigeodata_format::idmap::IdMap;
use teigeodata_format::place_index::{Neighbour, PlaceIndex};
use teigeodata_format::places::Places;
use teigeodata_format::region::Region;

const ROUNDS: usize = 7;

/// Nanoseconds per call of `f` over `n` calls, the minimum of `ROUNDS` rounds.
fn per_call(n: usize, mut f: impl FnMut(usize)) -> f64 {
    (0..ROUNDS)
        .map(|_| {
            let t = Instant::now();
            for i in 0..n {
                f(i);
            }
            t.elapsed().as_nanos() as f64 / n as f64
        })
        .fold(f64::INFINITY, f64::min)
}

fn main() {
    let arg = std::env::args()
        .nth(1)
        .expect("usage: measure <pack.tgd> | synthetic:<places>");
    // A synthetic country (tests/common) is written to a file so that it is mapped too.
    let path = match arg.strip_prefix("synthetic:") {
        Some(n) => {
            let model = common::synthetic(n.parse().unwrap(), 2026);
            let file = std::env::temp_dir().join(format!("teigeodata-measure-{n}.tgd"));
            std::fs::write(&file, model.encode()).unwrap();
            file.to_string_lossy().into_owned()
        }
        None => arg,
    };
    let bytes = std::fs::read(&path).unwrap();
    let pack = Pack::open(&bytes).unwrap();
    println!("pack {path}: {} bytes", bytes.len());
    for s in pack.sections() {
        println!(
            "  {:<14} {:>10} bytes",
            s.kind().to_string(),
            s.bytes().len()
        );
    }

    // Open, mapped: the first open after the file was read once, then the minimum of many.
    let t = Instant::now();
    let first = Layer::open(&path, LayerOptions::default()).unwrap();
    let first_open = t.elapsed();
    assert!(first.is_mapped());
    drop(first);
    let open_ns = per_call(50, |_| {
        black_box(Layer::open(&path, LayerOptions::default()).unwrap());
    });
    println!(
        "open (mapped, head checks and block proofs): first {first_open:?}, warm {:.1} µs",
        open_ns / 1e3
    );

    // Probes from the pack itself.
    let model = Model::from_pack(&bytes);
    let mut r = Rng(12_345);
    let n = 2_000;
    let ids: Vec<_> = (0..n)
        .map(|_| model.ids[r.below(model.ids.len() as u64) as usize])
        .collect();
    let points: Vec<Point<E6>> = (0..n)
        .map(|_| {
            let p = model.points[r.below(model.points.len() as u64) as usize];
            // Near a place, not on it.
            Point::from_units(
                p.lat() + r.below(20_000) as i32 - 10_000,
                p.lon() + r.below(20_000) as i32 - 10_000,
            )
            .unwrap()
        })
        .collect();
    let points7: Vec<Point<E7>> = points.iter().map(|p| p.to_e7()).collect();
    let queries: Vec<String> = (0..200)
        .filter_map(|_| {
            let row = r.below(model.names.len() as u64) as usize;
            model.names[row].first().map(|x| x.text.clone())
        })
        .collect();

    // Cold: the first query of each kind on a fresh layer verifies what it reads.
    let mut e = Engine::new();
    e.add(Layer::open(&path, LayerOptions::default()).unwrap());
    let mut s = Scratch::default();
    let (mut near, mut admin, mut hits) = (Vec::new(), Vec::new(), Vec::new());
    let cold = |label: &str, f: &mut dyn FnMut()| {
        let t = Instant::now();
        f();
        println!("first {label}: {:?}", t.elapsed());
    };
    cold("get (IDMAP, REDIRECTS, PLACES)", &mut || {
        black_box(e.get(ids[0]).unwrap());
    });
    cold("reverse (PLACE_INDEX)", &mut || {
        e.reverse(points[0], 10, &mut s, &mut near).unwrap();
    });
    cold("admin_at (ADMIN_REGION, ARCS, POLYGONS)", &mut || {
        e.admin_at(points7[0], &mut s, &mut admin).unwrap();
    });
    cold("tz_at (REGION)", &mut || {
        black_box(e.tz_at(points7[0]).unwrap());
    });
    cold("search (SEARCH)", &mut || {
        e.search(&queries[0], 10, &mut s, &mut hits).unwrap();
    });

    // Warm: 1 layer, 3 layers (the pack split by ID), and direct section calls.
    let parts = model.split(3, |_, id| {
        (id.get().wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 40) as usize % 3
    });
    let mut e3 = Engine::new();
    for p in parts {
        e3.add(Layer::from_bytes(p, LayerOptions::default()).unwrap());
    }
    let ids_view = IdMap::open(&pack.section(Kind::IDMAP).unwrap()).unwrap();
    let places = Places::open(&pack.section(Kind::PLACES).unwrap()).unwrap();
    let index = PlaceIndex::open(&pack.section(Kind::PLACE_INDEX).unwrap(), places).unwrap();
    let arcs = pack.section(Kind::ARCS).map(|a| Arcs::open(&a).unwrap());
    let admin_section = pack.section(Kind::ADMIN_REGION).unwrap();
    let admin_ix = match &arcs {
        Some(a) => Region::open_on_arcs(&admin_section, a).unwrap(),
        None => Region::open(&admin_section).unwrap(),
    };
    let zones = Region::open(&pack.section(Kind::REGION).unwrap()).unwrap();
    // Warm every engine once.
    for eng in [&e, &e3] {
        for i in 0..n {
            black_box(eng.get(ids[i]).unwrap());
            eng.reverse(points[i], 10, &mut s, &mut near).unwrap();
            eng.admin_at(points7[i], &mut s, &mut admin).unwrap();
            black_box(eng.tz_at(points7[i]).unwrap());
        }
        for q in &queries {
            eng.search(q, 10, &mut s, &mut hits).unwrap();
        }
    }
    let mut nb = [Neighbour::default(); 10];
    let mut regions = [0_u32; 16];
    println!();
    println!("| Query | direct (ns) | engine, 1 layer (ns) | engine, 3 layers (ns) |");
    println!("|---|---:|---:|---:|");
    let row = |label: &str, d: Option<f64>, one: f64, three: f64| {
        let d = d.map_or("n/a".to_owned(), |v| format!("{v:.0}"));
        println!("| {label} | {d} | {one:.0} | {three:.0} |");
    };
    let direct_get = per_call(n, |i| {
        let row = ids_view.row(ids[i]).unwrap();
        black_box((
            ids_view.id(row),
            places.point(row),
            places.kind(row),
            places.admin_level(row),
            places.flags(row),
            places.elevation(row),
        ));
    });
    let get1 = per_call(n, |i| {
        black_box(e.get(ids[i]).unwrap());
    });
    let get3 = per_call(n, |i| {
        black_box(e3.get(ids[i]).unwrap());
    });
    row("get", Some(direct_get), get1, get3);
    let direct_rev = per_call(n, |i| {
        let k = index.nearest(points[i], &mut nb);
        for x in &nb[..k] {
            black_box((
                ids_view.id(x.row),
                places.point(x.row),
                places.kind(x.row),
                x.meters(),
            ));
        }
    });
    let mut s1 = Scratch::default();
    let rev1 = per_call(n, |i| e.reverse(points[i], 10, &mut s1, &mut near).unwrap());
    let rev3 = per_call(n, |i| {
        e3.reverse(points[i], 10, &mut s1, &mut near).unwrap();
    });
    row("reverse, k = 10", Some(direct_rev), rev1, rev3);
    let direct_admin = per_call(n, |i| {
        let k = admin_ix.regions_at(points7[i], &mut regions);
        black_box(&regions[..k.min(16)]);
    });
    let adm1 = per_call(n, |i| {
        e.admin_at(points7[i], &mut s1, &mut admin).unwrap();
    });
    let adm3 = per_call(n, |i| {
        e3.admin_at(points7[i], &mut s1, &mut admin).unwrap();
    });
    row(
        "admin_at (direct: region IDs only)",
        Some(direct_admin),
        adm1,
        adm3,
    );
    let direct_tz = per_call(n, |i| {
        black_box(zones.region(points7[i]).and_then(|z| zones.place(z)));
    });
    let tz1 = per_call(n, |i| {
        black_box(e.tz_at(points7[i]).unwrap());
    });
    let tz3 = per_call(n, |i| {
        black_box(e3.tz_at(points7[i]).unwrap());
    });
    row("tz_at (direct: zone row only)", Some(direct_tz), tz1, tz3);
    let m = queries.len();
    let se1 = per_call(m, |i| {
        e.search(&queries[i], 10, &mut s1, &mut hits).unwrap();
    });
    let se3 = per_call(m, |i| {
        e3.search(&queries[i], 10, &mut s1, &mut hits).unwrap();
    });
    row("search, k = 10", None, se1, se3);
}
