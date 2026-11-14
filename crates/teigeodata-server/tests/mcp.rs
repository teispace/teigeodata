//! The MCP server's gates (phase 6.3, D-315): every tool answers structured content that its
//! own `outputSchema` accepts, the same object as text and its attribution in `_meta`; every
//! failure is a tool error with its stable code; pages walk the whole answer through signed
//! cursors that cannot be forged, moved to another tool or another server; resources read; and
//! the wire speaks both 2026-07-28 (`server/discover`, no handshake) and 2025-11-25
//! (`initialize`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "tests: failures panic with their message"
)]

#[path = "../../teigeodata-engine/tests/common/mod.rs"]
mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rmcp::ServiceExt;
use rmcp::model::CallToolResult;
use serde_json::{Map, Value, json};
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer};
use teigeodata_server::mcp::{ATTRIBUTION, DEPTH, Mcp, TOOLS};
use teigeodata_server::packs::Packs;
use teigeodata_server::query::{Code, Limits, QueryError};
use teigeodata_server::seams::{Authorize, Bare, Caller, Meter, Operation, Seams, Usage};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const NOTICE: &str = "Synthetic Source, ODbL";

/// A server over a noticed synthetic pack, and the model it was built from.
fn server(seams: Seams) -> (Mcp, common::Model, std::path::PathBuf) {
    server_of(common::synthetic(300, 7), seams)
}

/// A server over `model` noticed, and the model.
fn server_of(model: common::Model, seams: Seams) -> (Mcp, common::Model, std::path::PathBuf) {
    static N: AtomicUsize = AtomicUsize::new(0);
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
        "tgd-server-mcp-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("np.tgd");
    std::fs::write(&path, bytes).unwrap();
    let (packs, _) = Packs::open(std::slice::from_ref(&path), None).unwrap();
    let mcp = Mcp::new(Arc::new(packs), seams, Limits::SERVER).unwrap();
    (mcp, model, dir)
}

fn args(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => panic!("arguments are an object"),
    }
}

fn call(mcp: &Mcp, tool: &str, a: Value) -> CallToolResult {
    mcp.call(tool, &args(a), &Bare).expect("the tool exists")
}

/// The text of a result's first content block; any after it are resource links.
fn text(r: &CallToolResult) -> &str {
    assert!(
        r.content[1..]
            .iter()
            .all(|c| c.as_resource_link().is_some()),
        "a text block, then links"
    );
    &r.content[0].as_text().expect("a text block").text
}

/// The error code of a failed call.
fn code(r: &CallToolResult) -> String {
    assert_eq!(r.is_error, Some(true), "not an error: {}", text(r));
    assert!(r.structured_content.is_none());
    let v: Value = serde_json::from_str(text(r)).unwrap();
    assert!(v["error"]["message"].is_string(), "{v}");
    v["error"]["code"].as_str().unwrap().to_owned()
}

/// Whether `v` is valid against `schema`: the subset of JSON Schema the tools use (`type`,
/// `properties`, `required`, `items`, `minimum`, `maximum`). A failure names the path.
fn valid(v: &Value, schema: &Value, at: &str) -> Result<(), String> {
    if let Some(t) = schema.get("type") {
        let types: Vec<&str> = match t {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => return Err(format!("{at}: bad type in the schema")),
        };
        let is = |t: &str| match t {
            "null" => v.is_null(),
            "string" => v.is_string(),
            "boolean" => v.is_boolean(),
            "integer" => v.is_i64() || v.is_u64(),
            "number" => v.is_number(),
            "object" => v.is_object(),
            "array" => v.is_array(),
            _ => false,
        };
        if !types.iter().any(|t| is(t)) {
            return Err(format!("{at}: {v} is not {types:?}"));
        }
    }
    if let (Some(n), Some(min)) = (v.as_f64(), schema.get("minimum").and_then(Value::as_f64))
        && n < min
    {
        return Err(format!("{at}: {n} < {min}"));
    }
    if let (Some(n), Some(max)) = (v.as_f64(), schema.get("maximum").and_then(Value::as_f64))
        && n > max
    {
        return Err(format!("{at}: {n} > {max}"));
    }
    if let Some(o) = v.as_object() {
        for r in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let r = r.as_str().unwrap();
            if !o.contains_key(r) {
                return Err(format!("{at}: {r} is required: {v}"));
            }
        }
        if let Some(props) = schema.get("properties").and_then(Value::as_object) {
            for (k, x) in o {
                if let Some(s) = props.get(k) {
                    valid(x, s, &format!("{at}/{k}"))?;
                }
            }
        }
    }
    if let (Some(a), Some(items)) = (v.as_array(), schema.get("items")) {
        for (i, x) in a.iter().enumerate() {
            valid(x, items, &format!("{at}/{i}"))?;
        }
    }
    Ok(())
}

