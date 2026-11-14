//! A raw JSON-RPC peer for the MCP tests: lines over an in-memory pipe, as a client on stdio
//! sends them, so the tests see the wire and not an SDK's view of it.

#![allow(
    dead_code,
    clippy::unwrap_used,
    reason = "test support: each test crate uses some of it; failures panic"
)]

use rmcp::ServiceExt;
use serde_json::{Value, json};
use teigeodata_server::mcp::Mcp;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// A JSON-RPC peer on the other end of a pipe: sends a line, reads the answer to its ID.
pub(crate) struct Wire {
    tx: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    rx: BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
}

impl Wire {
    pub(crate) async fn send(&mut self, msg: Value) {
        let mut line = msg.to_string();
        line.push('\n');
        self.tx.write_all(line.as_bytes()).await.unwrap();
    }

    /// The next message the server sends.
    pub(crate) async fn next(&mut self) -> Value {
        let mut line = String::new();
        assert!(self.rx.read_line(&mut line).await.unwrap() > 0, "closed");
        serde_json::from_str(&line).unwrap()
    }

    pub(crate) async fn ask(&mut self, id: u64, method: &str, params: Value) -> Value {
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

/// A peer talking to `mcp` over an in-memory pipe.
pub(crate) fn wire(mcp: Mcp) -> Wire {
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
pub(crate) fn meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": { "name": "gate", "version": "0" },
    })
}
