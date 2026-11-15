//! The HTTP server (phase 6.2, feature `http`): axum over hyper (D-309), every query of
//! [`crate::query`] as a `GET` route, run inline on the runtime's worker.
//!
//! **Routes.** `GET /health`; `GET /packs`; under `/v1/`: `get?id=&names=`, `lookup?ids=a,b`,
//! `reverse?lat=&lon=&limit=&in_country=`, `near?id=&limit=&radius_m=`,
//! `search?q=&limit=&focus=lat,lon&scale_km=&strict=`, `hierarchy?id=`, `children?id=&limit=`,
//! `admin?lat=&lon=`,
//! `tz?lat=&lon=&t=` or `tz?id=&t=`, `elevation?lat=&lon=`, `boundary?id=&lod=` (a GeoJSON
//! feature), `distance?from=&to=` (each a place's ID or `lat,lon`),
//! `coverage?lat=&lon=&radius_m=` (the countries there and the store's catalog packs for them;
//! a server over a store only, D-318),
//! `postal?code=&scheme=&prefix=&limit=` or `postal?lat=&lon=`. A place query answers
//! `{"results": [...], "attribution": ...}` (the native records, D-032), or a GeoJSON
//! FeatureCollection with `format=geojson`; `limit` defaults to [`Limits::DEFAULT_LIMIT`].
//! Unknown parameters are ignored. A failure answers `{"error": {"code", "message"}}` with the
//! code's status ([`Code`]). With the `mcp` feature and `Config::mcp` set, `POST /mcp` is the
//! MCP server as stateless Streamable HTTP (D-316).
//!
//! **Safe by default:** bound to `127.0.0.1` unless told otherwise; CORS off unless origins are
//! configured; a query string past [`Config::max_query_bytes`] is refused (414); every bound of
//! [`Limits`]; a request timeout; a panic answers `internal` (500) and the server goes on.
//! The engine's calls are short and bounded by their inputs, and they do not yield, so the
//! timeout bounds the time a request waits, not an engine call already running.
//!
//! **Bounded against floods and slow clients** ([`serve`], D-334): at most
//! [`Config::max_connections`] connections (more wait in the kernel's backlog), at most
//! [`Config::max_in_flight`] requests answered at once (one past it is refused at once, 503
//! with `Retry-After`), a request head sent within [`Config::header_timeout`] (which also closes
//! an idle HTTP/1 connection), HTTP/2 peers pinged and at most 32 streams each.

use std::net::SocketAddr;
use std::pin::pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{ConnectInfo, RawQuery, State};
use axum::http::header::{ACCEPT, CONTENT_TYPE, HeaderName, HeaderValue};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::body::{Asked, Page, answer};
use crate::compat::{self, Route};
use crate::packs::Packs;
pub use crate::params::{ENDPOINTS, Endpoint, FORMAT, Param};
use crate::params::{Params, asked};
use crate::query::{Code, Limits, QueryError};
use crate::seams::{Caller, Incoming, Operation, Seams, Usage};

/// How the server runs.
#[derive(Clone, Debug)]
pub struct Config {
    /// The bounds of every query.
    pub limits: Limits,
    /// The longest query string in bytes; longer is refused with 414.
    pub max_query_bytes: usize,
    /// How long a request may take before it is answered `unavailable` (503).
    pub timeout: Duration,
    /// How long a client may take to send a request's head; an idle HTTP/1 connection is
    /// closed after it too ([`serve`]).
    pub header_timeout: Duration,
    /// The most connections [`serve`] keeps open; at the cap it stops accepting, and new
    /// connections wait in the kernel's backlog.
    pub max_connections: usize,
    /// The most requests answered at once; one past it is refused at once (`unavailable`, 503,
    /// `Retry-After: 1`) rather than queued.
    pub max_in_flight: usize,
    /// The origins a browser may call from (CORS); none, the default, sends no CORS headers.
    /// `/mcp` refuses a request from any other origin (MCP requires the check).
    pub cors_origins: Vec<HeaderValue>,
    /// The MCP server answered at `/mcp` (stateless Streamable HTTP, D-316); `None` leaves the
    /// route out.
    #[cfg(feature = "mcp")]
    pub mcp: Option<crate::mcp::Mcp>,
    /// The `Host` values `/mcp` accepts, against DNS rebinding of a server on loopback; empty
    /// accepts any (a server bound beyond loopback, where the name is the operator's).
    #[cfg(feature = "mcp")]
    pub mcp_hosts: Vec<String>,
    /// The port the server listens on. With `mcp_hosts` set (a server on loopback), `/mcp`
    /// also accepts the server's own origin, `http://` a loopback name and this port: a page
    /// it serves is not a rebinding attack. Another port is another site and is refused.
    #[cfg(feature = "mcp")]
    pub mcp_port: Option<u16>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            limits: Limits::SERVER,
            max_query_bytes: 4096,
            timeout: Duration::from_secs(10),
            header_timeout: Duration::from_secs(10),
            max_connections: 1024,
            max_in_flight: 256,
            cors_origins: Vec::new(),
            #[cfg(feature = "mcp")]
            mcp: None,
            #[cfg(feature = "mcp")]
            mcp_hosts: ["localhost", "127.0.0.1", "::1"].map(String::from).to_vec(),
            #[cfg(feature = "mcp")]
            mcp_port: None,
        }
    }
}

