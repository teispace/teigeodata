//! The server's resource bounds (phase 8.2, D-334, [server limits note]): one test per bound,
//! each proved red with its bound removed.
//!
//! - requests in flight past `max_in_flight` are refused at once (503, `Retry-After`);
//! - a `/mcp` body past 64 KiB is refused (413);
//! - a body that never ends is answered at the request timeout (503, the JSON `unavailable` error);
//! - a request head sent slower than `header_timeout` loses its connection, and so does an idle
//!   keep-alive connection;
//! - a connection past `max_connections` is not served until one closes;
//! - shutdown returns once the connections have finished.
//!
//! The first two run in process on the router; the others over real sockets, through
//! `http::serve`, where those bounds live.
//!
//! [server limits note]: ../../../docs/02-research/notes/server-limits.md

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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer};
use teigeodata_server::http::{App, Config, router, serve};
use teigeodata_server::mcp::Mcp;
use teigeodata_server::packs::Packs;
use teigeodata_server::query::{Limits, QueryError};
use teigeodata_server::seams::{Authorize, Caller, Operation, Seams};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tower::ServiceExt;

/// An app over a synthetic pack with `config`, MCP at `/mcp`, and its directory.
fn app(seams: Seams, config: Config) -> (Arc<App>, std::path::PathBuf) {
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
    w.manifest(Tag::ATTRIBUTION, b"Synthetic Source, ODbL")
        .unwrap();
    let bytes = w.pack(&sections, None).unwrap();
    let dir = std::env::temp_dir().join(format!(
        "tgd-server-limits-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("np.tgd");
    std::fs::write(&path, bytes).unwrap();
    let packs = Arc::new(Packs::open(std::slice::from_ref(&path), None).unwrap().0);
    let mcp = Mcp::new(Arc::clone(&packs), seams.clone(), Limits::SERVER).unwrap();
    let app = Arc::new(App {
        packs,
        seams,
        config: Config {
            mcp: Some(mcp),
            mcp_hosts: Vec::new(),
            ..config
        },
    });
    (app, dir)
}

fn get(uri: &str) -> Request<Body> {
    let mut req = Request::get(uri).body(Body::empty()).unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 9))));
    req
}

/// Holds every search in `authorize` until the test lets it go.
struct Hold {
    entered: Mutex<mpsc::Sender<()>>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl Authorize for Hold {
    fn authorize(&self, _: &Caller, op: Operation) -> Result<(), QueryError> {
        if op == Operation::Search {
            self.entered.lock().unwrap().send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
        }
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_past_max_in_flight_is_refused_at_once() {
    let (entered_tx, entered) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let hold = Hold {
        entered: Mutex::new(entered_tx),
        release: Mutex::new(release_rx),
    };
    let seams = Seams {
        authorize: Arc::new(hold),
        ..Seams::default()
    };
    let config = Config {
        max_in_flight: 1,
        ..Config::default()
    };
    let (app, dir) = app(seams, config);
    // One router, as `serve` builds it: every request shares its limit.
    let r = router(app);
    let first = tokio::spawn(r.clone().oneshot(get("/v1/search?q=a")));
    tokio::task::spawn_blocking(move || entered.recv().unwrap())
        .await
        .unwrap();

    let start = Instant::now();
    let refused = r.clone().oneshot(get("/health")).await.unwrap();
    assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "refused at once, not queued"
    );
    assert_eq!(refused.headers()["retry-after"], "1");
    let body = axum::body::to_bytes(refused.into_body(), 1 << 16)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["code"], "unavailable", "{json}");

    release.send(()).unwrap();
    assert_eq!(first.await.unwrap().unwrap().status(), StatusCode::OK);
    // The slot is free again.
    let again = r.oneshot(get("/health")).await.unwrap();
    assert_eq!(again.status(), StatusCode::OK);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn an_mcp_body_past_64_kib_is_refused() {
    let (app, dir) = app(Seams::default(), Config::default());
    let r = router(app);
    let post = |len: usize| {
        let body = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{"pad":"{}"}}}}"#,
            "x".repeat(len)
        );
        let mut req = Request::post("/mcp")
            .header("host", "127.0.0.1:8080")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-11-25")
            .body(Body::from(body))
            .unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 9))));
        req
    };
    let under = r.clone().oneshot(post(60 * 1024)).await.unwrap();
    assert_ne!(
        under.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "60 KiB is allowed"
    );
    let over = r.oneshot(post(64 * 1024)).await.unwrap();
    assert_eq!(over.status(), StatusCode::PAYLOAD_TOO_LARGE);
    std::fs::remove_dir_all(dir).unwrap();
}

