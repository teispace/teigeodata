//! `cargo xtask load`: phase 6.4's load test of the servers as they ship
//! ([research](../../docs/02-research/notes/server-load.md), D-320).
//!
//! Builds `tgd` (release, default features), serves each pack on loopback, finds a place by
//! `--query`, checks that every route of the mix answers 200, then drives each with `oha` for
//! `--secs` seconds, twice, sampling the server's physical footprint. Round two is reported;
//! the footprint's growth from the end of round one to the end of round two is the leak
//! check. `--check <budget.toml>` compares against per-pack floors and ceilings.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde::Deserialize;
use serde_json::Value;

/// The connections search is driven at; every other route at the middle one.
const SEARCH_CONNECTIONS: [u32; 3] = [1, 16, 64];
const CONNECTIONS: u32 = 16;

/// What the command was asked.
struct Options {
    packs: Vec<PathBuf>,
    /// The place the mix is built around; else the budget's for the pack; else Kathmandu.
    query: Option<String>,
    secs: u32,
    tgd: Option<PathBuf>,
    check: Option<PathBuf>,
    /// Only the routes named (for an experiment; the budget check needs them all).
    only: Vec<String>,
}

fn options(args: &[String]) -> Result<Options, Vec<String>> {
    let mut o = Options {
        packs: Vec::new(),
        query: None,
        secs: 5,
        tgd: None,
        check: None,
        only: Vec::new(),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| vec![format!("{name} needs a value")])
        };
        match a.as_str() {
            "--query" => o.query = Some(value("--query")?),
            "--secs" => {
                o.secs = value("--secs")?
                    .parse()
                    .map_err(|_| vec!["--secs takes whole seconds".to_owned()])?;
            }
            "--tgd" => o.tgd = Some(value("--tgd")?.into()),
            "--check" => o.check = Some(value("--check")?.into()),
            "--only" => o.only.push(value("--only")?),
            p if !p.starts_with("--") => o.packs.push(p.into()),
            other => return Err(vec![format!("unknown option {other}")]),
        }
    }
    if o.check.is_some() && !o.only.is_empty() {
        return Err(vec![
            "--check measures every route: leave out --only".into(),
        ]);
    }
    if o.packs.is_empty() {
        return Err(vec![
            "usage: cargo xtask load <pack.tgd>... [--query <name>] [--secs <n>] [--tgd <path>] [--check <budget.toml>]"
                .into(),
        ]);
    }
    Ok(o)
}

/// One route of the mix: its name, the path and query, and a body to `POST`.
struct Route {
    name: &'static str,
    target: String,
    body: Option<String>,
}

/// One measured point.
struct Point {
    route: &'static str,
    connections: u32,
    rps: f64,
    p50_ms: f64,
    p99_ms: f64,
    requests: u64,
    failed: u64,
}

/// One pack's results.
struct Report {
    pack: String,
    points: Vec<Point>,
    idle_kb: u64,
    peak_kb: u64,
    growth_kb: i64,
}

/// A served `tgd`, stopped when dropped.
struct Server {
    child: Child,
    addr: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let o = options(args)?;
    let oha = output(Command::new("oha").arg("--version"))
        .map_err(|e| vec![format!("oha is needed (cargo install oha): {e}")])?;
    let tgd = match &o.tgd {
        Some(t) => t.clone(),
        None => build(root)?,
    };
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} ({}s per point, two rounds, round two reported); load average {}",
        oha.trim(),
        o.secs,
        load_average()
    );
    let budgets = o.check.as_deref().map(budgets).transpose()?;
    let mut reports = Vec::new();
    for pack in &o.packs {
        let query = o
            .query
            .as_deref()
            .or_else(|| {
                budgets
                    .as_ref()
                    .and_then(|b| b.packs.get(&stem(pack)))
                    .and_then(|b| b.query.as_deref())
            })
            .unwrap_or("Kathmandu");
        let r = measure(root, &tgd, pack, query, &o)
            .map_err(|e| vec![format!("{}: {e}", pack.display())])?;
        print_report(&mut out, &r);
        reports.push(r);
    }
    if let (Some(budget), Some(budgets)) = (&o.check, &budgets) {
        let problems = check(budgets, &reports);
        if !problems.is_empty() {
            eprintln!("{out}");
            return Err(problems);
        }
        let _ = writeln!(out, "within {}", budget.display());
    }
    Ok(out)
}

