//! The `Engine` handle over a synthetic pack (the engine's own test model): every query as a
//! blob, its errors typed, and queries from many threads while packs are added (D-274).

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::many_single_char_names,
    reason = "tests: failures panic with their message"
)]

#[path = "../../teigeodata-engine/tests/common/mod.rs"]
mod common;

use std::sync::Arc;

use common::synthetic;
use teigeodata_api::{Blob, Engine, ErrorCode, LIMIT_MAX};
use teigeodata_blob::{Reader, View};

fn strings(blob: &Blob, name: &str) -> Vec<Option<String>> {
    let r = Reader::new(blob.as_bytes()).unwrap();
    let nulls = r.nulls(name);
    let null = |i: usize| nulls.is_some_and(|n| n.is_null(i));
    match r.column(name) {
        Some(View::Utf8(s)) => (0..r.rows())
            .map(|i| (!null(i)).then(|| s.get(i).unwrap().to_owned()))
            .collect(),
        Some(View::Dict(d)) => (0..r.rows())
            .map(|i| (!null(i)).then(|| d.get(i).unwrap().to_owned()))
            .collect(),
        _ => panic!("{name} is not a string column"),
    }
}

fn f64s(blob: &Blob, name: &str) -> Vec<f64> {
    let r = Reader::new(blob.as_bytes()).unwrap();
    let Some(View::F64(v)) = r.column(name) else {
        panic!("{name}")
    };
    v.f64s().collect()
}

fn rows(blob: &Blob) -> usize {
    Reader::new(blob.as_bytes()).unwrap().rows()
}

/// The synthetic country's row (rows are in Hilbert order, so not the first).
fn country(model: &common::Model) -> usize {
    model
        .kinds
        .iter()
        .position(|k| *k == teigeodata_core::PlaceKind::Country)
        .unwrap()
}

/// The bindings' fixture model: the synthetic country, coded `NP`.
fn fixture() -> common::Model {
    let mut model = synthetic(60, 5);
    let row = u32::try_from(country(&model)).unwrap();
    model.iso = vec![(row, "NP".parse().unwrap())];
    model
}

fn open() -> (Engine, common::Model) {
    let model = synthetic(400, 11);
    let engine = Engine::new();
    assert_eq!(engine.layers(), 0);
    assert_eq!(engine.open_bytes(&model.encode()), Ok(1));
    (engine, model)
}

#[test]
fn get_returns_one_row_with_every_column() {
    let (engine, model) = open();
    let row = country(&model);
    let id = model.ids[row].to_string();
    let blob = engine.get(&id).unwrap();
    let r = Reader::new(blob.as_bytes()).unwrap();
    assert_eq!(r.rows(), 1);
    assert_eq!(
        r.names().collect::<Vec<_>>(),
        [
            "id",
            "name",
            "kind",
            "level",
            "lat",
            "lon",
            "elevation",
            "parent",
            "tz",
            "iso"
        ]
    );
    assert_eq!(strings(&blob, "id"), [Some(id)]);
    assert_eq!(strings(&blob, "name"), [Some("Synthland".to_owned())]);
    assert_eq!(strings(&blob, "kind"), [Some("country".to_owned())]);
    let lat = f64s(&blob, "lat")[0];
    assert!((lat - model.points[row].lat_degrees()).abs() < 1e-9);
    // An unknown but well-formed ID: no row. A malformed one: an argument error.
    assert_eq!(rows(&engine.get("tgd:np/zzzzzz").unwrap()), 0);
    let e = engine.get("not an id").unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);
}

#[test]
fn search_reverse_and_admin_answer_as_blobs() {
    let (engine, model) = open();
    let hits = engine.search("Synthland", 5).unwrap();
    assert_eq!(strings(&hits, "name")[0].as_deref(), Some("Synthland"));
    let r = Reader::new(hits.as_bytes()).unwrap();
    assert!(matches!(r.column("score"), Some(View::I32(_))));
    // An empty search still carries its score column.
    let none = engine.search("qqqqqqqq", 5).unwrap();
    let r = Reader::new(none.as_bytes()).unwrap();
    assert_eq!(r.rows(), 0);
    assert!(r.column("score").is_some());

    let p = model.points[model.points.len() - 1];
    let (lat, lon) = (p.lat_degrees(), p.lon_degrees());
    let near = engine.reverse(lat, lon, 3).unwrap();
    let meters = f64s(&near, "meters");
    assert_eq!(meters.len(), 3);
    assert!(meters.windows(2).all(|w| w[0] <= w[1]), "{meters:?}");
    assert_eq!(f64s(&near, "bearing").len(), 3);

    let units = engine.admin_at(lat, lon).unwrap();
    let kinds = strings(&units, "kind");
    assert_ne!(kinds.len(), 0);
    assert!(
        kinds.iter().all(|k| k.as_deref() != Some("settlement")),
        "{kinds:?}"
    );
}

