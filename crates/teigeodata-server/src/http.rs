//! The HTTP server (phase 6.2, feature `http`): axum over hyper (D-309), every query of
//! [`crate::query`] as a `GET` route, run inline on the runtime's worker.
//!
//! **Routes.** `GET /health`; `GET /packs`; under `/v1/`: `get?id=&names=`, `lookup?ids=a,b`,
//! `reverse?lat=&lon=&limit=&in_country=`, `near?id=&limit=&radius_m=`,
//! `search?q=&limit=&focus=lat,lon&scale_km=&strict=`, `hierarchy?id=`, `children?id=&limit=`,
//! `admin?lat=&lon=`,
//! `tz?lat=&lon=&t=` or `tz?id=&t=`, `elevation?lat=&lon=`,
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

use std::net::SocketAddr;
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
use tower_http::timeout::TimeoutLayer;

use crate::body::{Asked, Page, answer};
use crate::compat::{self, Route};
use crate::packs::Packs;
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            limits: Limits::SERVER,
            max_query_bytes: 4096,
            timeout: Duration::from_secs(10),
            cors_origins: Vec::new(),
            #[cfg(feature = "mcp")]
            mcp: None,
            #[cfg(feature = "mcp")]
            mcp_hosts: ["localhost", "127.0.0.1", "::1"].map(String::from).to_vec(),
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

/// The router over `app`, with its limits, timeout, CORS and panic guard.
pub fn router(app: Arc<App>) -> Router {
    let timeout = app.config.timeout;
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
            crate::mcp::http_service(m.clone(), &app.config.mcp_hosts, &app.config.cors_origins),
        ),
        None => r,
    };
    let mut r = r.fallback(not_found).with_state(app);
    if let Some(c) = cors {
        r = r.layer(c);
    }
    r.layer(TimeoutLayer::with_status_code(
        StatusCode::SERVICE_UNAVAILABLE,
        timeout,
    ))
    .layer(CatchPanicLayer::custom(|_| {
        error(&QueryError::new(Code::Internal, "the server failed"))
    }))
}

/// Serves `app` on `listener` until `shutdown` resolves, following the store's journal every
/// `follow_every` (D-172).
///
/// # Errors
/// The listener's.
pub async fn serve(
    listener: tokio::net::TcpListener,
    app: Arc<App>,
    follow_every: Option<Duration>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    if let Some(every) = follow_every {
        crate::packs::spawn_follow(Arc::clone(&app.packs), every);
    }
    axum::serve(
        listener,
        router(app).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
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
    let body = serde_json::to_vec(&serde_json::json!({
        "error": { "code": e.code.name(), "message": e.message }
    }))
    .unwrap_or_default();
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
    json(
        StatusCode::OK,
        serde_json::to_vec(&crate::output::packs(&e)).unwrap_or_default(),
        "application/json",
    )
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