/// Builds `tgd` as it ships and returns its path.
fn build(root: &Path) -> Result<PathBuf, Vec<String>> {
    let status = Command::new(env!("CARGO"))
        .current_dir(root)
        .args(["build", "--release", "--locked", "-p", "teigeodata-cli"])
        .status()
        .map_err(|e| vec![format!("cargo build: {e}")])?;
    if !status.success() {
        return Err(vec!["cargo build -p teigeodata-cli failed".into()]);
    }
    Ok(root.join("target/release/tgd"))
}

fn measure(
    root: &Path,
    tgd: &Path,
    pack: &Path,
    query: &str,
    o: &Options,
) -> Result<Report, String> {
    let store = root.join("target/load/store");
    let _ = std::fs::remove_dir_all(&store);
    let server = serve(tgd, &store, pack)?;
    let mut routes = routes(&server.addr, query)?;
    if !o.only.is_empty() {
        routes.retain(|r| o.only.iter().any(|n| n == r.name));
    }
    // Every route answers before it is measured: a typo would otherwise measure a 400.
    for r in &routes {
        let code = status(&server.addr, r)?;
        if code != "200" {
            return Err(format!("{} answered {code}: {}", r.name, r.target));
        }
    }
    let pid = server.child.id();
    drive(
        &server.addr,
        routes.first().ok_or("no routes")?,
        CONNECTIONS,
        1,
    )?;
    let idle_kb = footprint_kb(pid)?;
    let mut peak_kb = idle_kb;
    let mut ends = [0_u64; 2];
    let mut points = Vec::new();
    for (round, end) in ends.iter_mut().enumerate() {
        for r in &routes {
            let conns: &[u32] = if r.name == "search" {
                &SEARCH_CONNECTIONS
            } else {
                &[CONNECTIONS]
            };
            for &c in conns {
                let p = drive(&server.addr, r, c, o.secs)?;
                peak_kb = peak_kb.max(footprint_kb(pid)?);
                if round == 1 {
                    points.push(p);
                }
            }
        }
        *end = footprint_kb(pid)?;
    }
    drop(server);
    let _ = std::fs::remove_dir_all(&store);
    let [first, second] = ends;
    Ok(Report {
        pack: stem(pack),
        points,
        idle_kb,
        peak_kb,
        growth_kb: i64::try_from(second).unwrap_or(i64::MAX)
            - i64::try_from(first).unwrap_or(i64::MAX),
    })
}

fn stem(pack: &Path) -> String {
    pack.file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned())
}

/// Starts `tgd serve` over `pack` on a free loopback port.
fn serve(tgd: &Path, store: &Path, pack: &Path) -> Result<Server, String> {
    let mut child = Command::new(tgd)
        .arg("--store")
        .arg(store)
        .args([
            "serve",
            "--listen",
            "127.0.0.1:0",
            "--follow",
            "0",
            "--pack",
        ])
        .arg(pack)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", tgd.display()))?;
    let mut line = String::new();
    let read = child
        .stdout
        .take()
        .map(|s| BufReader::new(s).read_line(&mut line));
    let addr = line
        .trim()
        .strip_prefix("serving on http://")
        .map(str::to_owned);
    if let (Some(Ok(_)), Some(addr)) = (read, addr) {
        return Ok(Server { child, addr });
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(format!("tgd serve did not start: {line:?}"))
}

/// The mix, around the first place `query` finds and its country.
fn routes(addr: &str, query: &str) -> Result<Vec<Route>, String> {
    let q: String = query
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect();
    let found = get(addr, &format!("/v1/search?q={q}&limit=1"))?;
    let place = found
        .get("results")
        .and_then(|r| r.get(0))
        .ok_or_else(|| format!("{query:?} is not in the pack (--query names a place it holds)"))?;
    let id = place
        .get("id")
        .and_then(Value::as_str)
        .ok_or("no id")?
        .to_owned();
    let lat = place.get("lat").and_then(Value::as_f64).ok_or("no lat")?;
    let lon = place.get("lon").and_then(Value::as_f64).ok_or("no lon")?;
    // The country: the last ancestor, or the place itself at the top.
    let up = get(addr, &format!("/v1/hierarchy?id={id}"))?;
    let country = up
        .get("results")
        .and_then(Value::as_array)
        .and_then(|a| a.last())
        .and_then(|c| c.get("id"))
        .and_then(Value::as_str)
        .unwrap_or(&id)
        .to_owned();
    let mcp = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "search", "arguments": { "query": query, "limit": 10 } }
    });
    let at = |name, target: String| Route {
        name,
        target,
        body: None,
    };
    Ok(vec![
        at("search", format!("/v1/search?q={q}&limit=10")),
        at(
            "reverse",
            format!("/v1/reverse?lat={lat}&lon={lon}&limit=10"),
        ),
        at("get", format!("/v1/get?id={id}")),
        at("near", format!("/v1/near?id={id}&limit=10")),
        at("hierarchy", format!("/v1/hierarchy?id={id}")),
        at("boundary", format!("/v1/boundary?id={country}")),
        at("nominatim", format!("/search?q={q}&format=jsonv2&limit=10")),
        Route {
            name: "mcp search",
            target: "/mcp".into(),
            body: Some(mcp.to_string()),
        },
    ])
}

