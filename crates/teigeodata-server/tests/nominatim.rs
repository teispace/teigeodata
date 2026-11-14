//! The Nominatim conformance corpus (ADR-0038, D-314): every request in `nominatim.json`, as a
//! client sends it, against the server; each field the client reads is there with its type, and
//! the fields we cannot fill are left out.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "tests: failures panic with their message"
)]

#[path = "../../teigeodata-engine/tests/common/mod.rs"]
mod common;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use serde_json::Value;
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer};
use teigeodata_server::http::{App, Config, router};
use teigeodata_server::packs::Packs;
use teigeodata_server::seams::Seams;
use tower::ServiceExt;

/// Whether `v` is of corpus type `ty`.
fn is(v: &Value, ty: &Value, id: &str) -> bool {
    let numstr = |v: &Value| {
        v.as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .is_some_and(f64::is_finite)
    };
    match ty.as_str() {
        Some("numstr") => numstr(v),
        Some("string") => v.is_string(),
        Some("object") => v.is_object(),
        Some("number") => v.is_number(),
        Some("bbox") => v
            .as_array()
            .is_some_and(|a| a.len() == 4 && a.iter().all(numstr)),
        Some("{id}") => v.as_str() == Some(id),
        _ => v == ty,
    }
}

#[tokio::test]
async fn the_clients_requests_get_what_they_read() {
    let model = common::synthetic(300, 7);
    let dir = std::env::temp_dir().join(format!("tgd-server-nominatim-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("np.tgd");
    let encoded = model.encode();
    let pack = Pack::open(&encoded).unwrap();
    let sections: Vec<(Kind, Encoding, u64, &[u8])> = pack
        .sections()
        .filter(|s| s.kind() != Kind::BLOCK_HASHES)
        .map(|s| (s.kind(), s.encoding(), s.item_count(), s.bytes()))
        .collect();
    let mut w = Writer::new("np", "places", "2026.11.0").unwrap();
    w.manifest(Tag::ATTRIBUTION, b"Synthetic Source, ODbL")
        .unwrap();
    std::fs::write(&path, w.pack(&sections, None).unwrap()).unwrap();
    let (packs, _) = Packs::open(std::slice::from_ref(&path), None).unwrap();
    let app = Arc::new(App {
        packs: Arc::new(packs),
        seams: Seams::default(),
        config: Config::default(),
    });
    let place = app.packs.load().get(model.ids[0]).unwrap().unwrap();
    let id = place.id.to_string();
    let (lat, lon) = (place.point.lat_degrees(), place.point.lon_degrees());

    let corpus: Value =
        serde_json::from_str(include_str!("nominatim.json")).expect("the corpus parses");
    let requests = corpus["requests"].as_array().unwrap();
    assert!(requests.len() >= 15);
    for r in requests {
        let client = r["client"].as_str().unwrap();
        let uri = r["request"]
            .as_str()
            .unwrap()
            .replace("{id}", &id)
            .replace("{lat}", &lat.to_string())
            .replace("{lon}", &lon.to_string());
        let mut req = Request::get(&uri).body(Body::empty()).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 9))));
        let res = router(Arc::clone(&app)).oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let body = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&body)
            .unwrap_or_else(|e| panic!("{client}: {uri}: not JSON ({e})"));
        assert_eq!(
            Some(u64::from(status)),
            r["status"].as_u64(),
            "{client}: {uri}: {v}"
        );

        let each: Vec<&Value> = match r["results"].as_str().unwrap() {
            "array" => {
                let a = v
                    .as_array()
                    .unwrap_or_else(|| panic!("{client}: not an array: {v}"));
                let min = r["min"].as_u64().unwrap_or(0);
                let max = r["max"].as_u64().unwrap_or(u64::MAX);
                let n = a.len() as u64;
                assert!(min <= n && n <= max, "{client}: {n} results: {v}");
                a.iter().collect()
            }
            "object" | "error" => {
                assert!(v.is_object(), "{client}: not an object: {v}");
                vec![&v]
            }
            other => panic!("{client}: results {other:?}"),
        };
        for item in each {
            for (pointer, ty) in r["reads"].as_object().into_iter().flatten() {
                let got = item
                    .pointer(pointer)
                    .unwrap_or_else(|| panic!("{client}: {uri}: {pointer} is missing: {item}"));
                assert!(is(got, ty, &id), "{client}: {pointer} is {got}, not {ty}");
            }
            for pointer in r["absent"].as_array().into_iter().flatten() {
                let p = pointer.as_str().unwrap();
                assert!(item.pointer(p).is_none(), "{client}: {p} is there: {item}");
            }
        }
    }
    drop(app);
    std::fs::remove_dir_all(&dir).unwrap();
}
