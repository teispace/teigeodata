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
#[path = "../../teigeodata-sync/tests/support/repo.rs"]
mod repo_support;

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;

use serde_json::Value;
use teigeodata_format::{Encoding, Kind, Part, Tag, Writer};

use repo_support::{Host, pack, pack_of, publish, root_file, serve, serve_at};

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

/// A Nepal pack whose names and outlines are split into parts (ADR-0040), of incompressible
/// bodies so the wire bytes are the bodies': names base, `*` and `ne`; outlines whole, to
/// level 0 and to level 1.
fn parts_pack() -> Vec<u8> {
    let body = |seed: u64, n: usize| -> Vec<u8> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x = x
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                (x >> 56) as u8
            })
            .collect()
    };
    let (ids, places) = (body(1, 4_000), body(2, 3_000));
    let (base, rest, ne) = (body(3, 30_000), body(4, 20_000), body(5, 40_000));
    let (arcs, to_0, to_1) = (body(6, 200_000), body(7, 10_000), body(8, 50_000));
    let (ne_key, o0, o1) = (
        Part::language("ne").unwrap(),
        Part::outline(0, 0).unwrap(),
        Part::outline(1, 0).unwrap(),
    );
    let mut w = Writer::new("np", "places", "2026.11.0").unwrap();
    w.manifest(Tag::LICENCE, b"odbl").unwrap();
    w.pack_parts(
        &[
            (Kind::IDMAP, Part::BASE, Encoding::RAW, 1, &ids),
            (Kind::PLACES, Part::BASE, Encoding::RAW, 1, &places),
            (Kind::NAMES, Part::BASE, Encoding::RAW, 1, &base),
            (Kind::NAMES, Part::REST, Encoding::RAW, 1, &rest),
            (Kind::NAMES, ne_key, Encoding::RAW, 1, &ne),
            (Kind::ARCS, Part::BASE, Encoding::RAW, 1, &arcs),
            (Kind::ARCS, o0, Encoding::RAW, 1, &to_0),
            (Kind::ARCS, o1, Encoding::RAW, 1, &to_1),
        ],
        None,
    )
    .unwrap()
}

/// The bytes an install printed it fetched.
fn fetched(out: &str) -> u64 {
    let (_, rest) = out.split_once(": ").unwrap();
    rest.split(' ').next().unwrap().parse().unwrap()
}

/// `install --names --outlines` (D-253): the catalog lists the parts, an install takes the
/// languages asked (one without a part of its own comes with `*`) and the smallest outline
/// part that holds the level; a later install replaces the parts it names and keeps the rest.
#[test]
fn an_install_takes_the_languages_and_outlines_asked() {
    let host: Host = Arc::default();
    publish(&host, 1, &[parts_pack()]);
    let url = serve(Arc::clone(&host));
    let store = store_for("parts", &url);
    ok(&store, &["refresh", "--countries", "np"]);
    let c = json(&store, &["catalog", "--json"]);
    let parts = c[0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 4, "{parts:?}");
    let size = |pick: &dyn Fn(&Value) -> bool| {
        parts.iter().find(|p| pick(p)).unwrap()["download"]
            .as_u64()
            .unwrap()
    };
    let ne = size(&|p| p["language"] == "ne");
    let to_0 = size(&|p| p["level"] == 0);
    let whole = c[0]["download"].as_u64().unwrap();

    // English (no part of its own: `*`) and the outlines to level 1: not `ne`, not the
    // whole outlines, not the outlines to level 0.
    let out = ok(
        &store,
        &["install", "np", "--names", "en", "--outlines", "1"],
    );
    let got = fetched(&out);
    assert!(got + ne + to_0 + 200_000 <= whole, "{got} of {whole}");
    let s = json(&store, &["status", "--json", "--deep"]);
    let p = &s["packs"][0];
    assert_eq!(p["health"], "Good");
    assert_eq!(p["parts"]["languages"], serde_json::json!(["*"]));
    assert_eq!(
        p["parts"]["outline"],
        serde_json::json!({"level": 1, "lod": 0})
    );
    assert!(
        ok(
            &store,
            &["install", "np", "--names", "en", "--outlines", "1@0m"]
        )
        .contains("is installed")
    );

    // Nepali instead: the outline stays. Then level 0: its own, smaller part.
    ok(&store, &["install", "np", "--names", "ne"]);
    let s = json(&store, &["status", "--json"]);
    assert_eq!(
        s["packs"][0]["parts"]["languages"],
        serde_json::json!(["ne"])
    );
    assert_eq!(s["packs"][0]["parts"]["outline"]["level"], 1);
    ok(&store, &["install", "np", "--outlines", "0@100m"]);
    let text = ok(&store, &["status"]);
    assert!(
        text.contains("names ne  outlines to level 0 at lod 0"),
        "{text}"
    );
    let s = json(&store, &["status", "--json", "--deep"]);
    assert_eq!(s["packs"][0]["parts"]["outline"]["level"], 0);
    assert_eq!(s["packs"][0]["health"], "Good");
    // Past the deepest part: the whole outlines.
    ok(&store, &["install", "np", "--outlines", "4"]);
    let s = json(&store, &["status", "--json"]);
    assert_eq!(s["packs"][0]["parts"]["outline"], "base");

    // Usage errors exit 2.
    for bad in [
        ["--outlines", "x"],
        ["--outlines", "1@ten"],
        ["--names", "e"],
    ] {
        let o = tgd(&store, &[&["install", "np"][..], &bad[..]].concat());
        assert_eq!(o.status.code(), Some(2), "{bad:?}");
    }
}