/// What every request shares.
#[derive(Debug)]
pub struct App {
    /// The packs answered from.
    pub packs: Arc<Packs>,
    /// The hosted seams.
    pub seams: Seams,
    /// How the server runs.
    pub config: Config,
}

/// The router over `app`, with its limits, timeout, CORS, panic guard and load shedding.
pub fn router(app: Arc<App>) -> Router {
    let timeout = app.config.timeout;
    let in_flight = app
        .config
        .max_in_flight
        .clamp(1, tokio::sync::Semaphore::MAX_PERMITS);
    let cors = (!app.config.cors_origins.is_empty()).then(|| {
        CorsLayer::new()
            // `POST` and these headers for `/mcp`; the other routes answer `GET` only.
            .allow_methods([Method::GET, Method::POST])
            .allow_headers([
                CONTENT_TYPE,
                ACCEPT,
                HeaderName::from_static("mcp-protocol-version"),
                HeaderName::from_static("mcp-method"),
                HeaderName::from_static("mcp-name"),
            ])
            .allow_origin(AllowOrigin::list(app.config.cors_origins.clone()))
    });
    let r = Router::new()
        .route("/health", get(health))
        .route("/packs", get(packs))
        .route("/v1/{query}", get(v1))
        .route("/search", get(nominatim_search))
        .route("/reverse", get(nominatim_reverse))
        .route("/lookup", get(nominatim_lookup));
    #[cfg(feature = "mcp")]
    let r = match &app.config.mcp {
        Some(m) => r.route_service(
            "/mcp",
            crate::mcp::http_service(
                m.clone(),
                &app.config.mcp_hosts,
                app.config.mcp_port,
                &app.config.cors_origins,
            ),
        ),
        None => r,
    };
    let mut r = r.fallback(not_found).with_state(app);
    if let Some(c) = cors {
        r = r.layer(c);
    }
    // A request past `timeout` gets the JSON `unavailable` error, as every other failure does.
    r.layer(
        tower::ServiceBuilder::new()
            .layer(axum::error_handling::HandleErrorLayer::new(timed_out))
            .layer(tower::timeout::TimeoutLayer::new(timeout)),
    )
    .layer(CatchPanicLayer::custom(|_| {
        error(&QueryError::new(Code::Internal, "the server failed"))
    }))
    // Outermost, so a request past the cap costs no more than its refusal. axum applies a
    // layer to each route on its own, so the semaphore is made once and shared: a
    // `ConcurrencyLimitLayer` would make one per route.
    .layer(
        tower::ServiceBuilder::new()
            .layer(axum::error_handling::HandleErrorLayer::new(overloaded))
            .load_shed()
            .layer(tower::limit::GlobalConcurrencyLimitLayer::with_semaphore(
                Arc::new(tokio::sync::Semaphore::new(in_flight)),
            )),
    )
}

/// The answer to a request past [`Config::timeout`].
async fn timed_out(_: tower::BoxError) -> Response {
    error(&QueryError::new(
        Code::Unavailable,
        "the request took longer than the server allows; ask again, or ask for less",
    ))
}

/// The answer to a request past [`Config::max_in_flight`].
async fn overloaded(_: tower::BoxError) -> Response {
    let mut r = error(&QueryError::new(
        Code::Unavailable,
        "the server is answering as many requests as it may; ask again shortly",
    ));
    r.headers_mut().insert(
        axum::http::header::RETRY_AFTER,
        HeaderValue::from_static("1"),
    );
    r
}

