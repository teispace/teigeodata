//! [`mcp`]: the MCP server's tools and resources on fuzzed arguments (D-331).
//!
//! The server runs over the bindings' synthetic fixture, written once to a temporary folder.
//! Each input calls one tool (`Mcp::call`, what `tools/call` does without a transport) with an
//! argument object built from the tool's own input schema. Each property listed there gets a
//! value of any JSON type:
//! - numbers past every bound, negative or fractional;
//! - strings of the fuzz bytes, or real IDs, zones and codes;
//! - arrays, objects and null.
//!
//! Unknown keys are added too. Or the input reads a resource URI made from the fuzz bytes. The
//! property: no call fails with `internal`. A refusal carries a client code, as the HTTP
//! server's does. A tool the server adds is fuzzed from its definition, with no change here.

#![allow(
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test harness: a panic is how a property failure is reported, and indexes are \
              taken modulo the length"
)]

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use serde_json::{Map, Value};

use crate::mcp::Mcp;
use crate::packs::Packs;
use crate::query::{Code, Limits};
use crate::seams::{Bare, Seams};

/// Real values of the fixture, so calls reach the queries.
const KNOWN: [&str; 7] = [
    "tgd:np/1rxre8",
    "tgd:np/1",
    "Asia/Kathmandu",
    "NP",
    "44600",
    "Pokhara",
    "teigeodata://place/tgd:np/1rxre8",
];

struct Cursor<'a>(&'a [u8]);

impl Cursor<'_> {
    fn u8(&mut self) -> u8 {
        let (v, rest) = self.0.split_first().map_or((0, &[][..]), |(v, r)| (*v, r));
        self.0 = rest;
        v
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes([0; 4].map(|_| self.u8()))
    }
    fn text(&mut self) -> String {
        let n = usize::from(self.u8() % 40);
        let (v, rest) = self.0.split_at(n.min(self.0.len()));
        self.0 = rest;
        String::from_utf8_lossy(v).into_owned()
    }
}

/// The server, once per process: the fixture is written and opened on first use.
fn server() -> Option<&'static Mcp> {
    static SERVER: OnceLock<Option<Mcp>> = OnceLock::new();
    SERVER
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("tgd-fuzz-mcp-{}", std::process::id()));
            std::fs::create_dir_all(&dir).ok()?;
            let path: PathBuf = dir.join("synth.tgd");
            std::fs::write(&path, SEED).ok()?;
            let (packs, _) = Packs::open(std::slice::from_ref(&path), None).ok()?;
            Mcp::new(Arc::new(packs), Seams::default(), Limits::SERVER).ok()
        })
        .as_ref()
}

/// The bindings' synthetic fixture (13 kinds, 300 places).
const SEED: &[u8] = include_bytes!("../../../../bindings/fixtures/synth.tgd");

/// A JSON value of any type from the fuzz bytes.
fn value(c: &mut Cursor<'_>, depth: u8) -> Value {
    match c.u8() % 12 {
        0 => Value::Null,
        1 => Value::Bool(c.u8() & 1 == 1),
        2 => Value::from(c.u32()),
        3 => Value::from(-i64::from(c.u32())),
        4 => Value::from(f64::from(c.u32()) / 1e4 - 200_000.0),
        5 => Value::from(u64::MAX),
        6 | 7 => Value::String(KNOWN[usize::from(c.u8()) % KNOWN.len()].to_string()),
        8 => Value::String(c.text()),
        9 if depth < 3 => Value::Array((0..c.u8() % 4).map(|_| value(c, depth + 1)).collect()),
        10 if depth < 3 => {
            let mut m = Map::new();
            for k in ["lat", "lon", "scale_km"] {
                if c.u8() & 1 == 1 {
                    m.insert(k.to_string(), value(c, depth + 1));
                }
            }
            Value::Object(m)
        }
        _ => Value::from(f64::from(c.u8()) / 8.0 + 26.0),
    }
}

/// One tool call or resource read on the server, from `data`.
///
/// # Panics
/// When a call fails with `internal`: that is the harness's report.
pub fn mcp(data: &[u8]) {
    let Some(server) = server() else {
        panic!("the MCP fuzz server could not start")
    };
    let mut c = Cursor(data);
    // The tools a server without a store lists (the store's and the installer's need one).
    let tools: Vec<_> = crate::mcp::tool_definitions(&Limits::SERVER)
        .into_iter()
        .filter(|t| {
            let n: &str = &t.name;
            !crate::mcp::STORE_TOOLS.contains(&n) && !crate::mcp::INSTALL_TOOLS.contains(&n)
        })
        .collect();
    if c.u8() % 8 == 0 {
        let prefix = [
            "teigeodata://dataset/",
            "teigeodata://place/",
            "teigeodata://boundary/",
            "",
        ][usize::from(c.u8()) % 4];
        let uri = format!("{prefix}{}", c.text());
        if let Err(e) = server.read_for_fuzz(&uri, &Bare) {
            assert!(e.code != Code::Internal, "read {uri}: internal: {e}");
        }
        return;
    }
    let Some(tool) = tools.get(usize::from(c.u8()) % tools.len().max(1)) else {
        return;
    };
    let keys: Vec<String> = tool
        .input_schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default();
    let mut args = Map::new();
    for k in &keys {
        if c.u8() % 4 != 0 {
            args.insert(k.clone(), value(&mut c, 0));
        }
    }
    if c.u8() % 8 == 0 {
        args.insert(c.text(), value(&mut c, 0));
    }
    let Some(r) = server.call(&tool.name, &args, &Bare) else {
        panic!("a listed tool is unknown: {}", tool.name)
    };
    if r.is_error == Some(true) {
        let text = r
            .content
            .first()
            .and_then(|b| b.as_text())
            .map(|t| t.text.clone())
            .unwrap_or_default();
        assert!(
            !text.contains("\"code\":\"internal\""),
            "{}({}): {text}",
            tool.name,
            Value::Object(args)
        );
    }
}