/// A partial install takes only its sections' space on disk (D-189): what it never writes is
/// punched out, after an install, an added component and a repair alike.
#[cfg(any(target_vendor = "apple", target_os = "linux"))]
#[test]
fn a_partial_install_takes_only_its_bytes_on_disk() {
    use std::os::unix::fs::MetadataExt;
    let host: Host = Arc::default();
    publish(&host, 1, &[pack_of(100_000, "2026.11.0", false)]);
    let url = serve(Arc::clone(&host));
    let store = store_for("holes", &url);
    // The installed file: its length and the bytes it takes.
    let file = || {
        let f = std::fs::read_dir(store.join("packs"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|x| x == "tgd"))
            .unwrap();
        let m = std::fs::metadata(&f).unwrap();
        (f, m.len(), m.blocks() * 512)
    };
    let good = || {
        let s = json(&store, &["status", "--json", "--deep"]);
        assert_eq!(s["packs"][0]["health"], "Good", "{s}");
    };
    ok(&store, &["install", "np", "--components", "point"]);
    good();
    let (_, len, used) = file();
    // Point is 52% of this pack; the rest is never written.
    assert!(
        used * 10 < len * 6,
        "points only: {used} of {len} bytes on disk"
    );
    ok(&store, &["install", "np", "--components", "names"]);
    good();
    let (path, len, with_names) = file();
    assert!(
        with_names > used && with_names * 10 < len * 8,
        "{with_names} of {len}"
    );
    // A repair copies the installed file into staging: it must not fill the holes again.
    let mut bytes = std::fs::read(&path).unwrap();
    let at = bytes.len() / 50;
    bytes[at] ^= 1;
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(
        tgd(&store, &["status", "--deep"]).status.code(),
        Some(1),
        "damaged"
    );
    ok(&store, &["repair", "np"]);
    good();
    let (_, _, repaired) = file();
    assert!(
        repaired <= with_names + 65_536,
        "repaired: {repaired} of {len}"
    );
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

/// Sync gate 8 with a real process: `tgd install` and `tgd update` killed (SIGKILL on Unix) at
/// points across their download, each kill followed by `status --deep`, which must find every
/// installed pack whole (or none installed); the next run then finishes, keeping what checked.
#[test]
fn a_killed_install_or_update_never_damages_the_store() {
    let host: Host = Arc::default();
    publish(&host, 1, &[pack_of(20_000, "2026.11.0", false)]);
    let asset_len = host
        .lock()
        .unwrap()
        .iter()
        .find(|(k, _)| k.starts_with("assets/"))
        .map(|(_, v)| v.len())
        .unwrap();
    // About a second and a half for the whole asset.
    let url = serve_at(Arc::clone(&host), asset_len * 2 / 3);
    let store = store_for("killed", &url);
    ok(&store, &["refresh", "--countries", "np"]);
    let killed = |args: &[&str], after_ms: u64| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tgd"))
            .arg("--store")
            .arg(&store)
            .args(args)
            .env_remove("TEIGEODATA_STORE")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(after_ms));
        let finished = child.try_wait().unwrap().is_some();
        let _ = child.kill();
        let _ = child.wait();
        let s = tgd(&store, &["status", "--json", "--deep"]);
        assert_eq!(
            s.status.code(),
            Some(0),
            "{args:?} killed at {after_ms} ms: the store is damaged"
        );
        finished
    };
    let mut cut = 0;
    for ms in [80, 250, 450, 700, 950, 1200] {
        cut += usize::from(!killed(&["install", "np"], ms));
    }
    assert!(
        cut >= 3,
        "the kills landed before the install finished ({cut} of 6)"
    );
    let done = ok(&store, &["install", "np"]);
    assert!(
        done.contains("is installed") || !done.contains(" 0 kept"),
        "the next install keeps what an earlier one checked: {done}"
    );
    let s = json(&store, &["status", "--json", "--deep"]);
    assert_eq!(s["packs"][0]["release"], "2026.11.0");
    assert_eq!(s["packs"][0]["health"], "Good");

    // An update killed the same way: the old release stays whole until the new one commits.
    publish(&host, 2, &[pack_of(20_000, "2026.12.0", true)]);
    ok(&store, &["refresh"]);
    for ms in [80, 300, 600, 900] {
        killed(&["update"], ms);
        let s = json(&store, &["status", "--json", "--deep"]);
        assert_eq!(s["packs"].as_array().unwrap().len(), 1, "{s}");
        assert_eq!(s["packs"][0]["health"], "Good", "{s}");
    }
    ok(&store, &["update"]);
    let s = json(&store, &["status", "--json", "--deep"]);
    assert_eq!(s["packs"][0]["release"], "2026.12.0");
    assert_eq!(s["packs"][0]["health"], "Good");
    let staged: Vec<_> = std::fs::read_dir(store.join("staging")).unwrap().collect();
    assert!(
        staged.is_empty(),
        "no staging file is left behind: {staged:?}"
    );
}
