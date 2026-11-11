//! `tgd`'s store commands end to end: a signed repository, delegated by country as a release
//! writes it, served over HTTP on localhost with range requests, and the binary driving a store
//! from `init` to `remove`: refresh, catalog, install by component, queries over the store,
//! update to a new release, pin, repair of a damaged file, status, and the network's exit code.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "tests: failures panic with their message; one story per test"
)]

#[path = "../../teigeodata-engine/tests/common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use ed25519_dalek::SigningKey;
use serde_json::Value;
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer};
use teigeodata_sync::repo::{
    self, Files, RoleKeys, TargetFile, country_role, sign, snapshot_listing, targets,
    targets_delegating, timestamp,
};

const YEAR: i64 = 365 * 86_400;

fn now() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap()
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn one(seed: u8) -> RoleKeys {
    RoleKeys {
        keys: vec![key(seed).verifying_key().to_bytes()],
        threshold: 1,
    }
}

/// Root keys 1 (threshold 1), timestamp 10, snapshot 20, targets 30.
fn root_file() -> Vec<u8> {
    let roles = [one(1), one(10), one(20), one(30)];
    sign(&repo::root(1, now() + YEAR, &roles).unwrap(), &[&key(1)]).unwrap()
}

/// The synthetic Nepal pack at `release`, with a licence and attribution; `shift` moves one
/// place so releases differ.
fn pack(release: &str, shift: bool) -> Vec<u8> {
    pack_of(700, release, shift)
}

/// [`pack`] with `n` places.
fn pack_of(n: usize, release: &str, shift: bool) -> Vec<u8> {
    let mut model = common::synthetic(n, 3);
    if shift {
        let p = model.points[5];
        model.points[5] =
            teigeodata_core::Point::from_degrees(p.lat_degrees() + 0.01, p.lon_degrees()).unwrap();
    }
    let bytes = model.encode();
    let p = Pack::open(&bytes).unwrap();
    let sections: Vec<(Kind, Encoding, u64, &[u8])> = p
        .sections()
        .filter(|s| s.kind() != Kind::BLOCK_HASHES)
        .map(|s| (s.kind(), s.encoding(), s.item_count(), s.bytes()))
        .collect();
    let mut w = Writer::new("np", "places", release).unwrap();
    w.manifest(Tag::LICENCE, b"odbl").unwrap();
    w.manifest(Tag::ATTRIBUTION, "© OpenStreetMap contributors".as_bytes())
        .unwrap();
    if let Some(b) = p.manifest().get(Tag::BBOX) {
        w.manifest(Tag::BBOX, b).unwrap();
    }
    w.pack(&sections, None).unwrap()
}

/// The files a host serves, by path (`meta/…`, `assets/…`).
type Host = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

/// Release `v` of `packs` on `host`: assets, Nepal's role, top-level targets, snapshot and
/// timestamp, as `tuf release` lays them out (consistent snapshots).
fn publish(host: &Host, v: u64, packs: &[Vec<u8>]) {
    let mut h = host.lock().unwrap();
    let mut files: Files = BTreeMap::new();
    for p in packs {
        let asset =
            teigeodata_sync::transport::encode(p, |d| zstd::bulk::compress(d, 3).ok()).unwrap();
        let entry = teigeodata_sync::measure::entry(p, &asset).unwrap();
        files.insert(
            entry.path(),
            TargetFile {
                length: asset.len() as u64,
                hashes: repo::target_hashes(&asset),
                custom: Some(entry.to_custom()),
            },
        );
        h.insert(format!("assets/{}", entry.file), asset);
    }
    let (top, countries) = repo::by_country(files);
    let delegated: Vec<_> = countries
        .keys()
        .map(|&cc| country_role(cc, one(30)))
        .collect();
    let expires = now() + YEAR;
    let tg = sign(
        &targets_delegating(v, expires, &top, &delegated).unwrap(),
        &[&key(30)],
    )
    .unwrap();
    let mut listed: Vec<(String, Vec<u8>)> = vec![("targets".into(), tg.clone())];
    for (cc, f) in &countries {
        let name = String::from_utf8(cc.to_vec()).unwrap();
        let b = sign(&targets(v, expires, f).unwrap(), &[&key(30)]).unwrap();
        h.insert(format!("meta/{v}.{name}.json"), b.clone());
        listed.push((name, b));
    }
    let refs: Vec<(&str, &[u8], u64)> = listed
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice(), v))
        .collect();
    let sn = sign(&snapshot_listing(v, expires, &refs).unwrap(), &[&key(20)]).unwrap();
    let ts = sign(
        &timestamp(v, now() + 14 * 86_400, &sn, v).unwrap(),
        &[&key(10)],
    )
    .unwrap();
    h.insert(format!("meta/{v}.targets.json"), tg);
    h.insert(format!("meta/{v}.snapshot.json"), sn);
    h.insert("meta/timestamp.json".into(), ts);
}

