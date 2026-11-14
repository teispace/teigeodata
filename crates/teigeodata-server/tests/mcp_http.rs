//! MCP over the HTTP server (phase 6.3, D-316): `/mcp` answers stateless Streamable HTTP for
//! 2026-07-28 and 2025-11-25 clients with JSON, refuses a `Host` off loopback (DNS rebinding)
//! and a browser `Origin` not configured, passes the request's headers to the seams, and is
//! absent unless configured.

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
use axum::http::{HeaderValue, Request, StatusCode};
use serde_json::{Value, json};
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer};
use teigeodata_server::http::{App, Config, router};
use teigeodata_server::mcp::{ATTRIBUTION, Mcp};
use teigeodata_server::packs::Packs;
use teigeodata_server::query::{Code, Limits, QueryError};
use teigeodata_server::seams::{Caller, Identify, Incoming, Seams};
use tower::ServiceExt;

const NOTICE: &str = "Synthetic Source, ODbL";

/// An app over a noticed synthetic pack, with MCP at `/mcp` when `mcp`, and its model.
fn served(seams: Seams, mcp: bool, cors: &[&str]) -> (Arc<App>, common::Model, std::path::PathBuf) {
    static N: AtomicUsize = AtomicUsize::new(0);
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
        "tgd-server-mcp-http-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("np.tgd");
    std::fs::write(&path, bytes).unwrap();
    let packs = Arc::new(Packs::open(std::slice::from_ref(&path), None).unwrap().0);
    let server = Mcp::new(Arc::clone(&packs), seams.clone(), Limits::SERVER).unwrap();
    let app = Arc::new(App {
        packs,
        seams,
        config: Config {
            mcp: mcp.then_some(server),
            cors_origins: cors
                .iter()
                .map(|c| HeaderValue::from_str(c).unwrap())
                .collect(),
            mcp_port: Some(8080),
            ..Config::default()
        },
    });
    (app, model, dir)
}

/// A JSON-RPC request to `/mcp` with `headers`, and its status and body.
async fn post(app: &Arc<App>, body: &Value, headers: &[(&str, &str)]) -> (StatusCode, Value) {
    let mut req = Request::post("/mcp")
        .header("host", "127.0.0.1:8080")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut req = req.body(Body::from(body.to_string())).unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 9))));
    let res = router(Arc::clone(app)).oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": { "name": "gate", "version": "0" },
    })
}

/// A 2026-07-28 `tools/call` and the headers that must mirror it.
fn tool_call(name: &str, arguments: &Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "_meta": meta(), "name": name, "arguments": arguments } })
}

const V2026: [(&str, &str); 1] = [("mcp-protocol-version", "2026-07-28")];

fn headers_for(name: &str) -> Vec<(&str, &str)> {
    let mut h = V2026.to_vec();
    h.extend([("mcp-method", "tools/call"), ("mcp-name", name)]);
    h
}