#[test]
fn errors_are_typed() {
    let (engine, _) = open();
    let code = |r: teigeodata_api::Result<Blob>| r.unwrap_err().code;
    assert_eq!(
        code(engine.search("x", LIMIT_MAX + 1)),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        code(engine.reverse(91.0, 0.0, 1)),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        code(engine.reverse(f64::NAN, 0.0, 1)),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        code(engine.admin_at(0.0, 181.0)),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        engine.open_bytes(b"not a pack").unwrap_err().code,
        ErrorCode::Corrupt
    );
    let missing = engine.open_file("/nonexistent/teigeodata.tgd").unwrap_err();
    assert_eq!(missing.code, ErrorCode::Io);
    assert_eq!(engine.layers(), 1, "a failed open adds nothing");
}

/// Queries from four threads while the main thread adds packs: each answer is whole, from one
/// snapshot (D-141), and no call blocks another.
#[test]
fn many_threads_query_while_packs_are_added() {
    let (engine, model) = open();
    let engine = Arc::new(engine);
    let id = model.ids[country(&model)].to_string();
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let (engine, id) = (Arc::clone(&engine), id.clone());
            std::thread::spawn(move || {
                for _ in 0..100 {
                    let blob = engine.get(&id).unwrap();
                    assert_eq!(strings(&blob, "name"), [Some("Synthland".to_owned())]);
                    assert!(rows(&engine.search("Synthland", 3).unwrap()) >= 1);
                }
            })
        })
        .collect();
    for seed in 0..4 {
        let other = synthetic(50, 100 + seed);
        engine.open_bytes(&other.encode()).unwrap();
    }
    for w in workers {
        w.join().unwrap();
    }
    assert_eq!(engine.layers(), 5);
}

#[test]
fn the_handle_is_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<Engine>();
}

/// `bindings/fixtures/synth.tgd`, the pack every language's parity test opens, is the
/// synthetic model's, byte for byte; `TEIGEODATA_WRITE_FIXTURE=1` rewrites it.
#[test]
fn the_bindings_fixture_is_current() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../bindings/fixtures/synth.tgd"
    );
    let bytes = fixture().encode();
    if std::env::var_os("TEIGEODATA_WRITE_FIXTURE").is_some() {
        std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
        std::fs::write(path, &bytes).unwrap();
    }
    let on_disk = std::fs::read(path).unwrap();
    assert!(
        on_disk == bytes,
        "{path} is stale: rerun with TEIGEODATA_WRITE_FIXTURE=1"
    );
}

/// `bindings/fixtures/dem.tgd`: one DEM tile (64 × 64 samples of 1″, Copernicus GLO-30) at
/// tile row 3,496 and column 14,400 (27.83° to 27.85° N, 76.00° to 76.02° E), sloping east.
fn dem_fixture() -> Vec<u8> {
    use teigeodata_format::dem::{self, DATUM_EGM2008, Family, Grid, GridInput, THRESHOLDS};
    use teigeodata_format::{Encoding, Kind, Writer};
    let family = Family::standard();
    let q: Vec<i32> = (0..4096).map(|i| 500 + (i % 64) / 8).collect();
    let tile = dem::encode_tile(&q, 6, &family, &THRESHOLDS, &dem::TileOptions::default()).unwrap();
    let grid = Grid {
        tile_row0: 3_496,
        tile_col0: 14_400,
        tile_rows: 1,
        tile_cols: 1,
        lat_step: 10,
        lon_step: 10,
        step_mm: 2_000,
        datum: DATUM_EGM2008,
        source: dem::Source::CopernicusGlo30.code(),
        tile_log2: 6,
        first_tile: 0,
    };
    let tiles = [Some(tile.as_slice())];
    let body = dem::encode(
        &[GridInput {
            grid,
            tiles: &tiles,
        }],
        &family,
        &THRESHOLDS,
    )
    .unwrap();
    Writer::new("zz", "places", "2026.11.0")
        .unwrap()
        .pack(&[(Kind::DEM, Encoding::TABLE, 1, &body)], None)
        .unwrap()
}

