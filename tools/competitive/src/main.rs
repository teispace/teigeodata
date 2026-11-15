//! Suite C, the competitive report ([research](../../docs/02-research/notes/competitive-suite.md)).
//!
//! Asks teigeodata (in process, and `tgd serve` over HTTP) and its rivals (Nominatim, Photon,
//! GeoNames' own points, `tzf-rs`) the same questions, from files in the repository, and writes
//! one Markdown report:
//!
//! 1. **Where is the place?** Each labelled place of `tests/golden/<cc>-places.csv` is asked
//!    by name; the first answer is measured against the label. GeoNames and teigeodata are also
//!    asked by identity (the place's own record), the floor a name search cannot beat.
//! 2. **Search recall:** the golden queries of `tests/golden/<cc>.yaml`; a hit is an answer
//!    within [`HIT_M`] of an expected place (within [`HIT_UNIT_M`] for an admin unit), so
//!    rivals with their own IDs are scored on the same terms.
//! 3. **Timezone at a point:** seeded random points on the globe; teigeodata's `tz_at`
//!    against `tzf-rs`.
//! 4. **Latency** of every call above, and of `reverse` at each labelled point.
//! 5. **Footprint,** from a table the workflow writes as it sets each rival up.
//!
//! ```text
//! competitive --cc np --pack world-np-lite.tgd --registry np-ids-lite.tsv \
//!   [--geonames NP.txt] [--tgd http://127.0.0.1:8080] [--nominatim http://127.0.0.1:8088] \
//!   [--photon http://127.0.0.1:2322] [--footprint footprint.tsv] [--tz-points 20000] \
//!   [--out report.md]
//! ```
//!
//! A rival left out is not asked; the report says which were. Not a gate: a rival improving
//! is not teigeodata's failure, and its own regressions are caught by the Q, L and golden gates.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;
use teigeodata_core::{E6, E7, Namespace, PlaceId, Point};
use teigeodata_engine::{Engine, Hit, Layer, LayerOptions, Near, Scratch, meters_between};

/// A first answer within this of the label counts as the place found.
const FOUND_M: f64 = 5_000.0;
/// A search answer within this of an expected settlement is a hit.
const HIT_M: f64 = 2_000.0;
/// … and within this of an expected admin unit, whose point each source puts differently.
const HIT_UNIT_M: f64 = 5_000.0;
/// Answers read per search: recall@1 and recall@5.
const DEPTH: usize = 5;
/// Hits asked for before the country filter, the server's most (`limit` at most 40).
const FILTERED_K: usize = 40;
/// The `tzf-rs` this harness is built with, as `Cargo.toml` pins it.
const TZF_VERSION: &str = "2.3.0";