/// Serves `host` on localhost: GET, whole or one `bytes=a-b` range, `ETag` by path, 404 for
/// anything else. Returns the base URL.
fn serve(host: Host) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut c) = conn else { continue };
            let mut reader = BufReader::new(c.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let path = line
                .split(' ')
                .nth(1)
                .unwrap_or("/")
                .trim_start_matches('/')
                .to_string();
            let mut range = None;
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                    break;
                }
                let lower = h.to_ascii_lowercase();
                if let Some(r) = lower.strip_prefix("range: bytes=") {
                    let (a, b) = r.trim().split_once('-').unwrap();
                    range = Some((a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap()));
                }
            }
            let body = host.lock().unwrap().get(&path).cloned();
            let reply = match (body, range) {
                (None, _) => {
                    b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                        .to_vec()
                }
                (Some(b), None) => {
                    let mut r = format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\netag: \"{path}\"\r\nconnection: close\r\n\r\n", b.len()).into_bytes();
                    r.extend_from_slice(&b);
                    r
                }
                (Some(b), Some((x, y))) => {
                    let y = y.min(b.len() - 1);
                    let part = &b[x..=y];
                    let mut r = format!(
                        "HTTP/1.1 206 Partial Content\r\ncontent-length: {}\r\ncontent-range: bytes {x}-{y}/{}\r\netag: \"{path}\"\r\nconnection: close\r\n\r\n",
                        part.len(),
                        b.len()
                    )
                    .into_bytes();
                    r.extend_from_slice(part);
                    r
                }
            };
            let _ = c.write_all(&reply);
            let _ = c.flush();
            let mut sink = [0_u8; 1];
            let _ = c.set_read_timeout(Some(std::time::Duration::from_millis(5)));
            let _ = c.read(&mut sink);
        }
    });
    format!("http://{addr}")
}

fn dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("sync-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn tgd(store: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tgd"))
        .arg("--store")
        .arg(store)
        .args(args)
        .env_remove("TEIGEODATA_STORE")
        .output()
        .unwrap()
}

fn ok(store: &Path, args: &[&str]) -> String {
    let o = tgd(store, args);
    assert!(
        o.status.success(),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout).unwrap()
}

fn json(store: &Path, args: &[&str]) -> Value {
    serde_json::from_str(&ok(store, args)).unwrap()
}

/// A store set up against `url`.
fn store_for(name: &str, url: &str) -> PathBuf {
    let d = dir(name);
    let root = d.join("root.json");
    std::fs::write(&root, root_file()).unwrap();
    let store = d.join("store");
    let (meta, assets) = (format!("{url}/meta"), format!("{url}/assets/"));
    ok(
        &store,
        &[
            "init",
            "--repo",
            &meta,
            "--assets",
            &assets,
            "--root",
            root.to_str().unwrap(),
        ],
    );
    store
}