/// Serves `app` on `listener` until `shutdown` resolves, following the store's journal every
/// `follow_every` (D-172). Each connection is bounded as the module says (D-334); at shutdown,
/// accepting stops and requests in progress may finish within the request timeout.
///
/// # Errors
/// None today: an accept error is waited out and the loop goes on. The `Result` is kept for
/// errors a later listener may raise.
pub async fn serve(
    listener: tokio::net::TcpListener,
    app: Arc<App>,
    follow_every: Option<Duration>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    if let Some(every) = follow_every {
        crate::packs::spawn_follow(Arc::clone(&app.packs), every);
    }
    let header_timeout = app.config.header_timeout;
    // Requests in progress at shutdown may finish, within a request's own bound.
    let drain = app.config.timeout.saturating_add(Duration::from_secs(1));
    let permits = Arc::new(tokio::sync::Semaphore::new(
        app.config
            .max_connections
            .clamp(1, tokio::sync::Semaphore::MAX_PERMITS),
    ));
    let router = router(app);
    let graceful = hyper_util::server::graceful::GracefulShutdown::new();
    let mut shutdown = pin!(shutdown);
    loop {
        // At the cap, stop accepting: new connections wait in the kernel's backlog.
        let permit = tokio::select! {
            p = Arc::clone(&permits).acquire_owned() => match p {
                Ok(p) => p,
                Err(_) => break,
            },
            () = &mut shutdown => break,
        };
        let (stream, peer) = tokio::select! {
            a = listener.accept() => match a {
                Ok(a) => a,
                Err(e) if is_connection_error(&e) => continue,
                // Out of file descriptors or the like: wait, as axum does, then go on.
                Err(_) => {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            },
            () = &mut shutdown => break,
        };
        let _ = stream.set_nodelay(true);
        let service = tower::ServiceExt::map_request(
            router.clone(),
            move |mut req: axum::http::Request<hyper::body::Incoming>| {
                req.extensions_mut().insert(ConnectInfo(peer));
                req.map(axum::body::Body::new)
            },
        );
        let watcher = graceful.watcher();
        tokio::spawn(async move {
            let mut builder =
                hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new());
            builder
                .http1()
                .timer(hyper_util::rt::TokioTimer::new())
                .header_read_timeout(header_timeout);
            builder
                .http2()
                .timer(hyper_util::rt::TokioTimer::new())
                .keep_alive_interval(Some(Duration::from_secs(20)))
                .keep_alive_timeout(Duration::from_secs(20))
                .max_concurrent_streams(32)
                .enable_connect_protocol();
            let conn = builder.serve_connection_with_upgrades(
                hyper_util::rt::TokioIo::new(stream),
                hyper_util::service::TowerToHyperService::new(service),
            );
            let _ = watcher.watch(conn.into_owned()).await;
            drop(permit);
        });
    }
    drop(listener);
    let _ = tokio::time::timeout(drain, graceful.shutdown()).await;
    Ok(())
}

/// An error of one connection, not of the listener.
fn is_connection_error(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
    )
}

/// A request's headers and peer, for the seams (MCP's `/mcp` reads them too).
pub(crate) struct Request<'a> {
    pub(crate) headers: &'a HeaderMap,
    pub(crate) peer: Option<SocketAddr>,
}

impl Incoming for Request<'_> {
    fn header(&self, name: &str) -> Option<&[u8]> {
        let mut all = self.headers.get_all(name).iter();
        let one = all.next()?;
        all.next().is_none().then(|| one.as_bytes())
    }

    fn peer(&self) -> Option<SocketAddr> {
        self.peer
    }
}

fn json(status: StatusCode, body: Vec<u8>, content_type: &'static str) -> Response {
    let mut r = (status, body).into_response();
    r.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    r
}