/// Checks a finished answer: structured content its tool's schema accepts, the same JSON as
/// text, and the attribution in `_meta` when the answer has one.
fn answered(mcp: &Mcp, tool: &str, r: &CallToolResult) -> Value {
    assert_eq!(r.is_error, Some(false), "{tool}: {}", text(r));
    let v = r.structured_content.clone().expect("structured content");
    assert_eq!(
        serde_json::from_str::<Value>(text(r)).unwrap(),
        v,
        "{tool}: text"
    );
    let schema = Value::Object(
        (*rmcp::ServerHandler::get_tool(mcp, tool)
            .unwrap()
            .output_schema
            .unwrap())
        .clone(),
    );
    valid(&v, &schema, tool).unwrap_or_else(|e| panic!("{tool}: {e}"));
    let meta = r.meta.as_ref().and_then(|m| m.0.get(ATTRIBUTION));
    match v.get("attribution") {
        Some(Value::String(a)) => assert_eq!(meta, Some(&Value::from(a.as_str())), "{tool}"),
        _ if tool == "packs" => assert_eq!(meta, Some(&Value::from(NOTICE))),
        _ => assert!(meta.is_none(), "{tool}"),
    }
    v
}

#[test]
fn every_tool_answers_what_its_schema_says() {
    let (mcp, model, dir) = server(Seams::default());
    let id = model.ids[0].to_string();
    let row = (0..model.ids.len())
        .find(|&r| model.parents.get(r).copied().flatten().is_some())
        .unwrap();
    let leaf = model.ids[row].to_string();
    let parent = model.ids[model.parents[row].unwrap() as usize].to_string();
    let calls = [
        ("search", json!({ "query": "Synthland", "limit": 3 })),
        (
            "search",
            json!({ "query": "Synthland", "focus": { "lat": 27.5, "lon": 85 }, "strict": true }),
        ),
        ("reverse", json!({ "lat": 27.5, "lon": 85, "limit": 3 })),
        (
            "nearby",
            json!({ "id": id, "limit": 3, "radius_m": 500_000 }),
        ),
        ("get_place", json!({ "id": id, "names": true })),
        ("lookup", json!({ "ids": [id, id] })),
        ("admin_at", json!({ "lat": 27.5, "lon": 85 })),
        ("hierarchy", json!({ "id": leaf })),
        ("children", json!({ "id": parent, "limit": 2 })),
        ("timezone", json!({ "lat": 27.5, "lon": 85, "t": 0 })),
        ("timezone", json!({ "id": id })),
        ("elevation", json!({ "lat": 27.5, "lon": 85 })),
        ("postal", json!({ "lat": 27.5, "lon": 85 })),
        ("boundary", json!({ "id": id })),
        ("distance", json!({ "from": id, "to": "27.5,85" })),
        ("distance", json!({ "from": "27.5,85", "to": "28,86" })),
        ("packs", json!({})),
    ];
    let mut seen = std::collections::BTreeSet::new();
    for (tool, a) in calls {
        let v = answered(&mcp, tool, &call(&mcp, tool, a.clone()));
        seen.insert(tool);
        if let Some(rs) = v.get("results").and_then(Value::as_array)
            && !matches!(tool, "postal" | "timezone")
        {
            assert!(!rs.is_empty(), "{tool} {a}: {v}");
            assert_eq!(v["attribution"], NOTICE, "{tool}");
        }
    }
    assert_eq!(seen.len(), TOOLS.len(), "every tool is called");
    let v = answered(
        &mcp,
        "get_place",
        &call(&mcp, "get_place", json!({ "id": id, "names": true })),
    );
    assert_eq!(v["results"][0]["id"], id.as_str());
    assert!(
        v["results"][0]["names"]
            .as_array()
            .is_some_and(|n| !n.is_empty())
    );
    // Nothing found: an empty list, no attribution, nothing in `_meta`.
    let v = answered(
        &mcp,
        "search",
        &call(&mcp, "search", json!({ "query": "zzzzzzzz" })),
    );
    assert_eq!(v, json!({ "results": [], "attribution": null }));
    drop(mcp); // A mapped pack cannot be removed on Windows.
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn every_failure_is_a_tool_error_with_its_code() {
    let (mcp, model, dir) = server(Seams::default());
    let absent = common::synthetic(300, 99).ids[0].to_string();
    let long = "x".repeat(257);
    for (tool, a, want) in [
        ("search", json!({}), "invalid_argument"),
        ("search", json!({ "query": 7 }), "invalid_argument"),
        ("search", json!({ "query": long }), "out_of_bounds"),
        (
            "search",
            json!({ "query": "a", "limit": 51 }),
            "out_of_bounds",
        ),
        (
            "search",
            json!({ "query": "a", "limit": -1 }),
            "invalid_argument",
        ),
        (
            "search",
            json!({ "query": "a", "focus": [1, 2] }),
            "invalid_argument",
        ),
        (
            "reverse",
            json!({ "lat": 91, "lon": 0 }),
            "invalid_argument",
        ),
        (
            "reverse",
            json!({ "lat": "27", "lon": 0 }),
            "invalid_argument",
        ),
        ("nearby", json!({ "id": absent }), "not_found"),
        (
            "nearby",
            json!({ "id": model.ids[0].to_string(), "radius_m": 1e9 }),
            "out_of_bounds",
        ),
        (
            "get_place",
            json!({ "id": "not an id" }),
            "invalid_argument",
        ),
        ("lookup", json!({ "ids": "a,b" }), "invalid_argument"),
        ("lookup", json!({ "ids": [1] }), "invalid_argument"),
        ("hierarchy", json!({ "id": absent }), "not_found"),
        ("timezone", json!({ "t": 0 }), "invalid_argument"),
        (
            "search",
            json!({ "query": "a", "cursor": 5 }),
            "invalid_argument",
        ),
        (
            "search",
            json!({ "query": "a", "cursor": "00" }),
            "invalid_argument",
        ),
    ] {
        let r = call(&mcp, tool, a.clone());
        assert_eq!(code(&r), want, "{tool} {a}");
    }
    assert!(mcp.call("no_such_tool", &Map::new(), &Bare).is_none());
    drop(mcp);
    std::fs::remove_dir_all(dir).unwrap();
}

/// The places of a page, and its cursor.
fn page(mcp: &Mcp, tool: &str, a: Value) -> (Vec<String>, Option<String>) {
    let v = answered(mcp, tool, &call(mcp, tool, a));
    let ids = v["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_owned())
        .collect();
    (ids, v["next_cursor"].as_str().map(str::to_owned))
}

#[test]
fn cursors_walk_the_answer_and_cannot_be_forged_or_moved() {
    let (mcp, _, dir) = server(Seams::default());
    // One call as deep as pages reach, unbounded, is the truth the pages must add up to.
    let (whole, _) = {
        let (mcp_deep, _, d) = server(Seams::default());
        let deep = Mcp::new(
            Arc::new(Packs::open(&[d.join("np.tgd")], None).unwrap().0),
            Seams::default(),
            Limits::NONE,
        )
        .unwrap();
        let r = page(
            &deep,
            "reverse",
            json!({ "lat": 27.5, "lon": 85, "limit": DEPTH }),
        );
        drop((deep, mcp_deep));
        std::fs::remove_dir_all(d).unwrap();
        r
    };
    assert_eq!(
        whole.len(),
        DEPTH,
        "the model has more places than pages reach"
    );

    let (mut got, mut next) = page(
        &mcp,
        "reverse",
        json!({ "lat": 27.5, "lon": 85, "limit": 50 }),
    );
    let mut pages = 1;
    while let Some(c) = next {
        // The cursor alone carries the call: other arguments are not needed.
        let (ids, n) = page(&mcp, "reverse", json!({ "cursor": c }));
        assert_eq!(ids.len(), 50);
        got.extend(ids);
        next = n;
        pages += 1;
    }
    assert_eq!(pages, DEPTH / 50, "the last page has no cursor");
    assert_eq!(got, whole, "the pages add up to the whole answer, in order");

    // A short page has no cursor; nor does an answer that is not paged.
    let (_, c) = page(&mcp, "search", json!({ "query": "zzzzzzzz" }));
    assert!(c.is_none());

    let (_, c) = page(
        &mcp,
        "reverse",
        json!({ "lat": 27.5, "lon": 85, "limit": 3 }),
    );
    let c = c.unwrap();
    let refused = |r: &CallToolResult| {
        assert_eq!(code(r), "invalid_argument");
        let v: Value = serde_json::from_str(text(r)).unwrap();
        assert_eq!(
            v["error"]["message"],
            "the cursor is not one this server gave for this tool: ask again without it"
        );
    };
    // Any byte changed is refused, so is the cursor on another tool or another server.
    for i in [0, c.len() / 2, c.len() - 1] {
        let mut b = c.clone().into_bytes();
        b[i] = if b[i] == b'0' { b'1' } else { b'0' };
        let forged = String::from_utf8(b).unwrap();
        refused(&call(&mcp, "reverse", json!({ "cursor": forged })));
    }
    // `admin_at` would take the cursor's arguments (lat, lon): the refusal must be the
    // cursor's, not a missing argument's.
    refused(&call(&mcp, "admin_at", json!({ "cursor": c })));
    refused(&call(&mcp, "search", json!({ "query": "a", "cursor": c })));
    let (other, _, d2) = server(Seams::default());
    refused(&call(&other, "reverse", json!({ "cursor": c })));
    drop((mcp, other));
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(d2).unwrap();
}

struct Gate(Arc<AtomicUsize>);

impl Authorize for Gate {
    fn authorize(&self, _: &Caller, op: Operation) -> Result<(), QueryError> {
        if op == Operation::Search {
            return Err(QueryError::new(Code::RateLimited, "slow down"));
        }
        Ok(())
    }
}

impl Meter for Gate {
    fn record(&self, _: &Caller, _: Operation, usage: Usage) {
        self.0.fetch_add(usage.results, Ordering::Relaxed);
    }
}

#[test]
fn the_seams_gate_and_meter_every_call() {
    let counted = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Gate(Arc::clone(&counted)));
    let seams = Seams {
        authorize: gate.clone(),
        meter: gate,
        ..Seams::default()
    };
    let (mcp, _, dir) = server(seams);
    assert_eq!(
        code(&call(&mcp, "search", json!({ "query": "Synthland" }))),
        "rate_limited"
    );
    assert_eq!(
        counted.load(Ordering::Relaxed),
        0,
        "a refusal is not metered"
    );
    let (ids, _) = page(
        &mcp,
        "reverse",
        json!({ "lat": 27.5, "lon": 85, "limit": 4 }),
    );
    assert_eq!(ids.len(), 4);
    assert_eq!(counted.load(Ordering::Relaxed), 4);
    // The tools answered outside the paged path are metered too.
    let id = ids[0].clone();
    for (tool, a) in [
        ("boundary", json!({ "id": id })),
        ("distance", json!({ "from": id, "to": "0,0" })),
    ] {
        assert_eq!(call(&mcp, tool, a).is_error, Some(false), "{tool}");
    }
    assert_eq!(counted.load(Ordering::Relaxed), 6);
    drop(mcp);
    std::fs::remove_dir_all(dir).unwrap();
}

/// A JSON-RPC peer on the other end of a pipe: sends a line, reads the answer to its ID.
struct Wire {
    tx: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    rx: BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
}

impl Wire {
    async fn send(&mut self, msg: Value) {
        let mut line = msg.to_string();
        line.push('\n');
        self.tx.write_all(line.as_bytes()).await.unwrap();
    }

    async fn ask(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await;
        loop {
            let mut line = String::new();
            assert!(self.rx.read_line(&mut line).await.unwrap() > 0, "closed");
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == id {
                return v;
            }
        }
    }
}

fn wire(mcp: Mcp) -> Wire {
    let (ours, theirs) = tokio::io::duplex(1 << 20);
    tokio::spawn(async move {
        if let Ok(running) = mcp.serve(theirs).await {
            let _ = running.waiting().await;
        }
    });
    let (rx, tx) = tokio::io::split(ours);
    Wire {
        tx,
        rx: BufReader::new(rx),
    }
}

/// The `_meta` a 2026-07-28 request carries.
fn meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": { "name": "gate", "version": "0" },
    })
}

