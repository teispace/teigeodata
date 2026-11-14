//! The HTTP server (phase 6.2, feature `http`): axum over hyper (D-309), every query of
//! [`crate::query`] as a `GET` route, run inline on the runtime's worker.
//!
//! **Routes.** `GET /health`; `GET /packs`; under `/v1/`: `get?id=&names=`, `lookup?ids=a,b`,
//! `reverse?lat=&lon=&limit=&in_country=`, `near?id=&limit=&radius_m=`,
//! `search?q=&limit=&focus=lat,lon&scale_km=&strict=`, `admin?lat=&lon=`,
//! `tz?lat=&lon=&t=` or `tz?id=&t=`, `elevation?lat=&lon=`,
//! `postal?code=&scheme=&prefix=&limit=` or `postal?lat=&lon=`. A place query answers
//! `{"results": [...], "attribution": ...}` (the native records, D-032), or a GeoJSON
//! FeatureCollection with `format=geojson`; `limit` defaults to [`Limits::DEFAULT_LIMIT`].
//! Unknown parameters are ignored. A failure answers `{"error": {"code", "message"}}` with the
//! code's status ([`Code`]).
//!
//! **Safe by default:** bound to `127.0.0.1` unless told otherwise; CORS off unless origins are
//! configured; a query string past [`Config::max_query_bytes`] is refused (414); every bound of
//! [`Limits`]; a request timeout; a panic answers `internal` (500) and the server goes on.
//! The engine's calls are short and bounded by their inputs, and they do not yield, so the
//! timeout bounds the time a request waits, not an engine call already running.

use std::cell::RefCell;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{ConnectInfo, RawQuery, State};
use axum::http::header::{CONTENT_TYPE, HeaderValue};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use teigeodata_engine::{Engine, PostalCode, Scratch};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::timeout::TimeoutLayer;

use crate::output::{self, Item, Mode};
use crate::packs::Packs;
use crate::query::{self, Ask, Code, Limits, PostalAsk, QueryError};
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
    pub cors_origins: Vec<HeaderValue>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            limits: Limits::SERVER,
            max_query_bytes: 4096,
            timeout: Duration::from_secs(10),
            cors_origins: Vec::new(),
        }
    }
}

/// What every request shares.
#[derive(Debug)]
pub struct App {
    /// The packs answered from.
    pub packs: Packs,
    /// The hosted seams.
    pub seams: Seams,
    /// How the server runs.
    pub config: Config,
}

thread_local! {
    /// Each worker's query memory, reused across requests (no allocation per query but the
    /// body).
    static WORK: RefCell<(Scratch, Vec<Item>, Vec<PostalCode>)> =
        RefCell::new((Scratch::default(), Vec::new(), Vec::new()));
}

/// The router over `app`, with its limits, timeout, CORS and panic guard.
pub fn router(app: Arc<App>) -> Router {
    let timeout = app.config.timeout;
    let cors = (!app.config.cors_origins.is_empty()).then(|| {
        CorsLayer::new()
            .allow_methods([Method::GET])
            .allow_origin(AllowOrigin::list(app.config.cors_origins.clone()))
    });
    let mut r = Router::new()
        .route("/health", get(health))
        .route("/packs", get(packs))
        .route("/v1/{query}", get(v1))
        .fallback(not_found)
        .with_state(app);
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
        let app = Arc::clone(&app);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(every);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                let app = Arc::clone(&app);
                // Opening a pack reads and checks its head: off the request workers.
                let _ = tokio::task::spawn_blocking(move || app.packs.follow()).await;
            }
        });
    }
    axum::serve(
        listener,
        router(app).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
}

/// A request's headers and peer, for the seams.
struct Request<'a> {
    headers: &'a HeaderMap,
    peer: SocketAddr,
}