const MCP_HEADERS: [&str; 3] = [
    "content-type: application/json",
    "accept: application/json, text/event-stream",
    "mcp-protocol-version: 2025-11-25",
];

fn get(addr: &str, target: &str) -> Result<Value, String> {
    let body = output(Command::new("curl").args(["-sf", &format!("http://{addr}{target}")]))?;
    serde_json::from_str(&body).map_err(|e| format!("{target}: {e}"))
}

/// The HTTP status of one request on `r`.
fn status(addr: &str, r: &Route) -> Result<String, String> {
    let mut c = Command::new("curl");
    c.args(["-s", "-o", "/dev/null", "-w", "%{http_code}"]);
    if let Some(body) = &r.body {
        c.args(["-X", "POST", "-d", body]);
        for h in MCP_HEADERS {
            c.args(["-H", h]);
        }
    }
    c.arg(format!("http://{addr}{}", r.target));
    output(&mut c)
}

/// `oha` on `r` with `connections` for `secs` seconds.
fn drive(addr: &str, r: &Route, connections: u32, secs: u32) -> Result<Point, String> {
    let mut c = Command::new("oha");
    c.args(["--no-tui", "--output-format", "json", "-z"])
        .arg(format!("{secs}s"))
        .arg("-c")
        .arg(connections.to_string());
    if let Some(body) = &r.body {
        c.args(["-m", "POST", "-d", body]);
        for h in MCP_HEADERS {
            c.args(["-H", h]);
        }
    }
    c.arg(format!("http://{addr}{}", r.target));
    let v: Value = serde_json::from_str(&output(&mut c)?).map_err(|e| format!("oha: {e}"))?;
    let num = |p: &[&str]| {
        p.iter()
            .try_fold(&v, |v, k| v.get(k))
            .and_then(Value::as_f64)
    };
    let codes = v.get("statusCodeDistribution").and_then(Value::as_object);
    let ok = codes
        .and_then(|c| c.get("200"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let all: u64 = codes.map_or(0, |c| c.values().filter_map(Value::as_u64).sum());
    // A request cut by the deadline is the run ending, not a failure.
    let errors: u64 = v
        .get("errorDistribution")
        .and_then(Value::as_object)
        .map_or(0, |e| {
            e.iter()
                .filter(|(k, _)| !k.contains("deadline"))
                .filter_map(|(_, n)| n.as_u64())
                .sum()
        });
    Ok(Point {
        route: r.name,
        connections,
        rps: num(&["summary", "requestsPerSec"]).unwrap_or(0.0),
        p50_ms: num(&["latencyPercentiles", "p50"]).unwrap_or(f64::INFINITY) * 1e3,
        p99_ms: num(&["latencyPercentiles", "p99"]).unwrap_or(f64::INFINITY) * 1e3,
        requests: all,
        failed: all - ok + errors,
    })
}

/// The process's physical footprint in KB: what the system charges it, compressed and
/// swapped pages included (macOS `footprint`; Linux `VmRSS`).
fn footprint_kb(pid: u32) -> Result<u64, String> {
    if cfg!(target_os = "linux") {
        let s =
            std::fs::read_to_string(format!("/proc/{pid}/status")).map_err(|e| e.to_string())?;
        return s
            .lines()
            .find_map(|l| l.strip_prefix("VmRSS:"))
            .and_then(|v| v.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .ok_or_else(|| "no VmRSS".into());
    }
    let s = output(Command::new("footprint").args(["-p", &pid.to_string()]))?;
    let (_, after) = s.split_once("Footprint: ").ok_or("no footprint")?;
    let mut words = after.split_whitespace();
    let n: f64 = words
        .next()
        .and_then(|n| n.parse().ok())
        .ok_or("no footprint")?;
    let scale = match words.next() {
        Some("B") => 1.0 / 1024.0,
        Some("KB") => 1.0,
        Some("MB") => 1024.0,
        Some("GB") => 1024.0 * 1024.0,
        other => return Err(format!("footprint unit {other:?}")),
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a positive size in KB"
    )]
    Ok((n * scale) as u64)
}

fn load_average() -> String {
    output(&mut Command::new("uptime")).map_or_else(
        |_| "unknown".into(),
        |u| {
            u.rsplit_once("averages:")
                .or_else(|| u.rsplit_once("average:"))
                .map_or_else(|| u.trim().to_owned(), |(_, a)| a.trim().to_owned())
        },
    )
}

fn output(c: &mut Command) -> Result<String, String> {
    let o = c.output().map_err(|e| format!("{c:?}: {e}"))?;
    if !o.status.success() {
        return Err(format!(
            "{c:?}: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    String::from_utf8(o.stdout).map_err(|e| e.to_string())
}

fn print_report(out: &mut String, r: &Report) {
    let _ = writeln!(out, "\n{}\n", r.pack);
    let _ = writeln!(
        out,
        "| Route | c | req/s | p50 ms | p99 ms | requests | failed |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|");
    for p in &r.points {
        let _ = writeln!(
            out,
            "| {} | {} | {:.0} | {:.3} | {:.3} | {} | {} |",
            p.route, p.connections, p.rps, p.p50_ms, p.p99_ms, p.requests, p.failed
        );
    }
    let _ = writeln!(
        out,
        "\nfootprint: {} KB idle, {} KB peak, {:+} KB across round two",
        r.idle_kb, r.peak_kb, r.growth_kb
    );
}

/// A pack's budget: footprint ceilings and each point's floor and ceiling.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Budget {
    /// The place the mix is built around, when the pack does not hold Kathmandu.
    query: Option<String>,
    footprint_mb: u64,
    growth_mb: u64,
    /// Keyed `"<route> c=<connections>"`.
    points: BTreeMap<String, Bounds>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bounds {
    min_rps: f64,
    max_p99_ms: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Budgets {
    packs: BTreeMap<String, Budget>,
}

fn budgets(path: &Path) -> Result<Budgets, Vec<String>> {
    let text =
        std::fs::read_to_string(path).map_err(|e| vec![format!("{}: {e}", path.display())])?;
    toml::from_str(&text).map_err(|e| vec![format!("{}: {e}", path.display())])
}

fn check(budgets: &Budgets, reports: &[Report]) -> Vec<String> {
    let mut problems = Vec::new();
    for r in reports {
        let Some(b) = budgets.packs.get(&r.pack) else {
            problems.push(format!("{}: no budget for this pack", r.pack));
            continue;
        };
        if r.peak_kb > b.footprint_mb * 1024 {
            problems.push(format!(
                "{}: footprint {} KB over {} MB",
                r.pack, r.peak_kb, b.footprint_mb
            ));
        }
        if r.growth_kb > i64::try_from(b.growth_mb * 1024).unwrap_or(i64::MAX) {
            problems.push(format!(
                "{}: footprint grew {} KB across a round, over {} MB",
                r.pack, r.growth_kb, b.growth_mb
            ));
        }
        for p in &r.points {
            let key = format!("{} c={}", p.route, p.connections);
            if p.failed > 0 {
                problems.push(format!(
                    "{}: {key}: {} of {} failed",
                    r.pack, p.failed, p.requests
                ));
            }
            let Some(bounds) = b.points.get(&key) else {
                problems.push(format!("{}: {key}: no budget", r.pack));
                continue;
            };
            if p.rps < bounds.min_rps {
                problems.push(format!(
                    "{}: {key}: {:.0} req/s under {:.0}",
                    r.pack, p.rps, bounds.min_rps
                ));
            }
            if p.p99_ms > bounds.max_p99_ms {
                problems.push(format!(
                    "{}: {key}: p99 {:.3} ms over {:.3}",
                    r.pack, p.p99_ms, bounds.max_p99_ms
                ));
            }
        }
    }
    problems
}
