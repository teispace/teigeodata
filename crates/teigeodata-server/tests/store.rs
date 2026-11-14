//! The servers over a store (D-318): `coverage` answers the countries at a point with the
//! stored catalog's packs and their state, alike over HTTP and MCP, read without opening the
//! store; a server without a store has no `coverage` tool and its route says why.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::many_single_char_names,
    clippy::panic,
    reason = "tests: failures panic with their message"
)]

#[path = "../../teigeodata-engine/tests/common/mod.rs"]
mod common;
#[path = "../../teigeodata-sync/tests/support/repo.rs"]
mod repo_support;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use repo_support::{Host, pack, publish, root_file, serve};
use serde_json::{Map, Value, json};
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer};
use teigeodata_server::http::{App, Config, router};
use teigeodata_server::mcp::Mcp;
use teigeodata_server::packs::Packs;
use teigeodata_server::query::Limits;
use teigeodata_server::seams::{Bare, Seams};
use teigeodata_sync::client::{self, Client, Want};
use tower::ServiceExt;

/// A directory of its own under the build's scratch space.
fn dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("server-store-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A store set up against a served repository holding the synthetic Nepal pack, its catalog
/// refreshed; and the pack's bytes.
fn store(name: &str) -> (PathBuf, Vec<u8>) {
    let host: Host = Arc::default();
    let bytes = pack("2026.11.0", false);
    publish(&host, 1, std::slice::from_ref(&bytes));
    let url = serve(host);
    let root = dir(name).join("store");
    client::init(
        &root,
        &[format!("{url}/meta")],
        &[format!("{url}/assets/")],
        &root_file(),
    )
    .unwrap();
    let (mut c, _) = Client::open(&root).unwrap();
    c.refresh(&["np".to_owned()]).unwrap();
    (root, bytes)
}

const LOCATOR_NOTICE: &str = "Locator Source, ODbL";

/// A pack with the synthetic country's outline, as a world pack gives the locator, noticed.
fn locator() -> Vec<u8> {
    let (model, _) = common::outlined(false);
    let p = model.encode();
    let pack = Pack::open(&p).unwrap();
    let sections: Vec<(Kind, Encoding, u64, &[u8])> = pack
        .sections()
        .filter(|s| s.kind() != Kind::BLOCK_HASHES)
        .map(|s| (s.kind(), s.encoding(), s.item_count(), s.bytes()))
        .collect();
    let mut w = Writer::new("np", "places", "2026.11.0").unwrap();
    w.manifest(Tag::ATTRIBUTION, LOCATOR_NOTICE.as_bytes())
        .unwrap();
    w.pack(&sections, None).unwrap()
}

/// A point inside the synthetic country's outline, clear of its hole.
fn inside() -> (f64, f64) {
    (
        f64::from(i32::midpoint(common::SOUTH, common::NORTH)) / 1e7,
        f64::from(i32::midpoint(common::WEST, common::EAST)) / 1e7,
    )
}

/// The HTTP app and the MCP server over `files` and the store at `root`.
fn servers(files: &[PathBuf], root: Option<&Path>) -> (Arc<App>, Mcp) {
    let packs = Arc::new(Packs::open(files, root).unwrap().0);
    let mcp = Mcp::new(Arc::clone(&packs), Seams::default(), Limits::SERVER).unwrap();
    let app = Arc::new(App {
        packs,
        seams: Seams::default(),
        config: Config::default(),
    });
    (app, mcp)
}

async fn get(app: &Arc<App>, uri: &str) -> (StatusCode, Value) {
    let mut req = Request::get(uri).body(Body::empty()).unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 9))));
    let res = router(Arc::clone(app)).oneshot(req).await.unwrap();
    let status = res.status();
    let body = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn coverage(mcp: &Mcp, a: Value) -> Value {
    let Value::Object(a) = a else { panic!() };
    let r = mcp.call("coverage", &a, &Bare).expect("the tool is listed");
    assert_eq!(r.is_error, Some(false), "{r:?}");
    r.structured_content.unwrap()
}

#[tokio::test]
async fn coverage_answers_the_stored_catalog_alike_over_http_and_mcp() {
    let (root, _) = store("coverage");
    // A world pack's outlines give the locator before anything is installed.
    let file = root.parent().unwrap().join("world.tgd");
    std::fs::write(&file, locator()).unwrap();
    let (app, mcp) = servers(std::slice::from_ref(&file), Some(&root));
    let (lat, lon) = inside();

    let (s, http) = get(&app, &format!("/v1/coverage?lat={lat}&lon={lon}")).await;
    assert_eq!(s, StatusCode::OK, "{http}");
    let row = &http["results"][0];
    assert_eq!(row["countries"], json!(["np"]), "{http}");
    let packs = row["packs"].as_array().unwrap();
    assert_eq!(packs.len(), 1, "{http}");
    assert_eq!(
        (&packs[0]["path"], &packs[0]["release"], &packs[0]["state"]),
        (
            &json!("np.places.tgd"),
            &json!("2026.11.0"),
            &json!("available")
        ),
        "{http}"
    );
    assert!(packs[0]["download"].as_u64().unwrap() > 0, "{http}");
    assert_eq!(
        http["attribution"], LOCATOR_NOTICE,
        "the pack that located it"
    );
    let mcp_answer = coverage(&mcp, json!({ "lat": lat, "lon": lon }));
    assert_eq!(mcp_answer, http, "the same answer over both");

    // Installed, the state follows the store's journal at once.
    let (mut c, _) = Client::open(&root).unwrap();
    c.install(&["np.places.tgd".to_owned()], &Want::default(), false, None)
        .unwrap();
    let after = coverage(&mcp, json!({ "lat": lat, "lon": lon }));
    assert_eq!(after["results"][0]["packs"][0]["state"], "installed");

    // Far from every country: nothing. Past the radius bound: refused.
    let (s, far) = get(&app, "/v1/coverage?lat=-60&lon=-120").await;
    assert_eq!((s, &far["results"]), (StatusCode::OK, &json!([])), "{far}");
    let (s, past) = get(
        &app,
        &format!("/v1/coverage?lat={lat}&lon={lon}&radius_m=1000001"),
    )
    .await;
    assert_eq!(
        (s, &past["error"]["code"]),
        (StatusCode::BAD_REQUEST, &json!("out_of_bounds")),
        "{past}"
    );
    drop((app, mcp));
    std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn without_a_store_there_is_no_coverage() {
    let d = dir("no-store");
    let file = d.join("np.tgd");
    std::fs::write(&file, pack("2026.11.0", false)).unwrap();
    let (app, mcp) = servers(std::slice::from_ref(&file), None);
    let (lat, lon) = inside();
    let (s, v) = get(&app, &format!("/v1/coverage?lat={lat}&lon={lon}")).await;
    assert_eq!(
        (s, &v["error"]["code"], &v["error"]["message"]),
        (
            StatusCode::NOT_FOUND,
            &json!("not_found"),
            &json!("coverage reads a store's catalog: this server serves packs without one")
        ),
        "{v}"
    );
    assert!(
        mcp.call("coverage", &Map::new(), &Bare).is_none(),
        "not listed"
    );
    assert!(
        rmcp::ServerHandler::get_tool(&mcp, "coverage").is_none(),
        "not listed"
    );
    // A store whose catalog cannot be read says so without its path.
    let (app, _) = servers(&[], Some(&d));
    let (s, v) = get(&app, &format!("/v1/coverage?lat={lat}&lon={lon}")).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert!(!v.to_string().contains(d.to_str().unwrap()), "{v}");
    drop(app);
    std::fs::remove_dir_all(d).unwrap();
}