#[test]
fn a_store_from_init_to_remove() {
    let host: Host = Arc::default();
    publish(&host, 1, &[pack("2026.11.0", false)]);
    let url = serve(Arc::clone(&host));
    let store = store_for("lifecycle", &url);

    // Before a refresh the catalog is empty; a refresh for Nepal lists its pack.
    let empty = json(&store, &["catalog", "--json"]);
    assert_eq!(empty, serde_json::json!([]));
    let r = ok(&store, &["refresh", "--countries", "np"]);
    assert!(r.contains("timestamp 1"), "{r}");
    let c = json(&store, &["catalog", "--json"]);
    assert_eq!(c[0]["path"], "np.places.tgd");
    assert_eq!(c[0]["state"], "available");
    // A second refresh fetches the timestamp alone.
    assert!(ok(&store, &["refresh"]).contains("current"));

    // Install search alone (with what it needs): queries over the store answer by name; elevation is not there.
    let out = ok(&store, &["install", "np", "--components", "search"]);
    assert!(out.contains("bytes fetched"), "{out}");
    let s = json(&store, &["status", "--json", "--deep"]);
    let comps: Vec<&str> = s["packs"][0]["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(comps, ["point", "names", "search"]);
    assert_eq!(s["packs"][0]["health"], "Good");
    let hit = json(&store, &["search", "Ritarra", "-k", "1", "--json"]);
    assert_eq!(hit[0]["name"], "Ritarra");
    // What is not installed is null, not an error; a query that needs it is refused.
    let part = json(&store, &["get", hit[0]["id"].as_str().unwrap(), "--json"]);
    assert_eq!(part[0]["name"], "Ritarra");
    for absent in ["elevation", "parent", "timezone"] {
        assert!(part[0][absent].is_null(), "{absent}: {part}");
    }
    let lat = hit[0]["lat"].as_f64().unwrap().to_string();
    let lon = hit[0]["lon"].as_f64().unwrap().to_string();
    assert_eq!(tgd(&store, &["tz", &lat, &lon]).status.code(), Some(1));

    // Everything: the same release, more components; the queries now answer whole.
    ok(&store, &["install", "np"]);
    let place = json(&store, &["get", hit[0]["id"].as_str().unwrap(), "--json"]);
    assert!(place[0]["elevation"].is_number(), "{place}");
    assert!(ok(&store, &["install", "np"]).contains("is installed"));
    assert_eq!(
        json(&store, &["catalog", "--json"])[0]["state"],
        "installed"
    );
    let found = json(&store, &["find", "Ritarra", "--json"]);
    assert_eq!(found[0]["packs"][0]["state"], "installed");

    // A new release: update fetches it, keeping what did not change.
    assert!(ok(&store, &["update"]).contains("up to date"));
    publish(&host, 2, &[pack("2026.12.0", true)]);
    let u = ok(&store, &["update"]);
    assert!(
        u.contains("np.places.tgd 2026.12.0") && u.contains("reused"),
        "{u}"
    );
    let s = json(&store, &["status", "--json"]);
    assert_eq!(s["packs"][0]["release"], "2026.12.0");
    assert_eq!(s["catalog"]["timestamp"], 2);

    // Pin; damage the file and repair it.
    ok(&store, &["pin", "np"]);
    let file = std::fs::read_dir(store.join("packs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut bytes = std::fs::read(&file).unwrap();
    let at = bytes.len() / 2;
    bytes[at] ^= 0xff;
    std::fs::write(&file, &bytes).unwrap();
    let bad = tgd(&store, &["status", "--deep"]);
    assert_eq!(
        bad.status.code(),
        Some(1),
        "a damaged pack fails status --deep"
    );
    let r = ok(&store, &["repair", "np"]);
    assert!(r.contains("repaired"), "{r}");
    let s = json(&store, &["status", "--json", "--deep"]);
    assert_eq!(s["packs"][0]["health"], "Good");
    assert_eq!(s["packs"][0]["pinned"], true);

    // Remove.
    assert!(ok(&store, &["remove", "np"]).contains("removed np.places.tgd"));
    assert_eq!(
        json(&store, &["status", "--json"])["packs"],
        serde_json::json!([])
    );
    assert_eq!(tgd(&store, &["remove", "np"]).status.code(), Some(1));
}

#[test]
fn the_network_and_the_trust_anchor_have_their_exit_codes() {
    // A mirror that is not there: exit 3; the store still works offline.
    let dead = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        drop(l);
        format!("http://{a}")
    };
    let store = store_for("offline", &dead);
    assert_eq!(
        tgd(&store, &["refresh", "--countries", "np"]).status.code(),
        Some(3)
    );
    ok(&store, &["catalog"]);
    ok(&store, &["status"]);
    // A store never set up is refused, and so is a root that does not verify.
    let none = dir("none").join("store");
    assert_eq!(tgd(&none, &["status"]).status.code(), Some(1));
    let d = dir("bad-root");
    std::fs::write(d.join("root.json"), b"{}").unwrap();
    let o = tgd(
        &d.join("store"),
        &[
            "init",
            "--repo",
            "x",
            "--assets",
            "y",
            "--root",
            d.join("root.json").to_str().unwrap(),
        ],
    );
    assert_eq!(o.status.code(), Some(1));
    // A repository whose timestamp is signed by another key: refused (1), not a network error.
    let host: Host = Arc::default();
    publish(&host, 1, &[pack("2026.11.0", false)]);
    {
        let mut h = host.lock().unwrap();
        let ts = h["meta/timestamp.json"].clone();
        let mut v: Value = serde_json::from_slice(&ts).unwrap();
        v["signed"]["version"] = 9.into();
        h.insert(
            "meta/timestamp.json".into(),
            serde_json::to_vec(&v).unwrap(),
        );
    }
    let url = serve(host);
    let store = store_for("lying", &url);
    assert_eq!(
        tgd(&store, &["refresh", "--countries", "np"]).status.code(),
        Some(1)
    );
}

#[test]
fn an_update_keeps_the_components_installed() {
    let host: Host = Arc::default();
    publish(&host, 1, &[pack("2026.11.0", false)]);
    let url = serve(Arc::clone(&host));
    let store = store_for("partial-update", &url);
    ok(&store, &["install", "np", "--components", "search"]);
    publish(&host, 2, &[pack("2026.12.0", true)]);
    assert!(ok(&store, &["update"]).contains("2026.12.0"));
    let s = json(&store, &["status", "--json", "--deep"]);
    assert_eq!(s["packs"][0]["release"], "2026.12.0");
    assert_eq!(
        s["packs"][0]["components"],
        serde_json::json!(["point", "names", "search"])
    );
    assert_eq!(s["packs"][0]["health"], "Good");
}

/// Timings of the binary `TGD_BIN` (default: this build's), the median of `TGD_RUNS` runs
/// (default 15) over a pack of 100,000 places, about Nepal's size: open and query, and an
/// install over HTTP on localhost. Run: `TGD_BIN=target/dist/tgd cargo test --release -p
/// teigeodata-cli --test sync -- --ignored timings --nocapture`.
#[test]
#[ignore = "a measurement: run by hand, see the doc comment"]
#[allow(clippy::print_stdout, reason = "a measurement prints its table")]
fn timings() {
    let bin = std::env::var("TGD_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_tgd").into());
    let runs: usize = std::env::var("TGD_RUNS").map_or(15, |r| r.parse().unwrap());
    let host: Host = Arc::default();
    let p = pack_of(100_000, "2026.11.0", false);
    publish(&host, 1, std::slice::from_ref(&p));
    let url = serve(Arc::clone(&host));
    let d = dir("timings");
    let file = d.join("np.tgd");
    std::fs::write(&file, &p).unwrap();
    let f = file.to_str().unwrap();
    let id = {
        let o = Command::new(&bin)
            .args(["search", "Ritarra", "-k", "1", "--json", "--pack", f])
            .output()
            .unwrap();
        let v: Value = serde_json::from_slice(&o.stdout).unwrap();
        v[0]["id"].as_str().unwrap().to_string()
    };
    let median = |args: &[&str], before: &dyn Fn()| {
        let mut t: Vec<f64> = (0..runs)
            .map(|_| {
                before();
                let start = std::time::Instant::now();
                let o = Command::new(&bin).args(args).output().unwrap();
                let ms = start.elapsed().as_secs_f64() * 1e3;
                assert!(
                    o.status.success(),
                    "{args:?}: {}",
                    String::from_utf8_lossy(&o.stderr)
                );
                ms
            })
            .collect();
        t.sort_by(f64::total_cmp);
        t[t.len() / 2]
    };
    let none = || {};
    println!("pack: {} bytes, {runs} runs, median ms", p.len());
    for (name, args) in [
        ("version (process start)", vec!["--version"]),
        ("inspect", vec!["inspect", f]),
        ("verify", vec!["verify", f]),
        ("verify --deep", vec!["verify", "--deep", f]),
        ("get", vec!["get", &id, "--pack", f]),
        ("reverse", vec!["reverse", "27.7", "85.3", "--pack", f]),
        ("search", vec!["search", "Ritarra", "--pack", f]),
        ("admin", vec!["admin", "27.7", "85.3", "--pack", f]),
    ] {
        println!("{name:>24}: {:8.2}", median(&args, &none));
    }
    let store = store_for("timings-store", &url);
    let st = store.to_str().unwrap();
    ok(&store, &["refresh", "--countries", "np"]);
    let remove = || {
        let _ = tgd(&store, &["remove", "np"]);
    };
    let install = median(&["--store", st, "install", "np"], &remove);
    println!("{:>24}: {install:8.2}", "install (whole, HTTP)");
    let names = median(
        &["--store", st, "install", "np", "--components", "search"],
        &remove,
    );
    println!("{:>24}: {names:8.2}", "install (search, HTTP)");
    ok(&store, &["install", "np"]);
    println!(
        "{:>24}: {:8.2}",
        "get (store)",
        median(&["--store", st, "get", &id], &none)
    );
    println!(
        "{:>24}: {:8.2}",
        "refresh (current)",
        median(&["--store", st, "refresh"], &none)
    );
    println!(
        "{:>24}: {:8.2}",
        "status --deep",
        median(&["--store", st, "status", "--deep"], &none)
    );
}

/// A platform's loop over `install --downloads` (D-184): exit 4 lists the ranges to fetch; the
/// "platform" fetches them (here from the host's files), and runs install again until it ends.
#[test]
fn a_platform_installs_through_downloads() {
    let host: Host = Arc::default();
    publish(&host, 1, &[pack("2026.11.0", false)]);
    let url = serve(Arc::clone(&host));
    let store = store_for("downloads", &url);
    ok(&store, &["refresh", "--countries", "np"]);
    let dl = store.parent().unwrap().join("dl");
    let dls = dl.to_str().unwrap();
    let mut rounds = 0;
    loop {
        rounds += 1;
        assert!(rounds <= 4, "at most three rounds of downloads");
        let o = tgd(&store, &["install", "np", "--downloads", dls]);
        match o.status.code() {
            Some(0) => break,
            Some(4) => {}
            c => panic!("{c:?}: {}", String::from_utf8_lossy(&o.stderr)),
        }
        let need: Value = serde_json::from_slice(&o.stdout).unwrap();
        for asset in need.as_array().unwrap() {
            let u = asset["urls"][0].as_str().unwrap();
            assert!(u.starts_with(&url), "{u}");
            let path = u.strip_prefix(&format!("{url}/")).unwrap();
            let bytes = host.lock().unwrap()[path].clone();
            let dir = PathBuf::from(asset["dir"].as_str().unwrap());
            std::fs::create_dir_all(&dir).unwrap();
            for r in asset["ranges"].as_array().unwrap() {
                let (a, b) = (r["start"].as_u64().unwrap(), r["end"].as_u64().unwrap());
                let part = &bytes[usize::try_from(a).unwrap()..usize::try_from(b).unwrap()];
                std::fs::write(dir.join(r["file"].as_str().unwrap()), part).unwrap();
            }
        }
    }
    assert_eq!(rounds, 3, "table and head, the frames, done");
    let s = json(&store, &["status", "--json", "--deep"]);
    assert_eq!(s["packs"][0]["health"], "Good");
    assert!(
        std::fs::read_dir(&dl).unwrap().next().is_none(),
        "the downloaded files are removed once committed"
    );
    assert!(ok(&store, &["search", "Ritarra", "-k", "1"]).contains("Ritarra"));
}

/// Writes a signed repository of a 100,000-place pack to `TGD_REPO_OUT`: `meta/`, `assets/` and
/// the trusted `root.json`, for the platform prototypes (`tools/platform-probe`) to serve.
/// Run: `TGD_REPO_OUT=DIR cargo test -p teigeodata-cli --test sync -- --ignored write_repo`.
#[test]
#[ignore = "writes a repository for the platform prototypes"]
fn write_repo() {
    let out = PathBuf::from(std::env::var("TGD_REPO_OUT").unwrap());
    let host: Host = Arc::default();
    publish(&host, 1, &[pack_of(100_000, "2026.11.0", false)]);
    for (path, bytes) in host.lock().unwrap().iter() {
        let p = out.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    std::fs::write(out.join("root.json"), root_file()).unwrap();
}