type LatLon = (f64, f64);

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("competitive: {e}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Default)]
struct Args {
    cc: String,
    packs: Vec<PathBuf>,
    registry: Option<PathBuf>,
    geonames: Option<PathBuf>,
    tgd: Option<String>,
    nominatim: Option<String>,
    photon: Option<String>,
    footprint: Option<PathBuf>,
    tz_points: usize,
    out: Option<PathBuf>,
    root: PathBuf,
}

const USAGE: &str = "usage: competitive --cc <cc> --pack <tgd>… --registry <tsv> [--geonames NP.txt] \
     [--tgd URL] [--nominatim URL] [--photon URL] [--footprint tsv] [--tz-points n] [--out md] \
     [--root repo]";

fn args() -> Result<Args, String> {
    let mut a = Args {
        tz_points: 20_000,
        root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        ..Args::default()
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || {
            it.next()
                .ok_or_else(|| format!("{k} needs a value\n{USAGE}"))
        };
        match k.as_str() {
            "--cc" => a.cc = v()?.to_ascii_lowercase(),
            "--pack" => a.packs.push(v()?.into()),
            "--registry" => a.registry = Some(v()?.into()),
            "--geonames" => a.geonames = Some(v()?.into()),
            "--tgd" => a.tgd = Some(v()?),
            "--nominatim" => a.nominatim = Some(v()?),
            "--photon" => a.photon = Some(v()?),
            "--footprint" => a.footprint = Some(v()?.into()),
            "--tz-points" => a.tz_points = v()?.parse().map_err(|e| format!("--tz-points: {e}"))?,
            "--out" => a.out = Some(v()?.into()),
            "--root" => a.root = v()?.into(),
            _ => return Err(USAGE.to_owned()),
        }
    }
    if a.cc.len() != 2 || a.packs.is_empty() || a.registry.is_none() {
        return Err(USAGE.to_owned());
    }
    Ok(a)
}

// ---------------------------------------------------------------------------------------------
// Rivals
// ---------------------------------------------------------------------------------------------

/// One system asked the same questions. Each call is timed by the caller.
trait Rival {
    /// The name in the report.
    fn name(&self) -> &str;
    /// The points of up to `k` answers to `query`, best first.
    fn search(&mut self, query: &str, k: usize) -> Result<Vec<LatLon>, String>;
    /// The point of the nearest place to `at`, if any.
    fn reverse(&mut self, at: LatLon) -> Result<Option<LatLon>, String>;
}

/// teigeodata in process: the engine over the packs given.
struct Local {
    engine: Engine,
    /// The country's namespace: search keeps its hits, as `countrycodes` does over HTTP.
    ns: Namespace,
    s: Scratch,
    hits: Vec<Hit>,
    near: Vec<Near>,
}

impl Rival for Local {
    fn name(&self) -> &'static str {
        "teigeodata (in process)"
    }
    fn search(&mut self, query: &str, k: usize) -> Result<Vec<LatLon>, String> {
        // Ask for the most, as the server does under a filter, then keep the country's `k`.
        self.engine
            .search(query, FILTERED_K, &mut self.s, &mut self.hits)
            .map_err(|e| e.to_string())?;
        Ok(self
            .hits
            .iter()
            .filter(|h| h.place.id.namespace() == self.ns)
            .take(k)
            .map(|h| deg(h.place.point))
            .collect())
    }
    fn reverse(&mut self, at: LatLon) -> Result<Option<LatLon>, String> {
        let p = Point::<E6>::from_degrees(at.0, at.1).map_err(|e| e.to_string())?;
        self.engine
            .reverse(p, 1, &mut self.s, &mut self.near)
            .map_err(|e| e.to_string())?;
        Ok(self.near.first().map(|n| deg(n.place.point)))
    }
}

fn deg(p: Point<E6>) -> LatLon {
    (p.lat_degrees(), p.lon_degrees())
}

/// An HTTP service, by the API it speaks.
struct Http {
    name: String,
    base: String,
    api: Api,
    cc: String,
    agent: ureq::Agent,
}

#[derive(Clone, Copy)]
enum Api {
    /// Nominatim's `/search` and `/reverse` (Nominatim itself, and `tgd serve`'s compatible
    /// routes): JSON arrays of objects with `lat` and `lon` as strings.
    Nominatim,
    /// Photon's `/api` and `/reverse`: GeoJSON features.
    Photon,
}

impl Http {
    fn new(name: &str, base: &str, api: Api, cc: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        Self {
            name: name.to_owned(),
            base: base.trim_end_matches('/').to_owned(),
            api,
            cc: cc.to_owned(),
            agent,
        }
    }

    fn get(&self, route: &str, query: &[(&str, &str)]) -> Result<Value, String> {
        let mut r = self.agent.get(format!("{}{route}", self.base));
        for (k, v) in query {
            r = r.query(*k, *v);
        }
        r.call()
            .map_err(|e| format!("{} {route}: {e}", self.name))?
            .body_mut()
            .read_json::<Value>()
            .map_err(|e| format!("{} {route}: {e}", self.name))
    }
}

impl Rival for Http {
    fn name(&self) -> &str {
        &self.name
    }
    fn search(&mut self, query: &str, k: usize) -> Result<Vec<LatLon>, String> {
        let k = k.to_string();
        match self.api {
            Api::Nominatim => {
                let v = self.get(
                    "/search",
                    &[
                        ("q", query),
                        ("limit", &k),
                        ("countrycodes", &self.cc),
                        ("format", "jsonv2"),
                    ],
                )?;
                Ok(nominatim_points(&v))
            }
            Api::Photon => {
                let v = self.get("/api", &[("q", query), ("limit", &k)])?;
                Ok(photon_points(&v))
            }
        }
    }
    fn reverse(&mut self, at: LatLon) -> Result<Option<LatLon>, String> {
        let (lat, lon) = (at.0.to_string(), at.1.to_string());
        match self.api {
            Api::Nominatim => {
                let v = self.get(
                    "/reverse",
                    &[("lat", &lat), ("lon", &lon), ("format", "jsonv2")],
                )?;
                // One object, or an `error` object where nothing is near.
                Ok(nominatim_point(&v))
            }
            Api::Photon => {
                let v = self.get("/reverse", &[("lat", &lat), ("lon", &lon), ("limit", "1")])?;
                Ok(photon_points(&v).into_iter().next())
            }
        }
    }
}

