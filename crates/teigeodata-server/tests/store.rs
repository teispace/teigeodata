//! The servers over a store (D-318): `coverage` answers the countries at a point with the
//! stored catalog's packs and their state, alike over HTTP and MCP, read without opening the
//! store; a server without a store has no `coverage` tool and its route says why. `install`
//! fetches catalog packs into the store and serves them: by a handle and `install_status`
//! within `max_bytes`, or, for a 2026-07-28 client that can ask the person, after an MRTR
//! confirmation whose state cannot be forged, as a task polled with `tasks/get`; for a
//! 2025-11-25 client on stdio, after an `elicitation/create` the server sends it.

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
mod wire;

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
    let (root, bytes, _) = store_on(name);
    (root, bytes)
}

/// [`store`], and the host's files.
fn store_on(name: &str) -> (PathBuf, Vec<u8>, Host) {
    let host: Host = Arc::default();
    let bytes = pack("2026.11.0", false);
    publish(&host, 1, std::slice::from_ref(&bytes));
    let url = serve(Arc::clone(&host));
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
    (root, bytes, host)
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

/// A server that installs into a fresh store, serving only what the store holds.
fn installing(name: &str) -> (Mcp, PathBuf) {
    let (mcp, root, _) = installing_on(name);
    (mcp, root)
}

/// [`installing`], and the host's files.
fn installing_on(name: &str) -> (Mcp, PathBuf, Host) {
    let (root, _, host) = store_on(name);
    let packs = Arc::new(Packs::open(&[], Some(&root)).unwrap().0);
    let mcp = Mcp::installing(packs, Seams::default(), Limits::SERVER).unwrap();
    (mcp, root, host)
}

fn call(mcp: &Mcp, tool: &str, a: Value) -> rmcp::model::CallToolResult {
    let Value::Object(a) = a else { panic!() };
    mcp.call(tool, &a, &Bare).expect("the tool is listed")
}

/// The code of a failed call.
fn code(r: &rmcp::model::CallToolResult) -> String {
    assert_eq!(r.is_error, Some(true), "{r:?}");
    let text = &r.content[0].as_text().unwrap().text;
    let v: Value = serde_json::from_str(text).unwrap();
    v["error"]["code"].as_str().unwrap().to_owned()
}

/// Asks `install_status` about `handle` until the install ends (30 s at most).
fn ended(mcp: &Mcp, handle: &str) -> Value {
    for _ in 0..300 {
        let r = call(mcp, "install_status", json!({ "handle": handle }));
        let v = r.structured_content.unwrap()["result"].clone();
        if v["status"] != "working" {
            return v;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("the install did not end");
}

#[test]
fn an_install_runs_by_handle_within_max_bytes_and_is_served() {
    let (mcp, root) = installing("handle");
    for tool in ["install", "install_status"] {
        assert!(
            rmcp::ServerHandler::get_tool(&mcp, tool).is_some(),
            "{tool}"
        );
    }
    let annotations = rmcp::ServerHandler::get_tool(&mcp, "install")
        .unwrap()
        .annotations
        .unwrap();
    assert_eq!(
        (
            annotations.read_only_hint,
            annotations.destructive_hint,
            annotations.open_world_hint
        ),
        (Some(false), Some(false), Some(true))
    );
    // Nothing served yet.
    let before = call(&mcp, "packs", json!({})).structured_content.unwrap();
    assert_eq!(before["packs"], json!([]), "{before}");

    // Past `max_bytes` the call names the size and how to go on.
    let r = call(&mcp, "install", json!({ "packs": ["np"], "max_bytes": 1 }));
    assert_eq!(code(&r), "out_of_bounds");
    let text = &r.content[0].as_text().unwrap().text;
    assert!(
        text.contains("MB") && text.contains("call again with `max_bytes` at least"),
        "{text}"
    );
    for (a, want) in [
        (json!({ "packs": ["xx"] }), "not_found"),
        (
            json!({ "packs": ["np"], "components": ["nope"] }),
            "invalid_argument",
        ),
        (json!({ "packs": [] }), "invalid_argument"),
        (json!({ "packs": "np" }), "invalid_argument"),
    ] {
        assert_eq!(code(&call(&mcp, "install", a.clone())), want, "{a}");
    }

    // Within it: a handle, polled to the end, and the pack served at once.
    let r = call(&mcp, "install", json!({ "packs": ["np"] }));
    assert_eq!(r.is_error, Some(false), "{r:?}");
    let started = r.structured_content.unwrap()["result"].clone();
    let handle = started["handle"].as_str().unwrap().to_owned();
    assert_eq!(handle.len(), 32, "{started}");
    assert_eq!(started["packs"], json!(["np.places.tgd"]), "{started}");
    assert!(started["download"].as_u64().unwrap() > 0, "{started}");
    let end = ended(&mcp, &handle);
    assert_eq!(end["status"], "completed", "{end}");
    assert_eq!(
        end["result"]["installed"][0]["path"], "np.places.tgd",
        "{end}"
    );
    assert!(
        end["result"]["installed"][0]["fetched"].as_u64().unwrap() > 0,
        "{end}"
    );
    let served = call(&mcp, "packs", json!({})).structured_content.unwrap();
    assert_eq!(served["packs"][0]["address"], "np", "{served}");

    // Again: nothing to download, so no bound applies; it ends as already installed.
    let r = call(
        &mcp,
        "install",
        json!({ "packs": ["np.places"], "max_bytes": 0 }),
    );
    let again = r.structured_content.unwrap()["result"].clone();
    assert_eq!(again["download"], 0, "{again}");
    let end = ended(&mcp, again["handle"].as_str().unwrap());
    assert_eq!(end["result"]["installed"][0]["already"], true, "{end}");
    assert_eq!(
        code(&call(
            &mcp,
            "install_status",
            json!({ "handle": "0".repeat(32) })
        )),
        "not_found"
    );
    drop(mcp);
    std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
}

#[test]
fn a_failed_install_says_why_without_the_stores_path() {
    let (mcp, root, host) = installing_on("failed");
    // The mirror serves a damaged asset: its hash no longer matches the signed catalog.
    for (path, bytes) in host.lock().unwrap().iter_mut() {
        if path.starts_with("assets/") {
            for b in bytes.iter_mut().skip(64) {
                *b ^= 0x5a;
            }
        }
    }
    let r = call(&mcp, "install", json!({ "packs": ["np"] }));
    let handle = r.structured_content.unwrap()["result"]["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    let end = ended(&mcp, &handle);
    assert_eq!(end["status"], "failed", "{end}");
    assert!(end["error"]["code"].is_string(), "{end}");
    assert!(end["result"].is_null(), "{end}");
    let text = end.to_string();
    assert!(!text.contains(root.to_str().unwrap()), "{text}");
    let served = call(&mcp, "packs", json!({})).structured_content.unwrap();
    assert_eq!(served["packs"], json!([]), "nothing served: {served}");
    // A failure whose own message names the store's files says "the store" instead.
    std::fs::remove_file(root.join(client::CONFIG)).unwrap();
    let r = call(&mcp, "install", json!({ "packs": ["np"] }));
    let handle = r.structured_content.unwrap()["result"]["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    let end = ended(&mcp, &handle);
    let message = end["error"]["message"].as_str().unwrap();
    assert!(message.starts_with("the store/config.json"), "{message}");
    assert!(!message.contains(root.to_str().unwrap()), "{message}");
    drop(mcp);
    std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
}

#[test]
fn install_is_absent_unless_asked_for() {
    let (root, _) = store("absent");
    let packs = Arc::new(Packs::open(&[], Some(&root)).unwrap().0);
    let mcp = Mcp::new(packs, Seams::default(), Limits::SERVER).unwrap();
    for tool in ["install", "install_status"] {
        assert!(
            rmcp::ServerHandler::get_tool(&mcp, tool).is_none(),
            "{tool}"
        );
        assert!(mcp.call(tool, &Map::new(), &Bare).is_none(), "{tool}");
    }
    assert!(rmcp::ServerHandler::get_tool(&mcp, "coverage").is_some());
    // Installing needs a store: over files alone there is none to install into.
    let d = dir("absent-files");
    let file = d.join("np.tgd");
    std::fs::write(&file, pack("2026.11.0", false)).unwrap();
    let packs = Arc::new(Packs::open(std::slice::from_ref(&file), None).unwrap().0);
    let mcp = Mcp::installing(packs, Seams::default(), Limits::SERVER).unwrap();
    assert!(rmcp::ServerHandler::get_tool(&mcp, "install").is_none());
    drop(mcp);
    std::fs::remove_dir_all(d).unwrap();
    std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
}

/// The `_meta` of a 2026-07-28 client that can ask the person and run tasks.
fn asking() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {
            "elicitation": { "form": {} },
            "extensions": { "io.modelcontextprotocol/tasks": {} },
        },
        "io.modelcontextprotocol/clientInfo": { "name": "gate", "version": "0" },
    })
}

#[tokio::test]
async fn an_install_is_confirmed_by_the_person_then_runs_as_a_task() {
    let (mcp, root) = installing("task");
    let mut peer = wire::wire(mcp);
    let install = |extra: Value| {
        let mut p = json!({ "_meta": asking(), "name": "install",
                            "arguments": { "packs": ["np"], "max_bytes": 0 } });
        if let (Some(p), Value::Object(e)) = (p.as_object_mut(), extra) {
            p.extend(e);
        }
        p
    };
    // The server asks first, whatever `max_bytes` says, and seals the call in its state.
    let asked = peer.ask(1, "tools/call", install(json!({}))).await;
    let r = &asked["result"];
    assert_eq!(r["resultType"], "input_required", "{asked}");
    let question = &r["inputRequests"]["confirm"];
    assert_eq!(question["method"], "elicitation/create", "{asked}");
    let message = question["params"]["message"].as_str().unwrap();
    assert!(
        message.starts_with("Download ") && message.contains("MB to install np.places.tgd"),
        "{message}"
    );
    let state = r["requestState"].as_str().unwrap().to_owned();

    // A forged state is refused; a declined confirmation installs nothing.
    let mut forged = state.clone().into_bytes();
    let last = forged.len() - 1;
    forged[last] = if forged[last] == b'0' { b'1' } else { b'0' };
    let forged = String::from_utf8(forged).unwrap();
    let accept = json!({ "confirm": { "action": "accept", "content": { "confirm": true } } });
    let refused = peer
        .ask(
            2,
            "tools/call",
            install(json!({ "requestState": forged, "inputResponses": accept })),
        )
        .await;
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    // Anything but an accepted `confirm: true` installs nothing.
    for (n, answer) in [
        json!({ "action": "decline" }),
        json!({ "action": "decline", "content": { "confirm": true } }),
        json!({ "action": "accept", "content": { "confirm": false } }),
    ]
    .into_iter()
    .enumerate()
    {
        let declined = peer
            .ask(
                3 + n as u64 * 100,
                "tools/call",
                install(json!({ "requestState": state, "inputResponses": { "confirm": answer } })),
            )
            .await;
        assert_eq!(
            declined["result"]["structuredContent"]["result"]["status"], "declined",
            "{answer}: {declined}"
        );
    }

    // Accepted: a task, polled with `tasks/get` until it completes with the call's result.
    let started = peer
        .ask(
            4,
            "tools/call",
            install(json!({ "requestState": state, "inputResponses": accept })),
        )
        .await;
    assert_eq!(started["result"]["resultType"], "task", "{started}");
    assert_eq!(started["result"]["ttlMs"], 3_600_000, "{started}");
    let id = started["result"]["taskId"].as_str().unwrap().to_owned();
    let mut done = Value::Null;
    for n in 0..300 {
        let got = peer
            .ask(
                10 + n,
                "tasks/get",
                json!({ "_meta": asking(), "taskId": id }),
            )
            .await;
        if got["result"]["status"] != "working" {
            done = got;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(done["result"]["status"], "completed", "{done}");
    assert_eq!(
        done["result"]["result"]["structuredContent"]["result"]["installed"][0]["path"],
        "np.places.tgd",
        "{done}"
    );
    let unknown = peer
        .ask(
            400,
            "tasks/get",
            json!({ "_meta": asking(), "taskId": "nope" }),
        )
        .await;
    assert_eq!(unknown["error"]["code"], -32602, "{unknown}");
    drop(peer);
    std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
}

/// A 2025-11-25 session that declared elicitation, begun on `peer`.
async fn legacy(peer: &mut wire::Wire) {
    let init = peer
        .ask(
            1,
            "initialize",
            json!({ "protocolVersion": "2025-11-25", "capabilities": { "elicitation": {} },
                    "clientInfo": { "name": "gate", "version": "0" } }),
        )
        .await;
    assert_eq!(init["result"]["protocolVersion"], "2025-11-25", "{init}");
    peer.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;
}

fn legacy_install(id: u64) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": "install", "arguments": { "packs": ["np"], "max_bytes": 0 } } })
}

#[tokio::test]
async fn a_2025_11_25_client_on_stdio_is_asked_through_elicitation_create() {
    let (mcp, root) = installing("legacy");
    let mut peer = wire::wire(mcp.clone().duplex());
    legacy(&mut peer).await;

    // Declined, then accepted: the server asks before each, whatever `max_bytes` says.
    for (id, answer) in [
        (2, json!({ "action": "decline" })),
        (
            3,
            json!({ "action": "accept", "content": { "confirm": false } }),
        ),
        (
            4,
            json!({ "action": "accept", "content": { "confirm": true } }),
        ),
    ] {
        peer.send(legacy_install(id)).await;
        let asked = peer.next().await;
        assert_eq!(asked["method"], "elicitation/create", "{asked}");
        let message = asked["params"]["message"].as_str().unwrap();
        assert!(
            message.starts_with("Download ") && message.contains("MB to install np.places.tgd"),
            "{message}"
        );
        assert_eq!(
            asked["params"]["requestedSchema"]["required"][0], "confirm",
            "{asked}"
        );
        peer.send(json!({ "jsonrpc": "2.0", "id": asked["id"], "result": answer }))
            .await;
        let r = peer.next().await;
        assert_eq!(r["id"], id, "{r}");
        let result = &r["result"]["structuredContent"]["result"];
        if id < 4 {
            assert_eq!(result["status"], "declined", "{answer}: {r}");
            continue;
        }
        assert_eq!(result["status"], "working", "{r}");
        let handle = result["handle"].as_str().unwrap();
        assert_eq!(ended(&mcp, handle)["status"], "completed");
    }
    drop(peer);
    std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn without_a_duplex_transport_a_2025_11_25_client_is_held_to_max_bytes() {
    let (mcp, root) = installing("legacy-simplex");
    let mut peer = wire::wire(mcp);
    legacy(&mut peer).await;
    peer.send(legacy_install(2)).await;
    let r = peer.next().await;
    assert_eq!(r["id"], 2, "never asked: {r}");
    assert_eq!(r["result"]["isError"], true, "{r}");
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("out_of_bounds") && text.contains("max_bytes"),
        "{text}"
    );
    drop(peer);
    std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
}
