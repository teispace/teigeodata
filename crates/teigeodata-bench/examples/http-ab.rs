//! The phase 6.0 A/B ([servers](../../../docs/02-research/notes/servers.md) §5): the same two
//! handlers on axum and on bare hyper, so a load generator measures what the stack costs.
//!
//! ```text
//! cargo run --release -p teigeodata-bench --example http-ab -- <pack> <axum|hyper> <port> [inline|blocking]
//! oha -z 10s -c 16 'http://127.0.0.1:<port>/search?q=Pokhara&limit=10'
//! ```
//!
//! `GET /search?q=&limit=` answers the best matches and `GET /place?id=` one place, both as
//! JSON. The query string is parsed by `form_urlencoded` on both arms; the engine call runs on
//! the runtime's worker (`inline`) or through `spawn_blocking`.
#![allow(
    clippy::print_stderr,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a measurement harness: it prints its address and stops on any setup error"
)]

use std::cell::RefCell;
use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{RawQuery, State};
use axum::response::IntoResponse;
use bytes::Bytes;
use http_body_util::Full;
use hyper::header::{CONTENT_TYPE, HeaderValue};
use hyper::{Request, Response, StatusCode};
use serde::Serialize;
use teigeodata_engine::{Engine, Hit, Layer, LayerOptions, Load, Scratch};

thread_local! {
    /// Each worker's query memory, reused across requests.
    static SCRATCH: RefCell<(Scratch, Vec<Hit>)> = RefCell::new((Scratch::default(), Vec::new()));
}

#[derive(Serialize)]
struct Row<'a> {
    id: String,
    name: Option<&'a str>,
    lat: f64,
    lon: f64,
    score: i32,
}

/// The answer to a request: its status and JSON body.
fn answer(engine: &Engine, path: &str, query: Option<&str>) -> (StatusCode, Vec<u8>) {
    let mut name = [0u8; 1024];
    if path == "/search" {
        let (mut q, mut limit) = (String::new(), 10usize);
        for (k, v) in form_urlencoded::parse(query.unwrap_or("").as_bytes()) {
            match &*k {
                "q" => q = v.into_owned(),
                "limit" => limit = v.parse().unwrap_or(10).min(50),
                _ => {}
            }
        }
        return SCRATCH.with_borrow_mut(|(s, hits)| {
            hits.clear();
            if engine.search(&q, limit, s, hits).is_err() {
                return (StatusCode::INTERNAL_SERVER_ERROR, b"{}".to_vec());
            }
            let mut out = Vec::with_capacity(1024);
            out.push(b'[');
            for (i, h) in hits.iter().enumerate() {
                let n = engine.name(&h.place, &mut name).ok().flatten();
                let row = Row {
                    id: h.place.id.to_string(),
                    name: n,
                    lat: h.place.point.lat_degrees(),
                    lon: h.place.point.lon_degrees(),
                    score: h.score,
                };
                if i > 0 {
                    out.push(b',');
                }
                serde_json::to_writer(&mut out, &row).unwrap();
            }
            out.push(b']');
            (StatusCode::OK, out)
        });
    }
    if path == "/place" {
        let id = form_urlencoded::parse(query.unwrap_or("").as_bytes())
            .find(|(k, _)| k == "id")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();
        let Ok(id) = id.parse() else {
            return (StatusCode::BAD_REQUEST, b"{}".to_vec());
        };
        return match engine.get(id) {
            Ok(Some(p)) => {
                let n = engine.name(&p, &mut name).ok().flatten();
                let row = Row {
                    id: p.id.to_string(),
                    name: n,
                    lat: p.point.lat_degrees(),
                    lon: p.point.lon_degrees(),
                    score: 0,
                };
                (StatusCode::OK, serde_json::to_vec(&row).unwrap())
            }
            Ok(None) => (StatusCode::NOT_FOUND, b"{}".to_vec()),
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, b"{}".to_vec()),
        };
    }
    (StatusCode::NOT_FOUND, b"{}".to_vec())
}

/// [`answer`] inline, or on the blocking pool.
async fn run(
    engine: Arc<Engine>,
    blocking: bool,
    path: String,
    query: Option<String>,
) -> (StatusCode, Vec<u8>) {
    if blocking {
        tokio::task::spawn_blocking(move || answer(&engine, &path, query.as_deref()))
            .await
            .unwrap()
    } else {
        answer(&engine, &path, query.as_deref())
    }
}

fn json(status: StatusCode, body: Vec<u8>) -> Response<Full<Bytes>> {
    let mut r = Response::new(Full::new(Bytes::from(body)));
    *r.status_mut() = status;
    r.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    r
}

async fn serve_axum(engine: Arc<Engine>, blocking: bool, listener: tokio::net::TcpListener) {
    type St = State<(Arc<Engine>, bool)>;
    async fn search(State((e, b)): St, RawQuery(q): RawQuery) -> impl IntoResponse {
        let (s, body) = run(e, b, "/search".to_owned(), q).await;
        json(s, body)
    }
    async fn place(State((e, b)): St, RawQuery(q): RawQuery) -> impl IntoResponse {
        let (s, body) = run(e, b, "/place".to_owned(), q).await;
        json(s, body)
    }
    let app = axum::Router::new()
        .route("/search", axum::routing::get(search))
        .route("/place", axum::routing::get(place))
        .with_state((engine, blocking));
    axum::serve(listener, app).await.unwrap();
}

async fn serve_hyper(engine: Arc<Engine>, blocking: bool, listener: tokio::net::TcpListener) {
    loop {
        let (stream, _) = listener.accept().await.unwrap();
        let engine = Arc::clone(&engine);
        tokio::spawn(async move {
            let svc = hyper::service::service_fn(move |req: Request<hyper::body::Incoming>| {
                let engine = Arc::clone(&engine);
                async move {
                    let path = req.uri().path().to_owned();
                    let query = req.uri().query().map(str::to_owned);
                    let (s, body) = run(engine, blocking, path, query).await;
                    Ok::<_, Infallible>(json(s, body))
                }
            });
            let io = hyper_util::rt::TokioIo::new(stream);
            let _ =
                hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
                    .serve_connection(io, svc)
                    .await;
        });
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [pack, stack, port, rest @ ..] = args.as_slice() else {
        eprintln!("usage: http-ab <pack> <axum|hyper> <port> [inline|blocking]");
        std::process::exit(2);
    };
    let blocking = rest.first().is_some_and(|m| m == "blocking");
    let mut engine = Engine::new();
    engine.add(
        Layer::open(
            pack,
            LayerOptions {
                load: Load::Map,
                ..LayerOptions::default()
            },
        )
        .expect("the pack opens"),
    );
    let engine = Arc::new(engine);
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    eprintln!(
        "{stack} on 127.0.0.1:{port}, engine calls {}",
        if blocking { "blocking" } else { "inline" }
    );
    match stack.as_str() {
        "axum" => serve_axum(engine, blocking, listener).await,
        _ => serve_hyper(engine, blocking, listener).await,
    }
}