fn nominatim_point(o: &Value) -> Option<LatLon> {
    let f = |k: &str| o.get(k)?.as_str()?.parse::<f64>().ok();
    Some((f("lat")?, f("lon")?))
}

fn nominatim_points(v: &Value) -> Vec<LatLon> {
    v.as_array()
        .map(|a| a.iter().filter_map(nominatim_point).collect())
        .unwrap_or_default()
}

fn photon_points(v: &Value) -> Vec<LatLon> {
    v.get("features")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|f| {
                    let c = f.get("geometry")?.get("coordinates")?.as_array()?;
                    Some((c.get(1)?.as_f64()?, c.first()?.as_f64()?))
                })
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------------------------

/// A labelled place of `<cc>-places.csv` with a mark.
struct Labelled {
    id: String,
    name: String,
    class: String,
    label: LatLon,
}

fn labelled(file: &Path) -> Result<Vec<Labelled>, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split(',').collect();
        let num = |i: usize| f.get(i).and_then(|s| s.trim().parse::<f64>().ok());
        if let (Some(lat), Some(lon)) = (num(7), num(8)) {
            out.push(Labelled {
                id: f.first().copied().unwrap_or_default().to_owned(),
                name: f.get(1).copied().unwrap_or_default().to_owned(),
                class: f.get(2).copied().unwrap_or_default().to_owned(),
                label: (lat, lon),
            });
        }
    }
    Ok(out)
}

#[derive(Deserialize)]
struct GoldenFile {
    queries: Vec<GoldenQuery>,
}

#[derive(Deserialize)]
struct GoldenQuery {
    class: String,
    query: String,
    expect: BTreeMap<String, u8>,
}

/// Source key → live serial of `build` (following `>N` retirements), from an ID registry
/// (`serial \t state \t keys`, the format `cargo xtask ids` keeps).
fn registry(file: &Path, build: &str) -> Result<HashMap<String, u64>, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let (mut keys, mut state) = (HashMap::new(), HashMap::new());
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.split('\t');
        let (Some(serial), Some(st), Some(ks)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let Ok(serial) = serial.parse::<u64>() else {
            continue;
        };
        state.insert(serial, st.to_owned());
        for k in ks.split(' ') {
            keys.insert(k.to_owned(), serial);
        }
    }
    let live = |mut s: u64| {
        for _ in 0..64 {
            match state.get(&s) {
                Some(st) if st.starts_with('>') => s = st.get(1..)?.parse().ok()?,
                Some(st) if st.split(',').any(|b| b == build) => return Some(s),
                _ => return None,
            }
        }
        None
    };
    Ok(keys
        .into_iter()
        .filter_map(|(k, s)| Some((k, live(s)?)))
        .collect())
}

/// `geonames:<id>` → its point, from a GeoNames dump (`NP.txt`).
fn geonames(file: &Path) -> Result<HashMap<String, LatLon>, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(text
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let p = (f.get(4)?.parse().ok()?, f.get(5)?.parse().ok()?);
            Some((format!("geonames:{}", f.first()?), p))
        })
        .collect())
}

/// Rows of the footprint table the workflow writes: rival, version, setup seconds, disk
/// bytes, memory bytes, a note.
fn footprint(file: &Path) -> Result<Vec<Vec<String>>, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split('\t').map(str::to_owned).collect())
        .collect())
}

// ---------------------------------------------------------------------------------------------
// Measures
// ---------------------------------------------------------------------------------------------

