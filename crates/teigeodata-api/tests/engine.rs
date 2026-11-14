//! The `Engine` handle over a synthetic pack (the engine's own test model): every query as a
//! blob, its errors typed, and queries from many threads while packs are added (D-274).

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::many_single_char_names,
    clippy::too_many_lines,
    reason = "tests: failures panic with their message; the parity test reads the whole file"
)]

#[path = "../../teigeodata-engine/tests/common/mod.rs"]
mod common;

use std::sync::Arc;

use common::synthetic;
use teigeodata_api::{
    Blob, Engine, ErrorCode, LIMIT_MAX, Load, OpenOptions, SearchOptions, Verify,
};
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

/// The bindings' fixture model: the synthetic country, coded `NP`, with postal codes in an
/// admin-aligned scheme `np`: district `d` (by row) has the area code `1060d` and the ward
/// code `1060d01`.
fn fixture() -> common::Model {
    use teigeodata_engine::CodeKind;
    use teigeodata_format::postal::{self, Code, Rule};
    let mut model = synthetic(60, 5);
    let row = u32::try_from(country(&model)).unwrap();
    model.iso = vec![(row, "NP".parse().unwrap())];
    let rules = [Rule {
        name: "np".into(),
        admin_aligned: true,
        ignore: " -".into(),
        templates: vec!["99999".into(), "9999999".into()],
        ..Rule::default()
    }];
    let districts = model.admin.as_ref().unwrap().rows[1..].to_vec();
    let code = |text: String, place, kind| Code {
        scheme: "np".into(),
        code: text,
        place,
        kind,
        valid_from: None,
        valid_to: None,
        redirect: None,
    };
    let codes: Vec<Code> = districts
        .iter()
        .enumerate()
        .flat_map(|(d, &place)| {
            [
                code(format!("1060{d}"), place, CodeKind::Area),
                code(format!("1060{d}01"), place, CodeKind::Part),
            ]
        })
        .collect();
    let places = u32::try_from(model.ids.len()).unwrap();
    let body = postal::encode(&rules, &codes, places, true).unwrap();
    model
        .raw
        .push((teigeodata_format::Kind::POSTAL, codes.len() as u64, body));
    model
}

