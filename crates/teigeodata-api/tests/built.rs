//! A build without components (D-291): the same functions, answering as a layer installed
//! without the component does. Runs under any feature set; CI runs it with none and with all.
//! The queries run on the bindings' fixture, a whole pack.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "tests: failures panic with their message"
)]

use std::path::Path;

use teigeodata_api::{Blob, Engine, ErrorCode, Result};
use teigeodata_blob::Reader;

fn engine() -> Engine {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings/fixtures/synth.tgd");
    let engine = Engine::new();
    engine.open_file(fixture.to_str().unwrap()).unwrap();
    engine
}

fn rows(blob: &Blob) -> usize {
    Reader::new(blob.as_bytes()).unwrap().rows()
}

/// `component` is built: the query answers with rows; otherwise it fails with
/// `MissingComponent`.
fn answers(built: &[&str], component: &str, what: &str, r: Result<Blob>) {
    match r {
        Ok(b) if built.contains(&component) => assert!(rows(&b) > 0, "{what}: no rows"),
        Ok(_) => panic!("{what} answered without `{component}` built"),
        Err(e) if built.contains(&component) => panic!("{what}: {e}"),
        Err(e) => assert_eq!(e.code, ErrorCode::MissingComponent, "{what}: {e}"),
    }
}

/// `component` is built: rows; otherwise no rows (postal codes, D-133).
fn rows_if(built: &[&str], component: &str, what: &str, r: Result<Blob>) {
    let n = rows(&r.unwrap());
    assert_eq!(n > 0, built.contains(&component), "{what}: {n} rows");
}

#[test]
fn components_names_what_the_build_has() {
    let list = teigeodata_api::components();
    let built: Vec<&str> = list.split(", ").collect();
    assert_eq!(built.first(), Some(&"point"), "{list}");
    for (component, on) in [
        ("elevation", cfg!(feature = "elevation")),
        ("admin", cfg!(feature = "admin")),
        ("names", cfg!(feature = "names")),
        ("search", cfg!(feature = "search")),
        ("timezone-index", cfg!(feature = "timezone-index")),
        ("admin-index", cfg!(feature = "admin-index")),
        ("postal", cfg!(feature = "postal")),
        ("dem", cfg!(feature = "dem")),
        ("tz-rules", cfg!(feature = "tz-rules")),
        ("substring", cfg!(feature = "substring")),
    ] {
        assert_eq!(built.contains(&component), on, "{component} in {list}");
    }
}

#[test]
fn every_query_answers_as_an_absent_component() {
    let list = teigeodata_api::components();
    let built: Vec<&str> = list.split(", ").collect();
    let e = engine();
    let (country, district, village) = ("tgd:np/18n8a6", "tgd:np/avwsr1", "tgd:np/sx2ze9");

    // `point` is always built: get, reverse and nearby answer in every build.
    let got = e.get(country).unwrap();
    assert_eq!(rows(&got), 1);
    assert!(rows(&e.reverse(27.5, 85.5, 3).unwrap()) > 0);
    assert!(rows(&e.nearby(village, 3).unwrap()) > 0);

    // A place's names are null without `names`, as for a layer installed without them.
    let r = Reader::new(got.as_bytes()).unwrap();
    let name_null = r.nulls("name").is_some_and(|n| n.is_null(0));
    assert_eq!(name_null, !built.contains(&"names"), "the country's name");

    answers(&built, "admin", "parent", e.parent(district));
    answers(&built, "admin", "ancestors", e.ancestors(village));
    answers(&built, "admin", "children", e.children(country));
    answers(&built, "admin", "by_iso", e.by_iso("NP"));
    answers(&built, "search", "search", e.search("Synthland", 3));
    match teigeodata_api::fold("Lahachowk") {
        Ok(f) => assert!(built.contains(&"search") && f == "lahachowk", "fold: {f}"),
        Err(e) => assert!(
            !built.contains(&"search") && e.code == ErrorCode::MissingComponent,
            "fold: {e}"
        ),
    }
    answers(&built, "admin-index", "admin_at", e.admin_at(27.5, 85.5));
    // The fixture has no zone rules: with `tz-rules` built that is no rows, not an error.
    match e.tz_zones() {
        Ok(_) => assert!(built.contains(&"tz-rules"), "tz_zones answered without it"),
        Err(e) => assert!(
            !built.contains(&"tz-rules") && e.code == ErrorCode::MissingComponent,
            "tz_zones: {e}"
        ),
    }
    rows_if(
        &built,
        "postal",
        "postal_lookup",
        e.postal_lookup("10601", ""),
    );
    rows_if(
        &built,
        "postal",
        "postal_prefix",
        e.postal_prefix("106", "", 5),
    );
    if !built.contains(&"tz-rules") {
        assert_eq!(e.tz_version(), "");
    }

    // Arguments are still checked first.
    let bad = e.search("x", teigeodata_api::LIMIT_MAX + 1).unwrap_err();
    assert_eq!(bad.code, ErrorCode::InvalidArgument);
    let half = teigeodata_api::SearchOptions {
        focus_lat: Some(27.0),
        ..teigeodata_api::SearchOptions::default()
    };
    let bad = e.search_with("x", 1, &half).unwrap_err();
    assert_eq!(bad.code, ErrorCode::InvalidArgument);
    let bad = e.elevation_at(91.0, 0.0).unwrap_err();
    assert_eq!(bad.code, ErrorCode::InvalidArgument);
}
