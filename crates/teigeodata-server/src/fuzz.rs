//! The fuzz harness of the HTTP query parsing (phase 6.2, feature `fuzz`): one property,
//! `query_string`, run on seeded inputs by `cargo test` (`tests/fuzz.rs`) and by libFuzzer
//! nightly (`fuzz/`, target `server_query`), as the format's harnesses are (D-072).

use teigeodata_engine::Engine;

use crate::body;
use crate::compat::{self, Route};
use crate::params::{self, Params, ROUTES};
use crate::query::{Code, Limits};

/// The parameter names the routes read, so random input reaches past "missing".
const KEYS: [&str; 28] = [
    "id",
    "ids",
    "names",
    "lat",
    "lon",
    "limit",
    "in_country",
    "radius_m",
    "q",
    "focus",
    "scale_km",
    "strict",
    "t",
    "code",
    "scheme",
    "prefix",
    "format",
    "x",
    "countrycodes",
    "viewbox",
    "bounded",
    "addressdetails",
    "zoom",
    "osm_ids",
    "json_callback",
    "city",
    "country",
    "postalcode",
];

/// Parses `data` as a route's query string and answers it on an engine with no packs, as the
/// server does, with the server's bounds: it must never panic, a refusal must carry a client
/// code (never `internal`), and an answer must be valid JSON.
///
/// The first byte's upper bits pick the route, its low bit the form of the rest: the bytes as a raw
/// query string, or segments split at `0xff`, each a key (its first byte, from the routes'
/// names, its low two bits the value's form) and a value made from the rest (`value`),
/// encoded as a browser would.
///
/// # Panics
/// When the property fails: that is the harness's report.
pub fn query_string(data: &[u8]) {
    let Some((&first, rest)) = data.split_first() else {
        return;
    };
    let route = ROUTES
        .get(usize::from(first >> 1) % ROUTES.len())
        .copied()
        .unwrap_or("none");
    let raw = if first & 1 == 0 {
        String::from_utf8_lossy(rest).into_owned()
    } else {
        let mut q = form_urlencoded::Serializer::new(String::new());
        for seg in rest.split(|&b| b == 0xff) {
            if let Some((&k, v)) = seg.split_first() {
                q.append_pair(
                    KEYS.get(usize::from(k >> 2) % KEYS.len())
                        .copied()
                        .unwrap_or("x"),
                    &value(k & 3, v),
                );
            }
        }
        q.finish()
    };
    let p = Params::parse(Some(&raw));
    let e = Engine::new();
    let compat = match route {
        "nominatim/search" => Some(Route::Search),
        "nominatim/reverse" => Some(Route::Reverse),
        "nominatim/lookup" => Some(Route::Lookup),
        _ => None,
    };
    if let Some(r) = compat {
        let mut out = Vec::new();
        match compat::answer(&e, r, &p, &Limits::SERVER, &mut out) {
            Ok(_) => {
                let v: Result<serde_json::Value, _> = serde_json::from_slice(&out);
                assert!(v.is_ok(), "not JSON: {}", String::from_utf8_lossy(&out));
            }
            Err(x) => assert!(x.code != Code::Internal, "{route}?{raw}: internal: {x}"),
        }
        return;
    }
    let Ok(asked) = params::asked(route, &p) else {
        return;
    };
    let mut out = Vec::new();
    match body::answer(
        &e,
        &asked,
        false,
        &Limits::SERVER,
        body::Page::ALL,
        &mut out,
    ) {
        Ok(_) => {
            let v: Result<serde_json::Value, _> = serde_json::from_slice(&out);
            assert!(v.is_ok(), "not JSON: {}", String::from_utf8_lossy(&out));
        }
        Err(x) => assert!(x.code != Code::Internal, "{route}?{raw}: internal: {x}"),
    }
}

/// A parameter's value from `bytes` in form `form`: the bytes as text, a small integer (a count,
/// a flag), a coordinate-like decimal, or a list of two decimals (`focus`), so inputs reach the
/// engine's paths, not only "not valid".
fn value(form: u8, bytes: &[u8]) -> String {
    let mut n = [0_u8; 8];
    for (d, b) in n.iter_mut().zip(bytes) {
        *d = *b;
    }
    let n = i64::from_le_bytes(n);
    // Degrees to six places, -180 to 180; integers from -2 to 61, around the bounds.
    let deg = |x: i64| f64::from(i32::try_from(x % 180_000_001).unwrap_or(0)) / 1e6;
    match form {
        0 => String::from_utf8_lossy(bytes).into_owned(),
        1 => (n.rem_euclid(64) - 2).to_string(),
        2 => deg(n).to_string(),
        _ => format!("{},{}", deg(n) / 2.0, deg(n >> 32)),
    }
}