/// The `at` quantile (nearest rank) of `v`, sorted in place; NaN when empty.
fn quantile(v: &mut [f64], at: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a rank within a short list"
    )]
    let i = ((at * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1;
    v[i]
}

#[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
fn pct(n: usize, of: usize) -> String {
    if of == 0 {
        "–".to_owned()
    } else {
        format!("{:.0}%", n as f64 * 100.0 / of as f64)
    }
}

fn km(m: f64) -> String {
    if m.is_nan() {
        "–".to_owned()
    } else if m < 10_000.0 {
        format!("{:.2} km", m / 1000.0)
    } else {
        format!("{:.0} km", m / 1000.0)
    }
}

fn ms(v: &mut [f64]) -> (String, String) {
    let f = |x: f64| {
        if x.is_nan() {
            "–".to_owned()
        } else if x < 0.001 {
            format!("{:.0} ns", x * 1e6)
        } else if x < 1.0 {
            format!("{:.0} µs", x * 1000.0)
        } else {
            format!("{x:.1} ms")
        }
    };
    (f(quantile(v, 0.5)), f(quantile(v, 0.95)))
}

fn m(a: LatLon, b: LatLon) -> f64 {
    meters_between(a.0, a.1, b.0, b.1)
}

/// Times of each call kind, in milliseconds, per rival.
#[derive(Default)]
struct Times(BTreeMap<&'static str, Vec<f64>>);

impl Times {
    fn time<T>(&mut self, kind: &'static str, f: impl FnOnce() -> T) -> T {
        let t = Instant::now();
        let r = f();
        self.0
            .entry(kind)
            .or_default()
            .push(t.elapsed().as_secs_f64() * 1000.0);
        r
    }
}

/// One rival's answers to question 1.
struct Found {
    asked: usize,
    found: usize,
    errors: usize,
    distances: Vec<f64>,
}

fn places(r: &mut dyn Rival, set: &[Labelled], times: &mut Times) -> Found {
    let mut f = Found {
        asked: set.len(),
        found: 0,
        errors: 0,
        distances: Vec::new(),
    };
    for p in set {
        match times.time("search", || r.search(&p.name, 1)) {
            Ok(a) => {
                if let Some(&first) = a.first() {
                    let d = m(first, p.label);
                    if d <= FOUND_M {
                        f.found += 1;
                        f.distances.push(d);
                    }
                }
            }
            Err(_) => f.errors += 1,
        }
    }
    f
}

/// Recall by class: (asked, hit@1, hit@5).
type Recall = BTreeMap<String, (usize, usize, usize)>;

/// The expected points of each golden query (grade ≥ 2), resolved through the registry and the
/// packs, and the radius each must be within.
struct Expected {
    class: String,
    query: String,
    points: Vec<(LatLon, f64)>,
}

fn recall(r: &mut dyn Rival, set: &[Expected], times: &mut Times) -> (Recall, usize) {
    let (mut out, mut errors) = (Recall::new(), 0);
    for q in set {
        let e = out.entry(q.class.clone()).or_default();
        e.0 += 1;
        let Ok(answers) = times.time("search", || r.search(&q.query, DEPTH)) else {
            errors += 1;
            continue;
        };
        let hit = |a: &LatLon| q.points.iter().any(|&(p, r)| m(*a, p) <= r);
        if answers.first().is_some_and(hit) {
            e.1 += 1;
        }
        if answers.iter().take(DEPTH).any(hit) {
            e.2 += 1;
        }
    }
    (out, errors)
}

// ---------------------------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------------------------

#[allow(clippy::too_many_lines, reason = "the report, section by section")]
fn run() -> Result<(), String> {
    let a = args()?;
    let cc = a.cc.as_str();
    let build = |p: &Path| {
        let n = p.file_name().map(|n| n.to_string_lossy().into_owned());
        if n.as_deref().is_some_and(|n| n.contains("-lite")) {
            "lite"
        } else {
            "main"
        }
    };
    let reg_path = a.registry.as_deref().ok_or(USAGE)?;
    let serials = registry(reg_path, build(reg_path))?;
    let ns = <[u8; 2]>::try_from(cc.as_bytes())
        .ok()
        .and_then(Namespace::from_country)
        .ok_or_else(|| format!("{cc}: not a two-letter country code"))?;

    let mut engine = Engine::new();
    let mut opened = Vec::new();
    for p in &a.packs {
        let t = Instant::now();
        let layer =
            Layer::open(p, LayerOptions::default()).map_err(|e| format!("{}: {e}", p.display()))?;
        let bytes = std::fs::metadata(p).map_or(0, |m| m.len());
        opened.push(format!(
            "`{}` ({} {}, {bytes} B, opened in {:.1} ms)",
            p.file_name().unwrap_or_default().to_string_lossy(),
            layer.address(),
            layer.release(),
            t.elapsed().as_secs_f64() * 1000.0
        ));
        engine.add(layer);
    }
    let point_of = |key: &str| -> Option<LatLon> {
        let id = PlaceId::new(ns, *serials.get(key)?)?;
        engine.get(id).ok().flatten().map(|p| deg(p.point))
    };

    let golden = a.root.join("tests/golden");
    let set = labelled(&golden.join(format!("{cc}-places.csv")))?;
    let gtext = std::fs::read_to_string(golden.join(format!("{cc}.yaml")))
        .map_err(|e| format!("{cc}.yaml: {e}"))?;
    let gfile: GoldenFile =
        serde_saphyr::from_str(&gtext).map_err(|e| format!("{cc}.yaml: {e}"))?;
    let mut unresolved = 0_usize;
    let expected: Vec<Expected> = gfile
        .queries
        .iter()
        .filter_map(|q| {
            let points: Vec<(LatLon, f64)> = q
                .expect
                .iter()
                .filter(|&(_, &g)| g >= 2)
                .filter_map(|(k, _)| {
                    let r = if k.starts_with("codab:") {
                        HIT_UNIT_M
                    } else {
                        HIT_M
                    };
                    Some((point_of(k)?, r))
                })
                .collect();
            if points.is_empty() {
                unresolved += 1;
                return None;
            }
            Some(Expected {
                class: q.class.clone(),
                query: q.query.clone(),
                points,
            })
        })
        .collect();
    let gn = a.geonames.as_deref().map(geonames).transpose()?;

    let mut rivals: Vec<Box<dyn Rival>> = Vec::new();
    let mut own = Engine::new();
    for p in &a.packs {
        own.add(Layer::open(p, LayerOptions::default()).map_err(|e| e.to_string())?);
    }
    rivals.push(Box::new(Local {
        engine: own,
        ns,
        s: Scratch::default(),
        hits: Vec::new(),
        near: Vec::new(),
    }));
    if let Some(u) = &a.tgd {
        rivals.push(Box::new(Http::new(
            "teigeodata (`tgd serve`)",
            u,
            Api::Nominatim,
            cc,
        )));
    }
    if let Some(u) = &a.nominatim {
        rivals.push(Box::new(Http::new("Nominatim", u, Api::Nominatim, cc)));
    }
    if let Some(u) = &a.photon {
        rivals.push(Box::new(Http::new("Photon", u, Api::Photon, cc)));
    }

    let mut times: Vec<Times> = rivals.iter().map(|_| Times::default()).collect();
    let mut found = Vec::new();
    let mut recalls = Vec::new();
    for (r, t) in rivals.iter_mut().zip(times.iter_mut()) {
        eprintln!("competitive: asking {}", r.name());
        // Warm up: the first calls of a service fill its caches.
        for p in set.iter().take(20) {
            let _ = r.search(&p.name, 1);
        }
        found.push(places(r.as_mut(), &set, t));
        recalls.push(recall(r.as_mut(), &expected, t));
        for p in &set {
            let _ = t.time("reverse", || r.reverse(p.label));
        }
    }

    // By identity: GeoNames' own point, and teigeodata's record of the same place.
    let mut identity = Vec::new();
    let mut ours = Vec::new();
    let mut theirs = Vec::new();
    for p in &set {
        if let Some(q) = point_of(&p.id) {
            ours.push(m(q, p.label));
        }
        if let Some(g) = gn.as_ref().and_then(|g| g.get(&p.id)) {
            theirs.push(m(*g, p.label));
        }
    }
    identity.push(("teigeodata (by ID)", ours));
    if gn.is_some() {
        identity.push(("GeoNames (its own point)", theirs));
    }

    // Timezones: seeded random points, uniform on the sphere.
    let finder = tzf_rs::DefaultFinder::new();
    let (mut asked, mut answered, mut agree) = (0_usize, 0_usize, 0_usize);
    let (mut t_ours, mut t_tzf) = (Vec::new(), Vec::new());
    let mut disagree: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut rng = 0x9E37_79B9_7F4A_7C15_u64;
    let mut buf = [0_u8; 256];
    for _ in 0..a.tz_points {
        let u = next(&mut rng);
        let v = next(&mut rng);
        let lat = (2.0 * u - 1.0).asin().to_degrees();
        let lon = v.mul_add(360.0, -180.0);
        let Ok(at) = Point::<E7>::from_degrees(lat, lon) else {
            continue;
        };
        asked += 1;
        let t = Instant::now();
        let mine = match engine.tz_at(at) {
            // The zone rules name it where a layer has them; a timezone area is named by its
            // tzdb name either way.
            Ok(Some(area)) => match engine.zone_name(&area) {
                Ok(Some(z)) => Some(z.to_owned()),
                _ => engine
                    .name(&area, &mut buf)
                    .map_err(|e| format!("name at {lat}, {lon}: {e}"))?
                    .map(str::to_owned),
            },
            Ok(None) => None,
            Err(e) => return Err(format!("tz_at {lat}, {lon}: {e}")),
        };
        t_ours.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        let other = finder.get_tz_name(lon, lat).to_owned();
        t_tzf.push(t.elapsed().as_secs_f64() * 1000.0);
        if let Some(mine) = mine {
            answered += 1;
            if mine == other {
                agree += 1;
            } else {
                *disagree.entry((mine, other)).or_default() += 1;
            }
        }
    }

    // ------------------------------------------------------------------------------------
    // The report
    // ------------------------------------------------------------------------------------
    let mut o = String::new();
    let w = &mut o;
    let _ = writeln!(w, "# Competitive report: {}", cc.to_ascii_uppercase());
    let _ = writeln!(
        w,
        "\nSuite C ([method](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/competitive-suite.md)): \
         teigeodata and its rivals asked the same questions from the repository's files. Not a \
         gate. Each rival's version and configuration are in the footprint table; corrections \
         from their maintainers are welcome.\n"
    );
    let _ = writeln!(w, "- teigeodata packs: {}", opened.join("; "));
    let _ = writeln!(
        w,
        "- Rivals asked: {}",
        rivals
            .iter()
            .map(|r| r.name())
            .collect::<Vec<_>>()
            .join(", ")
    );
    if let Ok(cpu) = std::fs::read_to_string("/proc/cpuinfo")
        && let Some(model) = cpu.lines().find(|l| l.starts_with("model name"))
    {
        let n = cpu.lines().filter(|l| l.starts_with("processor")).count();
        let _ = writeln!(
            w,
            "- Machine: {} × {n}",
            model.split(':').nth(1).unwrap_or("").trim()
        );
    }

    let _ = writeln!(w, "\n## 1. Where is the place?\n");
    let _ = writeln!(
        w,
        "{} places of `tests/golden/{cc}-places.csv` marked on imagery by a labeller blind to \
         every source (D-209). Asked **by name**; the first answer counts as found when within \
         {} of the mark, and the distances are over the found.\n",
        set.len(),
        km(FOUND_M)
    );
    let _ = writeln!(w, "| Asked by name | Found | P50 | P90 | Errors |");
    let _ = writeln!(w, "|---|--:|--:|--:|--:|");
    for (r, f) in rivals.iter().zip(found.iter_mut()) {
        let _ = writeln!(
            w,
            "| {} | {} ({}) | {} | {} | {} |",
            r.name(),
            f.found,
            pct(f.found, f.asked),
            km(quantile(&mut f.distances, 0.5)),
            km(quantile(&mut f.distances, 0.9)),
            f.errors
        );
    }
    let _ = writeln!(
        w,
        "\nAsked **by identity** (the source record of the same place): the floor a name search \
         cannot beat, and what a library built on the source returns.\n"
    );
    let _ = writeln!(w, "| By identity | Places | P50 | P90 | Within 500 m |");
    let _ = writeln!(w, "|---|--:|--:|--:|--:|");
    for (name, mut d) in identity {
        let within = d.iter().filter(|&&x| x <= 500.0).count();
        let _ = writeln!(
            w,
            "| {name} | {} | {} | {} | {} |",
            d.len(),
            km(quantile(&mut d, 0.5)),
            km(quantile(&mut d, 0.9)),
            pct(within, d.len())
        );
    }
    let classes: BTreeMap<&str, usize> = set.iter().fold(BTreeMap::new(), |mut acc, p| {
        *acc.entry(p.class.as_str()).or_default() += 1;
        acc
    });
    let _ = writeln!(
        w,
        "\nThe set: {}.",
        classes
            .iter()
            .map(|(c, n)| format!("{n} {c}s"))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let _ = writeln!(w, "\n## 2. Search recall\n");
    let _ = writeln!(
        w,
        "{} queries of `tests/golden/{cc}.yaml` ({unresolved} left out: no expected place in \
         these packs). A hit is an answer within {} of an expected place, {} of an expected \
         admin unit. Recall@1 / recall@{DEPTH}:\n",
        expected.len(),
        km(HIT_M),
        km(HIT_UNIT_M)
    );
    let mut all_classes: Vec<&String> = recalls.iter().flat_map(|(r, _)| r.keys()).collect();
    all_classes.sort();
    all_classes.dedup();
    let _ = write!(w, "| Class | Queries |");
    for r in &rivals {
        let _ = write!(w, " {} |", r.name());
    }
    let _ = writeln!(w);
    let _ = writeln!(w, "|---|--:|{}", "--:|".repeat(rivals.len()));
    let mut totals = vec![(0, 0, 0); rivals.len()];
    for c in &all_classes {
        let n = recalls
            .first()
            .and_then(|(r, _)| r.get(*c))
            .map_or(0, |e| e.0);
        let _ = write!(w, "| {c} | {n} |");
        for (i, (r, _)) in recalls.iter().enumerate() {
            let e = r.get(*c).copied().unwrap_or_default();
            totals[i].0 += e.0;
            totals[i].1 += e.1;
            totals[i].2 += e.2;
            let _ = write!(w, " {} / {} |", pct(e.1, e.0), pct(e.2, e.0));
        }
        let _ = writeln!(w);
    }
    let _ = write!(w, "| **all** | {} |", totals.first().map_or(0, |t| t.0));
    for t in &totals {
        let _ = write!(w, " **{} / {}** |", pct(t.1, t.0), pct(t.2, t.0));
    }
    let _ = writeln!(w);
    let errs: Vec<String> = rivals
        .iter()
        .zip(&recalls)
        .filter(|(_, (_, e))| *e > 0)
        .map(|(r, (_, e))| format!("{} {e}", r.name()))
        .collect();
    if !errs.is_empty() {
        let _ = writeln!(
            w,
            "\nCalls that failed (counted as misses): {}.",
            errs.join(", ")
        );
    }

    let _ = writeln!(w, "\n## 3. Timezone at a point\n");
    let (o50, o95) = ms(&mut t_ours);
    let (z50, z95) = ms(&mut t_tzf);
    let _ = writeln!(
        w,
        "{asked} seeded random points, uniform on the sphere. teigeodata answered {answered} \
         (the rest are at sea or outside every pack's zones); `tzf-rs` {} answers every point \
         (at sea with `Etc/GMT±N`). Agreement over the points teigeodata answered: **{}** \
         ({agree} of {answered}). A layer without a timezone index names the zone of the \
         deepest admin unit holding the point (D-151, D-245): exact inside a unit with one \
         zone, the unit's main zone where a zone border crosses it.\n",
        TZF_VERSION,
        pct(agree, answered)
    );
    let _ = writeln!(w, "| Per call | Median | P95 |");
    let _ = writeln!(w, "|---|--:|--:|");
    let _ = writeln!(w, "| teigeodata `tz_at` + `zone_name` | {o50} | {o95} |");
    let _ = writeln!(w, "| `tzf-rs` `get_tz_name` | {z50} | {z95} |");
    if !disagree.is_empty() {
        let mut d: Vec<_> = disagree.into_iter().collect();
        d.sort_by_key(|e| std::cmp::Reverse(e.1));
        let _ = writeln!(
            w,
            "\nThe commonest disagreements (teigeodata → `tzf-rs`):\n"
        );
        let _ = writeln!(w, "| teigeodata | `tzf-rs` | Points |");
        let _ = writeln!(w, "|---|---|--:|");
        for ((mine, other), n) in d.into_iter().take(10) {
            let _ = writeln!(w, "| `{mine}` | `{other}` | {n} |");
        }
    }

    let _ = writeln!(w, "\n## 4. Latency\n");
    let _ = writeln!(
        w,
        "Every call above, one client, after a warm-up of 20 searches. In process for the \
         library; over loopback HTTP for the services (teigeodata's own server included, so \
         the services compare like for like).\n"
    );
    let _ = writeln!(
        w,
        "| Rival | Search median | Search P95 | Reverse median | Reverse P95 |"
    );
    let _ = writeln!(w, "|---|--:|--:|--:|--:|");
    for (r, t) in rivals.iter().zip(times.iter_mut()) {
        let (s50, s95) = ms(t.0.get_mut("search").map_or(&mut [][..], |v| &mut v[..]));
        let (r50, r95) = ms(t.0.get_mut("reverse").map_or(&mut [][..], |v| &mut v[..]));
        let _ = writeln!(w, "| {} | {s50} | {s95} | {r50} | {r95} |", r.name());
    }

    let _ = writeln!(w, "\n## 5. Footprint\n");
    if let Some(f) = &a.footprint {
        let _ = writeln!(
            w,
            "| Rival | Version | Set-up | On disk | Memory | Set up from |"
        );
        let _ = writeln!(w, "|---|---|--:|--:|--:|---|");
        for row in footprint(f)? {
            let cell = |i: usize| row.get(i).map_or("–", String::as_str);
            let _ = writeln!(
                w,
                "| {} | {} | {} | {} | {} | {} |",
                cell(0),
                cell(1),
                seconds(cell(2)),
                bytes(cell(3)),
                bytes(cell(4)),
                cell(5)
            );
        }
    } else {
        let _ = writeln!(w, "No footprint table given (`--footprint`).");
    }

    match &a.out {
        Some(p) => std::fs::write(p, &o).map_err(|e| format!("{}: {e}", p.display()))?,
        None => print!("{o}"),
    }
    Ok(())
}

/// A uniform `f64` in `[0, 1)` (splitmix64).
fn next(s: &mut u64) -> f64 {
    *s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *s;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    #[allow(clippy::cast_precision_loss, reason = "53 bits, exact")]
    let x = (z >> 11) as f64 / (1_u64 << 53) as f64;
    x
}

fn seconds(s: &str) -> String {
    s.parse::<f64>().map_or_else(
        |_| s.to_owned(),
        |x| {
            if x < 1.0 {
                format!("{:.0} ms", x * 1000.0)
            } else if x < 100.0 {
                format!("{x:.1} s")
            } else {
                format!("{:.0} min", x / 60.0)
            }
        },
    )
}

#[allow(clippy::cast_precision_loss, reason = "sizes far below 2^52")]
fn bytes(s: &str) -> String {
    s.parse::<u64>().map_or_else(
        |_| s.to_owned(),
        |b| {
            if b >= 1 << 30 {
                format!("{:.1} GB", b as f64 / f64::from(1_u32 << 30))
            } else {
                format!("{:.1} MB", b as f64 / f64::from(1_u32 << 20))
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_each_api_shape() {
        let n: Value =
            serde_json::from_str(r#"[{"lat":"28.2096","lon":"83.9856","name":"Pokhara"}]"#)
                .unwrap();
        assert_eq!(nominatim_points(&n), vec![(28.2096, 83.9856)]);
        let none: Value = serde_json::from_str(r#"{"error":"Unable to geocode"}"#).unwrap();
        assert_eq!(nominatim_point(&none), None);
        let p: Value = serde_json::from_str(
            r#"{"type":"FeatureCollection","features":[{"geometry":{"type":"Point","coordinates":[83.9856,28.2096]}}]}"#,
        )
        .unwrap();
        assert_eq!(
            photon_points(&p),
            vec![(28.2096, 83.9856)],
            "GeoJSON is lon, lat"
        );
    }

    #[test]
    fn quantiles_take_the_nearest_rank() {
        let mut v = vec![5.0, 1.0, 4.0, 2.0, 3.0];
        assert!((quantile(&mut v, 0.5) - 3.0).abs() < f64::EPSILON);
        assert!((quantile(&mut v, 0.9) - 5.0).abs() < f64::EPSILON);
        assert!(quantile(&mut [], 0.5).is_nan());
    }

    #[test]
    fn random_points_are_on_the_globe() {
        let mut s = 1;
        for _ in 0..10_000 {
            let x = next(&mut s);
            assert!((0.0..1.0).contains(&x));
        }
    }

    #[test]
    fn retired_keys_follow_to_the_survivor() {
        let dir = std::env::temp_dir().join(format!("competitive-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("r.tsv");
        std::fs::write(
            &f,
            "1\t>2\tgeonames:1\n2\tlite\tgeonames:2\n3\tx\tgeonames:3\n",
        )
        .unwrap();
        let r = registry(&f, "lite").unwrap();
        assert_eq!(r.get("geonames:1"), Some(&2));
        assert_eq!(r.get("geonames:2"), Some(&2));
        assert_eq!(r.get("geonames:3"), None, "a tombstone has no live place");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
