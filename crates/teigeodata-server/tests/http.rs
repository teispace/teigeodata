//! The HTTP server's gates (phase 6.2): every route answers its native body with attribution,
//! every failure its code and status, a panic answers `internal` and the server goes on, the
//! seams are called (refused before the engine runs, metered after), and CORS is off by default.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "tests: failures panic with their message"
)]

#[path = "../../teigeodata-engine/tests/common/mod.rs"]
mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer};
use teigeodata_server::http::{App, Config, router};
use teigeodata_server::packs::Packs;
use teigeodata_server::query::{Code, QueryError};
use teigeodata_server::seams::{Authorize, Caller, Meter, Operation, Seams, Usage};
use tower::ServiceExt;

const NOTICE: &str = "Synthetic Source, ODbL";

fn app(seams: Seams) -> (Arc<App>, common::Model, std::path::PathBuf) {
    let model = common::synthetic(300, 7);
    let p = model.encode();
    let pack = Pack::open(&p).unwrap();
    let sections: Vec<(Kind, Encoding, u64, &[u8])> = pack
        .sections()
        .filter(|s| s.kind() != Kind::BLOCK_HASHES)
        .map(|s| (s.kind(), s.encoding(), s.item_count(), s.bytes()))
        .collect();
    let mut w = Writer::new("np", "places", "2026.11.0").unwrap();
    w.manifest(Tag::ATTRIBUTION, NOTICE.as_bytes()).unwrap();
    let bytes = w.pack(&sections, None).unwrap();
    let dir = std::env::temp_dir().join(format!(
        "tgd-server-http-{}-{}",
        std::process::id(),
        rand_tag()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("np.tgd");
    std::fs::write(&path, bytes).unwrap();
    let (packs, _) = Packs::open(std::slice::from_ref(&path), None).unwrap();
    let app = Arc::new(App {
        packs: Arc::new(packs),
        seams,
        config: Config::default(),
    });
    (app, model, dir)
}

fn rand_tag() -> usize {
    static N: AtomicUsize = AtomicUsize::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

async fn call(app: &Arc<App>, uri: &str, origin: Option<&str>) -> (StatusCode, Value, String) {
    let mut req = Request::get(uri);
    if let Some(o) = origin {
        req = req.header("origin", o);
    }
    let mut req = req.body(Body::empty()).unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 9))));
    let res = router(Arc::clone(app)).oneshot(req).await.unwrap();
    let status = res.status();
    let cors = res
        .headers()
        .get("access-control-allow-origin")
        .map(|v| v.to_str().unwrap().to_owned())
        .unwrap_or_default();
    let body = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    let json = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, json, cors)
}