#[test]
fn the_dem_fixture_is_current_and_answers_heights() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../bindings/fixtures/dem.tgd"
    );
    let bytes = dem_fixture();
    if std::env::var_os("TEIGEODATA_WRITE_FIXTURE").is_some() {
        std::fs::write(path, &bytes).unwrap();
    }
    assert!(std::fs::read(path).unwrap() == bytes, "{path} is stale");
    let engine = Engine::new();
    engine.open_bytes(&bytes).unwrap();
    let h = engine.elevation_at(27.84, 76.01).unwrap();
    let r = Reader::new(h.as_bytes()).unwrap();
    assert_eq!(
        r.names().collect::<Vec<_>>(),
        ["meters", "source", "resolution_m", "datum"]
    );
    let m = f64s(&h, "meters")[0];
    assert!((1_000.0..1_020.0).contains(&m), "{m}");
    assert_eq!(strings(&h, "source"), [Some("copernicus-glo30".to_owned())]);
    assert_eq!(strings(&h, "datum"), [Some("EGM2008".to_owned())]);
    assert!((f64s(&h, "resolution_m")[0] - 30.9).abs() < 0.1);
    // Off the grid: no row, every column still there.
    let none = engine.elevation_at(27.5, 85.5).unwrap();
    assert_eq!(rows(&none), 0);
    assert_eq!(Reader::new(none.as_bytes()).unwrap().names().count(), 4);
    let e = engine.elevation_at(95.0, 0.0).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);
}

/// The engine section of `bindings/parity.json`, which every language checks, holds what Rust
/// answers on the fixture.
#[test]
fn the_parity_file_matches_rust() {
    let parity: serde_json::Value =
        serde_json::from_str(include_str!("../../../bindings/parity.json")).unwrap();
    let p = &parity["engine"];
    let s = |v: &serde_json::Value| v.as_str().unwrap().to_owned();
    let n = |v: &serde_json::Value| v.as_f64().unwrap();
    let ids = |v: &serde_json::Value| -> Vec<Option<String>> {
        v.as_array().unwrap().iter().map(|x| Some(s(x))).collect()
    };
    let engine = Engine::new();
    let path = format!(
        "{}/../../bindings/{}",
        env!("CARGO_MANIFEST_DIR"),
        s(&p["fixture"])
    );
    assert_eq!(engine.open_file(&path), Ok(1));

    let g = &p["get"];
    let blob = engine.get(&s(&g["id"])).unwrap();
    let r = Reader::new(blob.as_bytes()).unwrap();
    assert_eq!(
        serde_json::json!(r.names().collect::<Vec<_>>()),
        p["columns"]
    );
    assert_eq!(strings(&blob, "name"), [Some(s(&g["name"]))]);
    assert_eq!(strings(&blob, "kind"), [Some(s(&g["kind"]))]);
    assert_eq!(f64s(&blob, "lat"), [n(&g["lat"])]);
    assert_eq!(f64s(&blob, "lon"), [n(&g["lon"])]);

    let q = &p["search"];
    let hits = engine
        .search(&s(&q["query"]), q["limit"].as_u64().unwrap() as u32)
        .unwrap();
    assert_eq!(strings(&hits, "id")[0], Some(s(&q["first_id"])));

    let v = &p["reverse"];
    let near = engine
        .reverse(
            n(&v["lat"]),
            n(&v["lon"]),
            v["limit"].as_u64().unwrap() as u32,
        )
        .unwrap();
    assert_eq!(strings(&near, "id"), ids(&v["ids"]));
    assert!((f64s(&near, "meters")[0] - n(&v["first_meters"])).abs() < 0.1);

    let a = &p["admin_at"];
    let units = engine.admin_at(n(&a["lat"]), n(&a["lon"])).unwrap();
    assert_eq!(strings(&units, "id"), ids(&a["ids"]));

    for (method, call) in [
        (
            "parent",
            Engine::parent as fn(&Engine, &str) -> teigeodata_api::Result<Blob>,
        ),
        ("ancestors", Engine::ancestors),
        ("children", Engine::children),
    ] {
        let c = &p[method];
        let got = call(&engine, &s(&c["id"])).unwrap();
        assert_eq!(strings(&got, "id"), ids(&c["ids"]), "{method}");
    }
    let c = &p["nearby"];
    let got = engine
        .nearby(&s(&c["id"]), c["limit"].as_u64().unwrap() as u32)
        .unwrap();
    assert_eq!(strings(&got, "id"), ids(&c["ids"]));
    let c = &p["within_radius"];
    let got = engine
        .within_radius(
            n(&c["lat"]),
            n(&c["lon"]),
            n(&c["meters"]),
            c["limit"].as_u64().unwrap() as u32,
        )
        .unwrap();
    assert_eq!(strings(&got, "id"), ids(&c["ids"]));
    let c = &p["tz_at"];
    let got = engine.tz_at(n(&c["lat"]), n(&c["lon"])).unwrap();
    assert_eq!(strings(&got, "tz"), [Some(s(&c["tz"]))]);
    let c = &p["by_iso"];
    let got = engine.by_iso(&s(&c["code"])).unwrap();
    assert_eq!(strings(&got, "id"), [Some(s(&c["id"]))]);
    assert_eq!(strings(&got, "iso"), [Some(s(&c["code"]).to_uppercase())]);
    let e = engine.by_iso(&s(&p["bad_iso"])).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);

    let c = &p["elevation_at"];
    let dem = format!(
        "{}/../../bindings/{}",
        env!("CARGO_MANIFEST_DIR"),
        s(&c["fixture"])
    );
    assert_eq!(engine.open_file(&dem), Ok(2));
    let got = engine.elevation_at(n(&c["lat"]), n(&c["lon"])).unwrap();
    assert!((f64s(&got, "meters")[0] - n(&c["meters"])).abs() < 1e-6);
    assert_eq!(strings(&got, "source"), [Some(s(&c["source"]))]);

    let e = engine.get(&s(&p["bad_id"])).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);
    for (name, code) in [("Io", ErrorCode::Io), ("Corrupt", ErrorCode::Corrupt)] {
        assert_eq!(parity["error_codes"][name], code as i32);
    }
}