#[tokio::test]
async fn mcp_answers_over_http_for_both_revisions() {
    let (app, model, dir) = served(Seams::default(), true, &[]);
    let id = model.ids[0].to_string();

    let discover = json!({ "jsonrpc": "2.0", "id": 1, "method": "server/discover",
                           "params": { "_meta": meta() } });
    let mut h = V2026.to_vec();
    h.push(("mcp-method", "server/discover"));
    let (s, v) = post(&app, &discover, &h).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["result"]["supportedVersions"].is_array(), "{v}");

    let (s, v) = post(
        &app,
        &tool_call("get_place", &json!({ "id": id })),
        &headers_for("get_place"),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        v["result"]["structuredContent"]["results"][0]["id"],
        id.as_str(),
        "{v}"
    );
    assert_eq!(v["result"]["_meta"][ATTRIBUTION], NOTICE, "{v}");

    // A 2025-11-25 client: `initialize`, then a call in its own request (no session kept).
    let init = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-11-25", "capabilities": {},
        "clientInfo": { "name": "gate", "version": "0" } } });
    let (s, v) = post(&app, &init, &[]).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["result"]["protocolVersion"], "2025-11-25", "{v}");
    let call = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "reverse", "arguments": { "lat": 27.5, "lon": 85, "limit": 2 } } });
    let (s, v) = post(&app, &call, &[("mcp-protocol-version", "2025-11-25")]).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        v["result"]["structuredContent"]["results"]
            .as_array()
            .map(Vec::len),
        Some(2),
        "{v}"
    );
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn mcp_refuses_a_foreign_host_and_origin() {
    let (app, _, dir) = served(Seams::default(), true, &[]);
    let call = tool_call("packs", &json!({}));
    let h = headers_for("packs");

    // DNS rebinding: a page on another name resolving to 127.0.0.1.
    let mut req = Request::post("/mcp")
        .header("host", "evil.example")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    for (k, v) in &h {
        req = req.header(*k, *v);
    }
    let req = req.body(Body::from(call.to_string())).unwrap();
    let res = router(Arc::clone(&app)).oneshot(req).await.unwrap();
    assert!(res.status().is_client_error(), "{}", res.status());

    // A browser page's origin, none configured.
    let mut with_origin = h.clone();
    with_origin.push(("origin", "https://evil.example"));
    let (s, _) = post(&app, &call, &with_origin).await;
    assert!(s.is_client_error(), "{s}");
    // A native client sends none: answered.
    let (s, v) = post(&app, &call, &h).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // The server's own origin (a page it serves) is answered under every loopback name; a
    // loopback page on another port is another site and is refused.
    for own in [
        "http://127.0.0.1:8080",
        "http://localhost:8080",
        "http://[::1]:8080",
    ] {
        let mut o = h.clone();
        o.push(("origin", own));
        let (s, v) = post(&app, &call, &o).await;
        assert_eq!(s, StatusCode::OK, "{own}: {v}");
    }
    for other in [
        "http://127.0.0.1:3000",
        "https://127.0.0.1:8080",
        "http://localhost",
    ] {
        let mut o = h.clone();
        o.push(("origin", other));
        let (s, _) = post(&app, &call, &o).await;
        assert!(s.is_client_error(), "{other}: {s}");
    }
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();

    // A configured origin is answered, on its scheme's port only.
    let (app, _, dir) = app_with_cors();
    let mut ok = h.clone();
    ok.push(("origin", "https://maps.example"));
    let (s, v) = post(&app, &call, &ok).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let mut other_port = h.clone();
    other_port.push(("origin", "https://maps.example:8443"));
    let (s, _) = post(&app, &call, &other_port).await;
    assert!(s.is_client_error(), "{s}");
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}

fn app_with_cors() -> (Arc<App>, common::Model, std::path::PathBuf) {
    served(Seams::default(), true, &["https://maps.example"])
}

/// Admits a caller with the key `k1` only.
struct Key;

impl Identify for Key {
    fn identify(&self, request: &dyn Incoming) -> Result<Caller, QueryError> {
        match request.header("x-api-key") {
            Some(b"k1") => Ok(Caller {
                id: Some("k1".into()),
                tenant: None,
            }),
            _ => Err(QueryError::new(Code::Unauthenticated, "no key")),
        }
    }
}

#[tokio::test]
async fn the_seams_read_the_requests_headers_and_mcp_is_absent_unless_configured() {
    let seams = Seams {
        identify: Arc::new(Key),
        ..Seams::default()
    };
    let (app, _, dir) = served(seams, true, &[]);
    let call = tool_call("packs", &json!({}));
    let h = headers_for("packs");
    let (s, v) = post(&app, &call, &h).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["result"]["isError"], true, "{v}");
    assert!(
        v["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("unauthenticated"),
        "{v}"
    );
    let mut keyed = h.clone();
    keyed.push(("x-api-key", "k1"));
    let (s, v) = post(&app, &call, &keyed).await;
    assert_eq!(
        (s, &v["result"]["isError"]),
        (StatusCode::OK, &json!(false)),
        "{v}"
    );
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();

    let (app, _, dir) = served(Seams::default(), false, &[]);
    let (s, _) = post(&app, &call, &h).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}