/// A server over `config` on a free loopback port, its address, and a shutdown handle.
struct Running {
    addr: SocketAddr,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
    dir: std::path::PathBuf,
}

async fn start(config: Config) -> Running {
    let (app, dir) = app(Seams::default(), config);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(serve(listener, app, None, async move {
        let _ = stopped.await;
    }));
    Running {
        addr,
        stop: Some(stop),
        task,
        dir,
    }
}

impl Running {
    /// Shuts the server down and checks `serve` returns promptly.
    async fn stop(mut self) {
        self.stop.take().unwrap().send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(15), self.task)
            .await
            .expect("serve returns after shutdown")
            .unwrap()
            .unwrap();
        std::fs::remove_dir_all(&self.dir).unwrap();
    }
}

/// Reads until the peer closes, for at most `within`; the bytes and when it closed.
async fn read_to_close(s: &mut TcpStream, within: Duration) -> (Vec<u8>, Option<Instant>) {
    let mut out = Vec::new();
    let mut buf = [0_u8; 4096];
    let deadline = Instant::now() + within;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, s.read(&mut buf)).await {
            Ok(Ok(0) | Err(_)) => return (out, Some(Instant::now())),
            Ok(Ok(n)) => out.extend_from_slice(&buf[..n]),
            Err(_) => return (out, None),
        }
    }
}

const HEADER_TIMEOUT: Duration = Duration::from_millis(400);
/// Timers may fire this much early on a loaded machine.
const SLACK: Duration = Duration::from_millis(50);

#[tokio::test]
async fn a_slow_head_and_an_idle_connection_are_closed() {
    let server = start(Config {
        header_timeout: HEADER_TIMEOUT,
        ..Config::default()
    })
    .await;

    // A head that never ends.
    let mut slow = TcpStream::connect(server.addr).await.unwrap();
    let start = Instant::now();
    slow.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n")
        .await
        .unwrap();
    let (_, closed) = read_to_close(&mut slow, Duration::from_secs(5)).await;
    let after = closed.expect("a slow head is cut") - start;
    assert!(after >= HEADER_TIMEOUT.saturating_sub(SLACK), "{after:?}");

    // An answered request, then nothing: the idle connection is closed too.
    let mut idle = TcpStream::connect(server.addr).await.unwrap();
    idle.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let start = Instant::now();
    let (bytes, closed) = read_to_close(&mut idle, Duration::from_secs(5)).await;
    assert!(
        bytes.starts_with(b"HTTP/1.1 200"),
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let after = closed.expect("an idle connection is closed") - start;
    assert!(after >= HEADER_TIMEOUT.saturating_sub(SLACK), "{after:?}");
    server.stop().await;
}

#[tokio::test]
async fn a_body_that_never_ends_is_answered_at_the_timeout() {
    let timeout = Duration::from_millis(400);
    let server = start(Config {
        timeout,
        ..Config::default()
    })
    .await;
    let mut s = TcpStream::connect(server.addr).await.unwrap();
    let start = Instant::now();
    s.write_all(
        b"POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
          Accept: application/json, text/event-stream\r\nContent-Length: 1000\r\n\r\n{\"jsonrpc\"",
    )
    .await
    .unwrap();
    let mut buf = [0_u8; 1024];
    let mut n = 0;
    // The status line, then the body: the same JSON error as every other failure.
    while n < buf.len() {
        let got = tokio::time::timeout(Duration::from_secs(5), s.read(&mut buf[n..]))
            .await
            .expect("answered at the timeout")
            .unwrap();
        n += got;
        if got == 0 || buf[..n].ends_with(b"}\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf[..n]);
    assert!(text.starts_with("HTTP/1.1 503"), "{text}");
    assert!(text.contains(r#""code":"unavailable""#), "{text}");
    assert!(start.elapsed() >= timeout.saturating_sub(SLACK));
    drop(s);
    server.stop().await;
}

#[tokio::test]
async fn a_connection_past_max_connections_waits_for_one_to_close() {
    let server = start(Config {
        max_connections: 1,
        header_timeout: Duration::from_secs(30),
        ..Config::default()
    })
    .await;
    // The one connection allowed, sending nothing.
    let holder = TcpStream::connect(server.addr).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut next = TcpStream::connect(server.addr).await.unwrap();
    next.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = [0_u8; 512];
    let waited = tokio::time::timeout(Duration::from_millis(500), next.read(&mut buf)).await;
    assert!(waited.is_err(), "served past the cap");

    drop(holder);
    let (bytes, _) = read_to_close(&mut next, Duration::from_secs(5)).await;
    assert!(
        bytes.starts_with(b"HTTP/1.1 200"),
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    server.stop().await;
}