#[tokio::test]
async fn every_route_answers_with_its_attribution() {
    let (app, model, dir) = app(Seams::default());
    let id = model.ids[0].to_string();
    let row = (0..model.ids.len())
        .find(|&r| model.parents.get(r).copied().flatten().is_some())
        .unwrap();
    let leaf = model.ids[row].to_string();
    let parent = model.ids[model.parents[row].unwrap() as usize].to_string();
    let (s, j, _) = call(&app, "/health", None).await;
    assert_eq!((s, j["layers"].as_u64()), (StatusCode::OK, Some(1)));

    for uri in [
        format!("/v1/get?id={id}"),
        format!("/v1/lookup?ids={id},{id}"),
        "/v1/reverse?lat=27.5&lon=85&limit=3".to_owned(),
        format!("/v1/near?id={id}&limit=3"),
        "/v1/search?q=Synthland&limit=3".to_owned(),
        "/v1/search?q=Synthland&focus=27.5,85&scale_km=20&strict=1".to_owned(),
        "/v1/admin?lat=27.5&lon=85".to_owned(),
        format!("/v1/hierarchy?id={leaf}"),
        format!("/v1/children?id={parent}&limit=3"),
        "/v1/tz?lat=27.5&lon=85&t=0".to_owned(),
    ] {
        let (s, j, _) = call(&app, &uri, None).await;
        assert_eq!(s, StatusCode::OK, "{uri}: {j}");
        let n = j["results"].as_array().unwrap().len();
        assert!(n > 0, "{uri}: {j}");
        assert_eq!(j["attribution"], NOTICE, "{uri}");
    }
    // Nothing found: 200, an empty list and no attribution.
    let (s, j, _) = call(&app, "/v1/search?q=zzzzzzzz", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(j, serde_json::json!({ "results": [], "attribution": null }));
    // GeoJSON carries the same notice.
    let (s, j, _) = call(&app, "/v1/search?q=Synthland&format=geojson", None).await;
    assert_eq!(
        (s, &j["type"], &j["attribution"]),
        (
            StatusCode::OK,
            &Value::from("FeatureCollection"),
            &Value::from(NOTICE)
        )
    );
    // A distance, and an outline (a point place's is its point), as GeoJSON always.
    let (s, j, _) = call(&app, &format!("/v1/distance?from={id}&to=27.5,85"), None).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert!(j["result"]["meters"].as_f64().unwrap() > 0.0, "{j}");
    assert_eq!(
        (
            &j["result"]["from"]["id"],
            &j["result"]["to"]["id"],
            &j["attribution"]
        ),
        (
            &Value::from(id.as_str()),
            &Value::Null,
            &Value::from(NOTICE)
        ),
        "{j}"
    );
    let (s, j, _) = call(&app, &format!("/v1/boundary?id={id}&format=json"), None).await;
    assert_eq!(
        (s, &j["type"], &j["id"], &j["attribution"]),
        (
            StatusCode::OK,
            &Value::from("Feature"),
            &Value::from(id.as_str()),
            &Value::from(NOTICE)
        ),
        "{j}"
    );
    let (s, j, _) = call(&app, "/packs", None).await;
    assert_eq!(
        (s, &j["packs"][0]["attribution"], &j["packs"][0]["places"]),
        (
            StatusCode::OK,
            &Value::from(NOTICE),
            &Value::from(model.ids.len())
        ),
        "{j}"
    );
    drop(app); // A mapped pack cannot be removed on Windows.
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn every_failure_answers_its_code_and_status() {
    let (app, model, dir) = app(Seams::default());
    let absent = common::synthetic(300, 99).ids[0];
    assert!(!model.ids.contains(&absent));
    let long = format!("/v1/search?q={}", "a".repeat(5000));
    for (uri, status, code) in [
        ("/v1/search?q=x&limit=51", 400, "out_of_bounds"),
        ("/v1/search?q=x&limit=ten", 400, "invalid_argument"),
        ("/v1/search", 400, "invalid_argument"),
        ("/v1/get?id=nope", 400, "invalid_argument"),
        (&*format!("/v1/get?id={absent}"), 404, "not_found"),
        ("/v1/admin?lat=91&lon=0", 400, "invalid_argument"),
        ("/v1/near?id=x&radius_m=0", 400, "out_of_bounds"),
        (
            "/v1/elevation?lat=27&lon=85&format=geojson",
            400,
            "invalid_argument",
        ),
        (
            &*format!("/v1/boundary?id={}&lod=7", model.ids[0]),
            400,
            "invalid_argument",
        ),
        ("/v1/boundary?lod=1", 400, "invalid_argument"),
        ("/v1/distance?from=27,85", 400, "invalid_argument"),
        ("/v1/distance?from=91,0&to=0,0", 400, "invalid_argument"),
        (
            &*format!("/v1/distance?from={absent}&to=0,0"),
            404,
            "not_found",
        ),
        ("/v1/nothing", 404, "not_found"),
        ("/nothing", 404, "not_found"),
        (&*long, 414, "out_of_bounds"),
    ] {
        let (s, j, _) = call(&app, uri, None).await;
        assert_eq!(
            (s.as_u16(), j["error"]["code"].as_str()),
            (status, Some(code)),
            "{uri}: {j}"
        );
    }
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}

/// Refuses searches; panics on `admin`.
struct Gate;

impl Authorize for Gate {
    fn authorize(&self, _: &Caller, op: Operation) -> Result<(), QueryError> {
        match op {
            Operation::Search => Err(QueryError::new(Code::RateLimited, "slow down")),
            Operation::Admin => panic!("a seam failed"),
            _ => Ok(()),
        }
    }
}

#[derive(Default)]
struct Count(AtomicUsize, AtomicUsize);

impl Meter for Count {
    fn record(&self, _: &Caller, _: Operation, usage: Usage) {
        self.0.fetch_add(1, Ordering::Relaxed);
        self.1.fetch_add(usage.results, Ordering::Relaxed);
    }
}

#[tokio::test]
async fn the_seams_gate_and_meter_and_a_panic_is_answered() {
    let count = Arc::new(Count::default());
    let (app, model, dir) = app(Seams {
        authorize: Arc::new(Gate),
        meter: Arc::clone(&count) as Arc<dyn Meter>,
        ..Seams::default()
    });
    let (s, j, _) = call(&app, "/v1/search?q=Synthland", None).await;
    assert_eq!(
        (s.as_u16(), j["error"]["code"].as_str()),
        (429, Some("rate_limited"))
    );
    let (s, j, _) = call(&app, "/v1/admin?lat=27.5&lon=85", None).await;
    assert_eq!(
        (s.as_u16(), j["error"]["code"].as_str()),
        (500, Some("internal"))
    );
    // The server goes on.
    let id = model.ids[0].to_string();
    let (s, _, _) = call(&app, &format!("/v1/lookup?ids={id},{id}"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        (
            count.0.load(Ordering::Relaxed),
            count.1.load(Ordering::Relaxed)
        ),
        (1, 2),
        "only the answered request is metered, with its results"
    );
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn cors_is_off_unless_configured() {
    let (open, _, dir) = app(Seams::default());
    let (_, _, cors) = call(&open, "/health", Some("https://app.example")).await;
    assert_eq!(cors, "");
    let configured = Arc::new(App {
        packs: Arc::new(Packs::open(&[dir.join("np.tgd")], None).unwrap().0),
        seams: Seams::default(),
        config: Config {
            cors_origins: vec!["https://app.example".parse().unwrap()],
            ..Config::default()
        },
    });
    let (_, _, cors) = call(&configured, "/health", Some("https://app.example")).await;
    assert_eq!(cors, "https://app.example");
    let (_, _, cors) = call(&configured, "/health", Some("https://other.example")).await;
    assert_eq!(cors, "");
    drop((open, configured));
    std::fs::remove_dir_all(dir).unwrap();
}