fn error(e: &QueryError) -> Response {
    let mut body = serde_json::to_vec(&serde_json::json!({
        "error": { "code": e.code.name(), "message": e.message }
    }))
    .unwrap_or_default();
    body.push(b'\n');
    let status =
        StatusCode::from_u16(e.code.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    json(status, body, "application/json")
}

async fn not_found() -> Response {
    error(&QueryError::new(Code::NotFound, "no such route"))
}

async fn health(State(app): State<Arc<App>>) -> Response {
    let layers = app.packs.load().layers().len();
    json(
        StatusCode::OK,
        format!("{{\"status\":\"ok\",\"layers\":{layers}}}\n").into_bytes(),
        "application/json",
    )
}

async fn packs(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let req = Request {
        headers: &headers,
        peer: Some(peer),
    };
    if let Err(e) = app.seams.admit(&req, Operation::Packs) {
        return error(&e);
    }
    let e = app.packs.load();
    let mut body = serde_json::to_vec(&crate::output::packs(&e)).unwrap_or_default();
    body.push(b'\n');
    json(StatusCode::OK, body, "application/json")
}

async fn v1(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    axum::extract::Path(route): axum::extract::Path<String>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Response {
    let start = Instant::now();
    if raw
        .as_ref()
        .is_some_and(|q| q.len() > app.config.max_query_bytes)
    {
        let mut r = error(&QueryError::new(
            Code::OutOfBounds,
            format!(
                "the query string is past {} bytes",
                app.config.max_query_bytes
            ),
        ));
        *r.status_mut() = StatusCode::URI_TOO_LONG;
        return r;
    }
    let p = Params::parse(raw.as_deref());
    let asked = match asked(&route, &p) {
        Ok(a) => a,
        Err(e) => return error(&e),
    };
    let geojson = match p.get("format") {
        // An outline is a GeoJSON feature in any format asked.
        _ if matches!(asked, Asked::Boundary(..)) => true,
        None | Some("json") => false,
        Some("geojson") if matches!(asked, Asked::Places(_)) => true,
        Some(f) => {
            return error(&QueryError::new(
                Code::InvalidArgument,
                format!("`format` must be json or geojson here: {f:?}"),
            ));
        }
    };
    let op = asked.operation();
    let req = Request {
        headers: &headers,
        peer: Some(peer),
    };
    let caller: Caller = match app.seams.admit(&req, op) {
        Ok(c) => c,
        Err(e) => return error(&e),
    };
    let mut body = Vec::with_capacity(1024);
    let e = app.packs.load();
    match answer(
        &e,
        app.packs.root(),
        &asked,
        geojson,
        &app.config.limits,
        Page::ALL,
        &mut body,
    ) {
        Ok(done) => {
            let usage = Usage {
                results: done.results,
                bytes: body.len(),
                micros: u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX),
            };
            app.seams.meter.record(&caller, op, usage);
            json(
                StatusCode::OK,
                body,
                if geojson {
                    "application/geo+json"
                } else {
                    "application/json"
                },
            )
        }
        Err(x) => error(&x),
    }
}

async fn nominatim_search(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Response {
    nominatim(&app, Route::Search, peer, &headers, raw.as_deref())
}

async fn nominatim_reverse(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Response {
    nominatim(&app, Route::Reverse, peer, &headers, raw.as_deref())
}

async fn nominatim_lookup(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Response {
    nominatim(&app, Route::Lookup, peer, &headers, raw.as_deref())
}

/// A failure as Nominatim answers it, with the code's status.
fn nominatim_error(e: &QueryError, status: Option<StatusCode>) -> Response {
    let status = status.unwrap_or_else(|| {
        StatusCode::from_u16(e.code.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    });
    json(status, compat::error_body(e), "application/json")
}

/// A Nominatim-compatible request (D-314): the seams, the bounds and the panic guard as for
/// `/v1/`, Nominatim's shapes for the answer and the errors.
fn nominatim(
    app: &App,
    route: Route,
    peer: SocketAddr,
    headers: &HeaderMap,
    raw: Option<&str>,
) -> Response {
    let start = Instant::now();
    if raw.is_some_and(|q| q.len() > app.config.max_query_bytes) {
        let e = QueryError::new(
            Code::OutOfBounds,
            format!(
                "the query string is past {} bytes",
                app.config.max_query_bytes
            ),
        );
        return nominatim_error(&e, Some(StatusCode::URI_TOO_LONG));
    }
    let p = Params::parse(raw);
    let op = route.operation();
    let req = Request {
        headers,
        peer: Some(peer),
    };
    let caller = match app.seams.admit(&req, op) {
        Ok(c) => c,
        Err(e) => return nominatim_error(&e, None),
    };
    let mut body = Vec::with_capacity(1024);
    let e = app.packs.load();
    match compat::answer(&e, route, &p, &app.config.limits, &mut body) {
        Ok(results) => {
            let usage = Usage {
                results,
                bytes: body.len(),
                micros: u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX),
            };
            app.seams.meter.record(&caller, op, usage);
            let geo = p.get("format") == Some("geojson");
            json(
                StatusCode::OK,
                body,
                if geo {
                    "application/geo+json"
                } else {
                    "application/json"
                },
            )
        }
        Err(x) => nominatim_error(&x, None),
    }
}