impl Incoming for Request<'_> {
    fn header(&self, name: &str) -> Option<&[u8]> {
        let mut all = self.headers.get_all(name).iter();
        let one = all.next()?;
        all.next().is_none().then(|| one.as_bytes())
    }

    fn peer(&self) -> Option<SocketAddr> {
        Some(self.peer)
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
        peer,
    };
    if let Err(e) = app.seams.admit(&req, Operation::Packs) {
        return error(&e);
    }
    let e = app.packs.load();
    let rows: Vec<serde_json::Value> = e
        .layers()
        .map(|l| {
            serde_json::json!({
                "address": l.address(),
                "component": l.component(),
                "release": l.release(),
                "places": l.len(),
                "components": l.components().iter().map(teigeodata_engine::Component::name).collect::<Vec<_>>(),
                "attribution": l.attribution(),
            })
        })
        .collect();
    json(
        StatusCode::OK,
        serde_json::to_vec(&serde_json::json!({ "packs": rows })).unwrap_or_default(),
        "application/json",
    )
}

/// A request's query parameters, decoded once.
struct Params(Vec<(String, String)>);

impl Params {
    fn parse(raw: Option<&str>) -> Self {
        Self(
            form_urlencoded::parse(raw.unwrap_or("").as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect(),
        )
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn text(&self, name: &str) -> Result<String, QueryError> {
        self.get(name)
            .map(str::to_owned)
            .ok_or_else(|| QueryError::new(Code::InvalidArgument, format!("`{name}` is missing")))
    }

    fn parsed<T: core::str::FromStr>(&self, name: &str) -> Result<Option<T>, QueryError> {
        self.get(name)
            .map(|v| {
                v.parse().map_err(|_| {
                    QueryError::new(
                        Code::InvalidArgument,
                        format!("`{name}` is not valid: {v:?}"),
                    )
                })
            })
            .transpose()
    }

    fn need<T: core::str::FromStr>(&self, name: &str) -> Result<T, QueryError> {
        self.parsed(name)?
            .ok_or_else(|| QueryError::new(Code::InvalidArgument, format!("`{name}` is missing")))
    }

    fn flag(&self, name: &str) -> Result<bool, QueryError> {
        match self.get(name) {
            None | Some("0" | "false") => Ok(false),
            Some("" | "1" | "true") => Ok(true),
            Some(v) => Err(QueryError::new(
                Code::InvalidArgument,
                format!("`{name}` is not a flag: {v:?}"),
            )),
        }
    }

    fn limit(&self) -> Result<usize, QueryError> {
        Ok(self.parsed("limit")?.unwrap_or(Limits::DEFAULT_LIMIT))
    }
}

/// What a `/v1/` route asks.
enum Asked {
    Places(Ask),
    Elevation(f64, f64),
    Postal(PostalAsk),
}

impl Asked {
    const fn operation(&self) -> Operation {
        match self {
            Self::Places(a) => Operation::of(a),
            Self::Elevation(..) => Operation::Elevation,
            Self::Postal(_) => Operation::Postal,
        }
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The ask of route `/v1/<route>` with parameters `p`.
fn asked(route: &str, p: &Params) -> Result<Asked, QueryError> {
    let ask = match route {
        "get" => Ask::Get {
            id: p.text("id")?,
            names: p.flag("names")?,
        },
        "lookup" => Ask::Lookup {
            ids: p
                .text("ids")?
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
        },
        "reverse" => Ask::Reverse {
            lat: p.need("lat")?,
            lon: p.need("lon")?,
            limit: p.limit()?,
            in_country: p.flag("in_country")?,
        },
        "near" => Ask::Near {
            id: p.text("id")?,
            limit: p.limit()?,
            radius_m: p.parsed("radius_m")?,
        },
        "search" => Ask::Search {
            query: p.text("q")?,
            limit: p.limit()?,
            focus: match p.get("focus") {
                None => None,
                Some(f) => {
                    let bad = || {
                        QueryError::new(
                            Code::InvalidArgument,
                            format!("`focus` is not lat,lon: {f:?}"),
                        )
                    };
                    let (lat, lon) = f.split_once(',').ok_or_else(bad)?;
                    let scale_km: f64 = p.parsed("scale_km")?.unwrap_or(50.0);
                    Some((
                        lat.trim().parse().map_err(|_| bad())?,
                        lon.trim().parse().map_err(|_| bad())?,
                        scale_km * 1_000.0,
                    ))
                }
            },
            strict: p.flag("strict")?,
        },
        "admin" => Ask::Admin {
            lat: p.need("lat")?,
            lon: p.need("lon")?,
        },
        "tz" => {
            let t = p.parsed("t")?.unwrap_or_else(now);
            match p.get("id") {
                Some(id) => Ask::TzOf {
                    id: id.to_owned(),
                    t,
                },
                None => Ask::TzAt {
                    lat: p.need("lat")?,
                    lon: p.need("lon")?,
                    t,
                },
            }
        }
        "elevation" => return Ok(Asked::Elevation(p.need("lat")?, p.need("lon")?)),
        "postal" => {
            return Ok(Asked::Postal(match p.get("code") {
                Some(code) => PostalAsk::Code {
                    code: code.to_owned(),
                    scheme: p.get("scheme").map(str::to_owned),
                    prefix: p.flag("prefix")?,
                    limit: p.limit()?,
                },
                None => PostalAsk::At {
                    lat: p.need("lat")?,
                    lon: p.need("lon")?,
                },
            }));
        }
        _ => return Err(QueryError::new(Code::NotFound, "no such route")),
    };
    Ok(Asked::Places(ask))
}

/// Ends a native body: `"attribution": …}`.
fn close(out: &mut Vec<u8>, attribution: Option<&str>) {
    while out.last() == Some(&b'\n') {
        out.pop();
    }
    out.extend_from_slice(b",\"attribution\":");
    // A string or null cannot fail to serialize into a Vec.
    let _ = serde_json::to_writer(&mut *out, &attribution);
    out.extend_from_slice(b"}\n");
}

/// Runs `asked` on `e` and writes its body: the work of one request.
fn answer(
    e: &Engine,
    asked: &Asked,
    geojson: bool,
    limits: &Limits,
    out: &mut Vec<u8>,
) -> Result<usize, QueryError> {
    WORK.with_borrow_mut(|(s, items, codes)| {
        let fail = |x: output::OutputError| match x {
            output::OutputError::Engine(e) => QueryError::from(e),
            output::OutputError::Io(_) => QueryError::new(Code::Internal, "the answer failed"),
        };
        match asked {
            Asked::Places(ask) => {
                query::answer(e, ask, limits, s, items)?;
                if geojson {
                    output::write(e, items, Mode::Geojson, out).map_err(fail)?;
                } else {
                    out.extend_from_slice(b"{\"results\":");
                    output::write(e, items, Mode::Json, out).map_err(fail)?;
                    close(out, output::attribution_of(e, items).as_deref());
                }
                Ok(items.len())
            }
            Asked::Elevation(lat, lon) => {
                let h = query::elevation(e, *lat, *lon, s)?;
                out.extend_from_slice(b"{\"result\":");
                output::elevation(*lat, *lon, h.as_ref(), true, out).map_err(fail)?;
                close(
                    out,
                    output::attribution_layers(e, h.iter().map(|h| h.layer)).as_deref(),
                );
                Ok(usize::from(h.is_some()))
            }
            Asked::Postal(ask) => {
                query::postal(e, ask, limits, s, codes)?;
                out.extend_from_slice(b"{\"results\":");
                output::postal(e, codes, true, out).map_err(fail)?;
                close(
                    out,
                    output::attribution_layers(e, codes.iter().map(|c| c.place.layer)).as_deref(),
                );
                Ok(codes.len())
            }
        }
    })
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
        peer,
    };
    let caller: Caller = match app.seams.admit(&req, op) {
        Ok(c) => c,
        Err(e) => return error(&e),
    };
    let mut body = Vec::with_capacity(1024);
    let e = app.packs.load();
    match answer(&e, &asked, geojson, &app.config.limits, &mut body) {
        Ok(results) => {
            let usage = Usage {
                results,
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