#[tokio::test]
async fn the_wire_speaks_2026_07_28_without_a_handshake() {
    let (mcp, model, dir) = server(Seams::default());
    let mut peer = wire(mcp);
    let found = peer
        .ask(1, "server/discover", json!({ "_meta": meta() }))
        .await;
    let info = &found["result"];
    assert!(
        info["supportedVersions"].as_array().is_some_and(|v| {
            v.contains(&json!("2026-07-28")) && v.contains(&json!("2025-11-25"))
        }),
        "{found}"
    );
    // 2026-07-28 carries the server's identity in `_meta`.
    assert_eq!(
        info["_meta"]["io.modelcontextprotocol/serverInfo"]["name"], "teigeodata",
        "{found}"
    );
    assert!(info["capabilities"]["tools"].is_object(), "{found}");

    let listed = peer.ask(2, "tools/list", json!({ "_meta": meta() })).await;
    let tools = listed["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, TOOLS, "every tool, in a stable order");
    for tool in tools {
        assert_eq!(tool["annotations"]["readOnlyHint"], true, "{tool}");
        assert_eq!(tool["outputSchema"]["type"], "object", "{tool}");
        assert_eq!(tool["inputSchema"]["type"], "object", "{tool}");
        let name = tool["name"].as_str().unwrap();
        // Hosts that pass tools on to a model accept these characters only.
        assert!(
            name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
            "{name}"
        );
    }
    assert_eq!(listed["result"]["resultType"], "complete", "{listed}");
    assert!(listed["result"]["ttlMs"].as_u64().is_some(), "{listed}");
    assert!(listed["result"]["cacheScope"].is_string(), "{listed}");

    let id = model.ids[0].to_string();
    let got = peer
        .ask(
            3,
            "tools/call",
            json!({ "_meta": meta(), "name": "get_place", "arguments": { "id": id } }),
        )
        .await;
    let res = &got["result"];
    assert_eq!(res["isError"], false, "{got}");
    assert_eq!(
        res["structuredContent"]["results"][0]["id"],
        id.as_str(),
        "{got}"
    );
    assert_eq!(res["_meta"][ATTRIBUTION], NOTICE, "{got}");

    let unknown = peer
        .ask(
            4,
            "tools/call",
            json!({ "_meta": meta(), "name": "nope", "arguments": {} }),
        )
        .await;
    assert_eq!(unknown["error"]["code"], -32602, "{unknown}");
    drop(peer);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn the_resources_are_the_packs_and_the_places() {
    let (mcp, model, dir) = server(Seams::default());
    let mut peer = wire(mcp);
    let id = model.ids[0].to_string();
    // The pack, a place by ID (escaped or not), a place not there.
    let datasets = peer
        .ask(5, "resources/list", json!({ "_meta": meta() }))
        .await;
    assert_eq!(
        datasets["result"]["resources"][0]["uri"], "teigeodata://dataset/np/places",
        "{datasets}"
    );
    let templates = peer
        .ask(6, "resources/templates/list", json!({ "_meta": meta() }))
        .await;
    assert_eq!(
        templates["result"]["resourceTemplates"]
            .as_array()
            .map(Vec::len),
        Some(3),
        "{templates}"
    );
    let dataset = peer
        .ask(
            7,
            "resources/read",
            json!({ "_meta": meta(), "uri": "teigeodata://dataset/np/places" }),
        )
        .await;
    let body: Value =
        serde_json::from_str(dataset["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["attribution"], NOTICE, "{dataset}");
    for uri in [
        format!("teigeodata://place/{id}"),
        format!(
            "teigeodata://place/{}",
            id.replace(':', "%3A").replace('/', "%2F")
        ),
    ] {
        let read = peer
            .ask(8, "resources/read", json!({ "_meta": meta(), "uri": uri }))
            .await;
        let contents = &read["result"]["contents"][0];
        assert_eq!(contents["mimeType"], "application/geo+json", "{read}");
        let feature: Value = serde_json::from_str(contents["text"].as_str().unwrap()).unwrap();
        assert_eq!(
            (&feature["type"], &feature["id"], &feature["attribution"]),
            (&json!("Feature"), &json!(id), &json!(NOTICE)),
            "{read}"
        );
        assert!(feature["properties"]["names"].is_array(), "{read}");
    }
    let absent = common::synthetic(300, 99).ids[0].to_string();
    let missing = peer
        .ask(
            9,
            "resources/read",
            json!({ "_meta": meta(), "uri": format!("teigeodata://place/{absent}") }),
        )
        .await;
    assert_eq!(missing["error"]["code"], -32602, "{missing}");
    drop(peer);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn the_wire_serves_2025_11_25_clients_through_initialize() {
    let (mcp, _, dir) = server(Seams::default());
    let mut peer = wire(mcp);
    let init = peer
        .ask(
            1,
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "gate", "version": "0" },
            }),
        )
        .await;
    assert_eq!(init["result"]["protocolVersion"], "2025-11-25", "{init}");
    assert!(init["result"]["instructions"].is_string(), "{init}");
    peer.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;
    let listed = peer.ask(2, "tools/list", json!({})).await;
    assert_eq!(
        listed["result"]["tools"].as_array().map(Vec::len),
        Some(TOOLS.len()),
        "{listed}"
    );
    let got = peer
        .ask(
            3,
            "tools/call",
            json!({ "name": "reverse", "arguments": { "lat": 27.5, "lon": 85, "limit": 2 } }),
        )
        .await;
    let content = &got["result"]["structuredContent"];
    assert_eq!(
        content["results"].as_array().map(Vec::len),
        Some(2),
        "{got}"
    );
    assert!(content["next_cursor"].is_string(), "{got}");
    let off = peer
        .ask(
            4,
            "tools/call",
            json!({ "name": "reverse", "arguments": { "lat": 95, "lon": 85 } }),
        )
        .await;
    assert_eq!(off["result"]["isError"], true, "{off}");
    drop(peer);
    std::fs::remove_dir_all(dir).unwrap();
}

/// A server over the synthetic model with outlines, and its country's ID.
fn outlined_server() -> (Mcp, common::Model, std::path::PathBuf, String) {
    let (model, _) = common::outlined(false);
    let row = model
        .kinds
        .iter()
        .position(|&k| k == teigeodata_core::PlaceKind::Country)
        .unwrap();
    let (mcp, model, dir) = server_of(model, Seams::default());
    let id = model.ids[row].to_string();
    (mcp, model, dir, id)
}

#[test]
fn the_boundary_tool_summarises_its_outline_and_links_it() {
    let (mcp, model, dir, id) = outlined_server();
    let absent = common::synthetic(300, 99).ids[0].to_string();
    // The summary: what the outline is, not its vertices, and a link to it.
    let result = call(&mcp, "boundary", json!({ "id": id }));
    let summary = answered(&mcp, "boundary", &result);
    let uri = format!("teigeodata://boundary/{id}?lod=0");
    assert_eq!(
        summary["result"],
        json!({
            "id": id, "name": summary["result"]["name"], "outlined": true, "lod": 0,
            "tolerance_m": 0.0, "vertices": 12, "parts": 1, "holes": 1,
            "bbox": summary["result"]["bbox"], "uri": uri,
        }),
        "{summary}"
    );
    assert_eq!(summary["attribution"], NOTICE);
    let bbox: Vec<f64> = serde_json::from_value(summary["result"]["bbox"].clone()).unwrap();
    let west = f64::from(common::WEST) / 1e7;
    let east = f64::from(common::EAST) / 1e7;
    assert!(
        (bbox[0] - west).abs() < 1e-6 && (bbox[2] - east).abs() < 1e-6 && bbox[1] < bbox[3],
        "{bbox:?}"
    );
    let link = result.content[1]
        .as_resource_link()
        .expect("a resource link");
    assert_eq!(
        (link.uri.as_str(), link.mime_type.as_deref()),
        (uri.as_str(), Some("application/geo+json"))
    );
    let coarse = answered(
        &mcp,
        "boundary",
        &call(&mcp, "boundary", json!({ "id": id, "lod": 3 })),
    );
    assert_eq!(
        (&coarse["result"]["lod"], &coarse["result"]["vertices"]),
        (&json!(3), &json!(10))
    );
    // A district: one ring, no hole.
    let district = (0..model.ids.len())
        .find(|&r| model.kinds[r] == teigeodata_core::PlaceKind::Admin && model.levels[r] == 1)
        .unwrap();
    let one = answered(
        &mcp,
        "boundary",
        &call(
            &mcp,
            "boundary",
            json!({ "id": model.ids[district].to_string() }),
        ),
    );
    assert_eq!(
        (
            &one["result"]["vertices"],
            &one["result"]["parts"],
            &one["result"]["holes"]
        ),
        (&json!(6), &json!(1), &json!(0)),
        "{one}"
    );
    for (a, want) in [
        (json!({ "id": id, "lod": 7 }), "invalid_argument"),
        (json!({}), "invalid_argument"),
        (json!({ "id": absent }), "not_found"),
    ] {
        assert_eq!(code(&call(&mcp, "boundary", a.clone())), want, "{a}");
    }
    // A distance's failures.
    for (a, want) in [
        (json!({ "from": id }), "invalid_argument"),
        (json!({ "from": id, "to": "91,0" }), "invalid_argument"),
        (json!({ "from": absent, "to": id }), "not_found"),
    ] {
        assert_eq!(code(&call(&mcp, "distance", a.clone())), want, "{a}");
    }

    drop(mcp);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn the_boundary_resource_reads_the_outline_at_its_level() {
    let (mcp, _, dir, id) = outlined_server();
    let uri = format!("teigeodata://boundary/{id}?lod=0");
    // The resource: the outline as GeoJSON, at the level linked, and refused past 6.
    let mut peer = wire(mcp);
    let read = peer
        .ask(2, "resources/read", json!({ "_meta": meta(), "uri": uri }))
        .await;
    let contents = &read["result"]["contents"][0];
    assert_eq!(contents["mimeType"], "application/geo+json", "{read}");
    let feature: Value = serde_json::from_str(contents["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        (
            &feature["type"],
            &feature["id"],
            &feature["geometry"]["type"]
        ),
        (&json!("Feature"), &json!(id), &json!("Polygon")),
        "{feature}"
    );
    assert_eq!(feature["properties"]["vertices"], 12, "{feature}");
    assert_eq!(feature["attribution"], NOTICE, "{feature}");
    let escaped = format!(
        "teigeodata://boundary/{}?lod=2",
        id.replace(':', "%3A").replace('/', "%2F")
    );
    let read = peer
        .ask(
            3,
            "resources/read",
            json!({ "_meta": meta(), "uri": escaped }),
        )
        .await;
    let feature: Value =
        serde_json::from_str(read["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(feature["properties"]["lod"], 2, "{feature}");
    for bad in ["?lod=7", "?lod=x", "?zoom=1"] {
        let read = peer
            .ask(
                4,
                "resources/read",
                json!({ "_meta": meta(), "uri": format!("teigeodata://boundary/{id}{bad}") }),
            )
            .await;
        assert_eq!(read["error"]["code"], -32602, "{bad}: {read}");
    }
    drop(peer);
    std::fs::remove_dir_all(dir).unwrap();
}