#[test]
fn the_place_graph_radius_zone_and_codes() {
    let (engine, model) = open();
    let country = model.ids[country(&model)].to_string();
    let children = strings(&engine.children(&country).unwrap(), "id");
    assert_eq!(children.len(), 8, "{children:?}");
    // Every child's parent is the country, and its ancestors end there.
    for child in children.iter().flatten() {
        assert_eq!(
            strings(&engine.parent(child).unwrap(), "id"),
            [Some(country.clone())]
        );
        let up = strings(&engine.ancestors(child).unwrap(), "id");
        assert_eq!(up.last(), Some(&Some(country.clone())));
    }
    assert_eq!(
        rows(&engine.parent(&country).unwrap()),
        0,
        "a root has no parent"
    );
    assert_eq!(rows(&engine.ancestors("tgd:np/zzzzzz").unwrap()), 0);
    assert_eq!(rows(&engine.children("tgd:np/zzzzzz").unwrap()), 0);

    // nearby never answers the place itself; within_radius is reverse with a cap.
    let some = model.ids[model.ids.len() - 1].to_string();
    let near = strings(&engine.nearby(&some, 5).unwrap(), "id");
    assert_eq!(near.len(), 5);
    assert!(!near.contains(&Some(some.clone())));
    let p = model.points[model.points.len() - 1];
    let (lat, lon) = (p.lat_degrees(), p.lon_degrees());
    let all = f64s(&engine.reverse(lat, lon, 50).unwrap(), "meters");
    let cap = all[10];
    let within = f64s(&engine.within_radius(lat, lon, cap, 50).unwrap(), "meters");
    assert!(
        within.len() >= 11 && within.iter().all(|m| *m <= cap),
        "{within:?}"
    );
    assert_eq!(rows(&engine.within_radius(lat, lon, 0.0, 5).unwrap()), 1);
    for bad in [-1.0, f64::NAN, f64::INFINITY] {
        let e = engine.within_radius(lat, lon, bad, 5).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidArgument, "{bad}");
    }

    let zone = engine.tz_at(lat, lon).unwrap();
    assert_eq!(rows(&zone), 1);
    assert!(strings(&zone, "tz")[0].is_some());
    assert_eq!(rows(&engine.by_iso("FR").unwrap()), 0);
}