fn open() -> (Engine, common::Model) {
    let model = synthetic(400, 11);
    let engine = Engine::new();
    assert_eq!(engine.layers(), 0);
    assert_eq!(engine.open_bytes(&model.encode()), Ok(0));
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
    assert_eq!(engine.open_file(&path), Ok(0));

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

    let w = &p["search_with"];
    let options = SearchOptions {
        focus_lat: Some(n(&w["focus_lat"])),
        focus_lon: Some(n(&w["focus_lon"])),
        focus_scale_m: Some(n(&w["focus_scale_m"])),
        strict: false,
    };
    let limit = w["limit"].as_u64().unwrap() as u32;
    let got = engine
        .search_with(&s(&w["query"]), limit, &options)
        .unwrap();
    assert_eq!(strings(&got, "id"), ids(&w["ids"]));
    let plain = engine.search(&s(&w["query"]), limit).unwrap();
    assert_ne!(
        strings(&plain, "id"),
        ids(&w["ids"]),
        "the focus must matter"
    );

    // Open options: a flipped section byte, found at open by eager verification only.
    let o = &p["open_with"];
    let good = std::fs::read(&path).unwrap();
    let at = {
        let pack = teigeodata_format::Pack::open(&good).unwrap();
        let section = pack.section(teigeodata_format::Kind::ELEVATION).unwrap();
        section.bytes().as_ptr() as usize - good.as_ptr() as usize + section.bytes().len() / 2
    };
    assert_eq!(o["corrupt_at"].as_u64(), Some(at as u64), "corrupt_at");
    let mut bad = good;
    bad[at] ^= 0xff;
    let eager = OpenOptions {
        verify: Verify::Eager,
        ..OpenOptions::default()
    };
    let scratch = Engine::new();
    assert_eq!(
        scratch.open_bytes_with(&bad, &eager).unwrap_err().code,
        ErrorCode::Corrupt
    );
    assert_eq!(scratch.open_bytes(&bad), Ok(0));
    let deep = OpenOptions {
        outline_level: Some(u32::try_from(o["bad_outline_level"].as_u64().unwrap()).unwrap()),
        ..OpenOptions::default()
    };
    assert_eq!(
        scratch.open_file_with(&path, &deep).unwrap_err().code,
        ErrorCode::InvalidArgument
    );

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
    assert_eq!(engine.open_file(&dem), Ok(1));
    let got = engine.elevation_at(n(&c["lat"]), n(&c["lon"])).unwrap();
    assert!((f64s(&got, "meters")[0] - n(&c["meters"])).abs() < 1e-6);
    assert_eq!(strings(&got, "source"), [Some(s(&c["source"]))]);

    let c = &p["postal_lookup"];
    let got = engine
        .postal_lookup(&s(&c["code"]), &s(&c["scheme"]))
        .unwrap();
    assert_eq!(strings(&got, "postal"), ids(&c["postal"]));
    assert_eq!(strings(&got, "id"), ids(&c["ids"]));
    let c = &p["postal_prefix"];
    let got = engine
        .postal_prefix(
            &s(&c["input"]),
            &s(&c["scheme"]),
            c["limit"].as_u64().unwrap() as u32,
        )
        .unwrap();
    assert_eq!(strings(&got, "postal"), ids(&c["postal"]));
    let c = &p["postal_at"];
    let got = engine.postal_at(n(&c["lat"]), n(&c["lon"])).unwrap();
    assert_eq!(strings(&got, "postal"), ids(&c["postal"]));
    assert_eq!(strings(&got, "code_kind"), ids(&c["code_kind"]));
    let c = &p["postal_codes_of"];
    let got = engine.postal_codes_of(&s(&c["id"])).unwrap();
    assert_eq!(strings(&got, "postal"), ids(&c["postal"]));

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

#[test]
fn postal_codes_answer_with_their_places() {
    let engine = Engine::new();
    engine.open_bytes(&fixture().encode()).unwrap();
    let hit = engine.postal_lookup("106 01", "").unwrap();
    let r = Reader::new(hit.as_bytes()).unwrap();
    let names: Vec<_> = r.names().collect();
    assert_eq!(
        names[names.len() - 5..],
        ["postal", "scheme", "code_kind", "valid_from", "valid_to"]
    );
    assert_eq!(strings(&hit, "postal"), [Some("10601".to_owned())]);
    assert_eq!(strings(&hit, "scheme"), [Some("np".to_owned())]);
    let nulls = r.nulls("valid_to").unwrap();
    assert!(nulls.is_null(0), "no end date");
    // Another scheme, an unknown code, an empty prefix: no rows.
    assert_eq!(rows(&engine.postal_lookup("10601", "gb").unwrap()), 0);
    assert_eq!(rows(&engine.postal_lookup("99999", "").unwrap()), 0);
    assert_eq!(rows(&engine.postal_prefix("", "", 5).unwrap()), 0);
    assert_eq!(rows(&engine.postal_prefix("1060", "", 4).unwrap()), 4);
    assert_eq!(
        engine
            .postal_prefix("1", "", LIMIT_MAX + 1)
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    // A pack without postal codes answers none, not an error.
    let (plain, _) = open();
    assert_eq!(rows(&plain.postal_lookup("10601", "").unwrap()), 0);
    assert_eq!(rows(&plain.postal_at(27.5, 85.5).unwrap()), 0);
}

#[test]
fn packs_are_removed_by_their_layer_id() {
    let (engine, model) = open();
    let id = model.ids[country(&model)].to_string();
    let other = engine.open_bytes(&synthetic(50, 3).encode()).unwrap();
    assert_eq!((other, engine.layers()), (1, 2));
    // A held snapshot keeps answering while its pack goes.
    let blob = engine.get(&id).unwrap();
    assert!(engine.remove(0));
    assert!(!engine.remove(0), "already gone");
    assert!(!engine.remove(7), "never added");
    assert_eq!(engine.layers(), 1);
    assert_eq!(strings(&blob, "name"), [Some("Synthland".to_owned())]);
    // IDs are not reused.
    assert_eq!(engine.open_bytes(&model.encode()), Ok(2));
    assert!(engine.remove(other) && engine.remove(2));
    assert_eq!(engine.layers(), 0);
    assert_eq!(rows(&engine.get(&id).unwrap()), 0);
}

#[test]
fn a_focus_ranks_the_nearest_of_same_named_places_first() {
    let (engine, model) = open();
    // A name several places share: the focus at each of them puts that one first.
    let mut rows_of = std::collections::BTreeMap::<&str, Vec<usize>>::new();
    for (row, names) in model.names.iter().enumerate() {
        if let Some(n) = names.first() {
            rows_of.entry(n.text.as_str()).or_default().push(row);
        }
    }
    let (shared, rows_named) = rows_of.into_iter().find(|(_, r)| r.len() > 1).unwrap();
    let shared = shared.to_owned();
    let same: Vec<String> = rows_named
        .iter()
        .map(|&r| model.ids[r].to_string())
        .collect();
    let rank = |b: &Blob, id: &str| {
        strings(b, "id")
            .iter()
            .position(|x| x.as_deref() == Some(id))
    };
    let base = engine.search(&shared, 50).unwrap();
    let mut lifted = 0;
    for id in &same {
        let row = model.ids.iter().position(|p| p.to_string() == *id).unwrap();
        let options = SearchOptions {
            focus_lat: Some(model.points[row].lat_degrees()),
            focus_lon: Some(model.points[row].lon_degrees()),
            focus_scale_m: Some(1_000.0),
            strict: false,
        };
        let got = engine.search_with(&shared, 50, &options).unwrap();
        let (before, after) = (rank(&base, id).unwrap(), rank(&got, id).unwrap());
        assert!(
            after <= before,
            "{shared}: {id} fell from {before} to {after}"
        );
        lifted += usize::from(after < before);
    }
    assert!(lifted > 0, "a focus lifted none of {same:?}");
    let strict = SearchOptions {
        strict: true,
        ..SearchOptions::default()
    };
    assert!(rows(&engine.search_with(&shared, 5, &strict).unwrap()) >= 2);
    let bad = |o: SearchOptions| engine.search_with("x", 1, &o).unwrap_err().code;
    let half = SearchOptions {
        focus_lat: Some(1.0),
        ..SearchOptions::default()
    };
    assert_eq!(bad(half), ErrorCode::InvalidArgument);
    let zero = SearchOptions {
        focus_scale_m: Some(0.0),
        ..SearchOptions {
            focus_lon: Some(1.0),
            ..half
        }
    };
    assert_eq!(bad(zero), ErrorCode::InvalidArgument);
    let off = SearchOptions {
        focus_lat: Some(91.0),
        ..SearchOptions {
            focus_lon: Some(1.0),
            ..half
        }
    };
    assert_eq!(bad(off), ErrorCode::InvalidArgument);
}

#[test]
fn open_options_choose_verification_and_outline_levels() {
    let model = synthetic(60, 5);
    let good = model.encode();
    // One flipped byte inside the elevation section: found at open only when verifying
    // eagerly; a lazy open checks it the first time a query reads it.
    let at = {
        let pack = teigeodata_format::Pack::open(&good).unwrap();
        let section = pack.section(teigeodata_format::Kind::ELEVATION).unwrap();
        section.bytes().as_ptr() as usize - good.as_ptr() as usize + section.bytes().len() / 2
    };
    let mut bad = good.clone();
    bad[at] ^= 0xff;
    let engine = Engine::new();
    let eager = OpenOptions {
        verify: Verify::Eager,
        ..OpenOptions::default()
    };
    assert_eq!(
        engine.open_bytes_with(&bad, &eager).unwrap_err().code,
        ErrorCode::Corrupt
    );
    assert_eq!(engine.open_bytes(&bad), Ok(0));
    assert_eq!(engine.open_bytes_with(&good, &eager), Ok(1));

    // A pack that does not split its outlines by level reads them all, whatever the level
    // (the engine's outline tests cover packs that do).
    let p = model.points[model.points.len() - 1];
    let (lat, lon) = (p.lat_degrees(), p.lon_degrees());
    let every = Engine::new();
    every.open_bytes(&good).unwrap();
    let top = Engine::new();
    let level0 = OpenOptions {
        outline_level: Some(0),
        outline_tolerance_m: 1000,
        load: Load::Read,
        ..OpenOptions::default()
    };
    top.open_bytes_with(&good, &level0).unwrap();
    assert_eq!(
        every.admin_at(lat, lon).unwrap().as_bytes(),
        top.admin_at(lat, lon).unwrap().as_bytes()
    );

    let deep = OpenOptions {
        outline_level: Some(256),
        ..OpenOptions::default()
    };
    assert_eq!(
        top.open_bytes_with(&good, &deep).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
}
