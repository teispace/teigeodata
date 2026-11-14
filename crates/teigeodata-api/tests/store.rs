//! The `Store` and `Job` handles end to end: a signed repository as a release writes it, served
//! on localhost, and a store from `init` to `remove` through jobs; progress that only moves
//! forward and ends at its total; and a cancel mid-install that leaves the store as it was
//! (research note store-api, D-274).

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "tests: failures panic with their message; one story per test"
)]
#![cfg(feature = "store")]

#[path = "../../teigeodata-engine/tests/common/mod.rs"]
mod common;
#[path = "../../teigeodata-sync/tests/support/repo.rs"]
mod repo_support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use repo_support::{Host, pack, pack_of, publish, root_file, serve, serve_at};
use teigeodata_api::{Blob, ErrorCode, InstallOptions, Job, Store};
use teigeodata_blob::{Reader, View};

fn strings(blob: &Blob, name: &str) -> Vec<String> {
    let r = Reader::new(blob.as_bytes()).unwrap();
    match r.column(name) {
        Some(View::Utf8(s)) => s.iter().map(str::to_owned).collect(),
        Some(View::Dict(d)) => (0..r.rows())
            .map(|i| d.get(i).unwrap().to_owned())
            .collect(),
        _ => panic!("{name} is not a string column"),
    }
}

fn f64s(blob: &Blob, name: &str) -> Vec<f64> {
    let r = Reader::new(blob.as_bytes()).unwrap();
    let Some(View::F64(v)) = r.column(name) else {
        panic!("{name} is not an f64 column")
    };
    v.f64s().collect()
}

fn u64s(blob: &Blob, name: &str) -> Vec<u64> {
    let r = Reader::new(blob.as_bytes()).unwrap();
    match r.column(name) {
        Some(View::U64(v)) => v.u64s().collect(),
        Some(View::U8(v)) => v.u8s().map(u64::from).collect(),
        _ => panic!("{name} is not a number column"),
    }
}

fn dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("api-store-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A store at a fresh directory, set up for the repository at `url`.
fn store_for(name: &str, url: &str) -> (Store, PathBuf) {
    let root = dir(name).join("store");
    let root_s = root.to_str().unwrap();
    let store = Store::create(
        root_s,
        &format!("{url}/meta"),
        &format!("{url}/assets/"),
        &root_file(),
    )
    .unwrap();
    // Opened again, it is the same store.
    drop(Store::open(root_s).unwrap());
    (store, root)
}

/// Runs `job` to its end, checking its progress on the way: the sequence and the bytes only
/// move forward, and the bytes end at their total.
fn run(job: &Job) -> Result<Blob, teigeodata_api::Error> {
    let (mut seq, mut done) = (0, 0);
    while !job.is_ended() {
        let next = job.wait_progress(seq, 5_000);
        assert!(next >= seq, "the sequence went back: {next} after {seq}");
        seq = next;
        let d = job.done_bytes();
        assert!(d >= done, "the bytes went back: {d} after {done}");
        assert!(job.total_bytes() >= d || job.is_ended());
        done = d;
    }
    assert_eq!(job.done_bytes(), job.total_bytes(), "ended at its total");
    // A wait on an ended job returns at once.
    let at = std::time::Instant::now();
    let _ = job.wait_progress(job.seq(), 60_000);
    assert!(at.elapsed().as_secs() < 5);
    job.result()
}

#[test]
fn a_store_from_init_to_remove_through_jobs() {
    let host: Host = Arc::default();
    publish(&host, 1, &[pack("2026.11.0", false)]);
    let url = serve(Arc::clone(&host));
    let (store, root) = store_for("lifecycle", &url);
    assert_eq!(store.damaged(), "");

    // Before a refresh the catalog is empty; a refresh for Nepal lists its pack.
    assert_eq!(
        strings(&store.catalog().unwrap(), "path"),
        Vec::<String>::new()
    );
    let r = run(&store.refresh("np")).unwrap();
    assert_eq!(u64s(&r, "timestamp"), [1]);
    assert!(u64s(&r, "bytes")[0] > 0);
    let c = store.catalog().unwrap();
    assert_eq!(strings(&c, "path"), ["np.places.tgd"]);
    assert_eq!(strings(&c, "state"), ["available"]);
    assert_eq!(strings(&c, "licence"), ["ODbL-1.0"]);

    // Bad options fail at once, before any job.
    for bad in [
        InstallOptions {
            components: "pointy".into(),
            ..InstallOptions::default()
        },
        InstallOptions {
            languages: "not a tag".into(),
            ..InstallOptions::default()
        },
        InstallOptions {
            outline_level: Some(256),
            ..InstallOptions::default()
        },
    ] {
        assert_eq!(
            store.install("np", &bad).unwrap_err().code,
            ErrorCode::InvalidArgument,
            "{bad:?}"
        );
    }

    // Search alone (with what it needs): the engine answers by name and lacks the rest.
    let options = InstallOptions {
        components: "search".into(),
        ..InstallOptions::default()
    };
    let job = store.install("np", &options).unwrap();
    let d = run(&job).unwrap();
    assert_eq!(strings(&d, "path"), ["np.places.tgd"]);
    assert_eq!(u64s(&d, "changed"), [1]);
    assert!(u64s(&d, "fetched")[0] > 0);
    assert!(job.done_bytes() >= u64s(&d, "fetched")[0]);
    let s = store.status(true).unwrap();
    assert_eq!(strings(&s, "components"), ["point,names,search"]);
    assert_eq!(strings(&s, "health"), ["good"]);
    let engine = store.engine().unwrap();
    assert_eq!(engine.layers(), 1);
    let hit = engine.search("Ritarra", 1).unwrap();
    let id = strings(&hit, "id").remove(0);
    assert_eq!(strings(&engine.get(&id).unwrap(), "name"), ["Ritarra"]);
    assert_eq!(
        engine.tz_at(27.7, 85.3).unwrap_err().code,
        ErrorCode::MissingComponent
    );

    // What to install for a name: the catalog's packs over the engine's places, offline.
    let found = store.find(&engine, "Ritarra", 3).unwrap();
    assert_eq!(strings(&found, "name"), ["Ritarra"]);
    assert_eq!(strings(&found, "countries"), ["np"]);
    assert_eq!(strings(&found, "pack"), ["np.places.tgd"]);
    assert_eq!(strings(&found, "state"), ["installed"]);
    assert!(u64s(&found, "download")[0] > 0);
    assert_eq!(
        store.find(&engine, "Ritarra", 10_001).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    // Nothing found is no row; an empty engine finds nothing.
    assert_eq!(
        strings(&store.find(&engine, "Zzyzx Qwv", 3).unwrap(), "name"),
        Vec::<String>::new()
    );
    let (lat, lon) = (f64s(&hit, "lat")[0], f64s(&hit, "lon")[0]);

    // Everything, pinned: the same release, more components.
    let all = InstallOptions {
        pin: true,
        ..InstallOptions::default()
    };
    run(&store.install("np", &all).unwrap()).unwrap();
    assert_eq!(strings(&store.catalog().unwrap(), "state"), ["installed"]);
    let again = run(&store.install("np", &all).unwrap()).unwrap();
    assert_eq!(u64s(&again, "changed"), [0], "installed already");
    assert_eq!(u64s(&store.status(false).unwrap(), "pinned"), [1]);
    // What to install for where one stands: the countries whose outlines lie near a point.
    // The synthetic pack has no arc outlines, so no country; the engine's world tests check
    // the locator on real outlines.
    let engine = store.engine().unwrap();
    let here = store.locate(&engine, lat, lon, 1_000).unwrap();
    assert_eq!(strings(&here, "pack"), Vec::<String>::new());
    assert_eq!(
        store.locate(&engine, 91.0, 0.0, 1_000).unwrap_err().code,
        ErrorCode::InvalidArgument
    );

    // A new release: the update fetches it, reusing what did not change.
    publish(&host, 2, &[pack("2026.12.0", true)]);
    let u = run(&store.update("")).unwrap();
    assert_eq!(strings(&u, "release"), ["2026.12.0"]);
    assert!(u64s(&u, "reused")[0] > 0);
    assert_eq!(
        strings(&store.status(false).unwrap(), "release"),
        ["2026.12.0"]
    );

    // Damage the file: a deep status finds it, a repair fetches what it lacks.
    let file = std::fs::read_dir(root.join("packs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut bytes = std::fs::read(&file).unwrap();
    let at = bytes.len() / 2;
    bytes[at] ^= 0xff;
    std::fs::write(&file, &bytes).unwrap();
    assert_eq!(
        strings(&store.status(true).unwrap(), "health"),
        ["sections"]
    );
    let r = run(&store.repair("np")).unwrap();
    assert_eq!(strings(&r, "health"), ["sections"]);
    assert!(u64s(&r, "fetched")[0] > 0);
    assert_eq!(strings(&store.status(true).unwrap(), "health"), ["good"]);
    // A whole pack's repair fetches nothing.
    assert_eq!(u64s(&run(&store.repair("np")).unwrap(), "fetched"), [0]);

    // Unpin, remove; a second remove is refused.
    store.pin("np", false).unwrap();
    let gone = store.remove("np").unwrap();
    assert_eq!(strings(&gone, "path"), ["np.places.tgd"]);
    assert_eq!(
        strings(&store.status(false).unwrap(), "path"),
        Vec::<String>::new()
    );
    assert_eq!(store.remove("np").unwrap_err().code, ErrorCode::Refused);
    assert_eq!(store.pin("np", true).unwrap_err().code, ErrorCode::Refused);

    // A job's errors come through its result: an update of nothing installed is refused.
    assert_eq!(
        run(&store.update("np")).unwrap_err().code,
        ErrorCode::Refused
    );
}

#[test]
fn the_network_and_a_store_not_set_up_have_their_codes() {
    let d = dir("codes");
    let missing = d.join("nothing");
    assert_eq!(
        Store::open(missing.to_str().unwrap()).unwrap_err().code,
        ErrorCode::Refused
    );
    assert_eq!(
        Store::create(d.join("s").to_str().unwrap(), "http://x/meta", "", b"{}")
            .unwrap_err()
            .code,
        ErrorCode::Refused,
        "a root that does not verify"
    );
    // A host that answers nothing: the refresh fails at the network.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let (store, _) = store_for("network", &url);
    assert_eq!(
        run(&store.refresh("np")).unwrap_err().code,
        ErrorCode::Network
    );
}

#[test]
fn a_cancelled_install_leaves_the_store_as_it_was() {
    let host: Host = Arc::default();
    publish(&host, 1, &[pack_of(20_000, "2026.11.0", false)]);
    let asset_len = host
        .lock()
        .unwrap()
        .iter()
        .find(|(k, _)| k.starts_with("assets/"))
        .map(|(_, v)| v.len())
        .unwrap();
    // About three seconds for the whole asset.
    let url = serve_at(Arc::clone(&host), asset_len / 3);
    let (store, root) = store_for("cancel", &url);
    run(&store.refresh("np")).unwrap();
    let journal = std::fs::read(root.join("journal")).ok();

    let job = store.install("np", &InstallOptions::default()).unwrap();
    // Cancel once a tenth of the pack has come (the catalog's metadata comes first), well
    // before all of it.
    let mut seq = 0;
    while job.done_bytes() < asset_len as u64 / 10 && !job.is_ended() {
        seq = job.wait_progress(seq, 5_000);
    }
    assert!(job.total_bytes() > job.done_bytes(), "still downloading");
    job.cancel();
    let e = job.result().unwrap_err();
    assert_eq!(e.code, ErrorCode::Cancelled, "{e:?}");
    assert!(job.is_ended());
    assert_eq!(job.done_bytes(), job.total_bytes());

    // The journal is as it was and nothing is installed; the next install completes.
    assert_eq!(std::fs::read(root.join("journal")).ok(), journal);
    assert_eq!(
        strings(&store.status(true).unwrap(), "path"),
        Vec::<String>::new()
    );
    let d = run(&store.install("np", &InstallOptions::default()).unwrap()).unwrap();
    assert_eq!(u64s(&d, "changed"), [1]);
    assert_eq!(strings(&store.status(true).unwrap(), "health"), ["good"]);
}

#[test]
fn handles_are_send_and_sync() {
    fn both<T: Send + Sync>() {}
    both::<Store>();
    both::<Job>();
}

/// `bindings/fixtures/store-root.json`, the trusted root every language's store parity check
/// sets a store up with: the test repository's keys, expiring in 2036 so the file stays
/// valid; `TEIGEODATA_WRITE_FIXTURE=1` rewrites it. Signatures are deterministic (Ed25519).
#[test]
fn the_bindings_store_root_is_current() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../bindings/fixtures/store-root.json"
    );
    let roles = [
        repo_support::one(1),
        repo_support::one(10),
        repo_support::one(20),
        repo_support::one(30),
    ];
    let root = teigeodata_sync::repo::root(1, 2_082_758_400, &roles).unwrap();
    let bytes = teigeodata_sync::repo::sign(&root, &[&repo_support::key(1)]).unwrap();
    if std::env::var_os("TEIGEODATA_WRITE_FIXTURE").is_some() {
        std::fs::write(path, &bytes).unwrap();
    }
    assert!(
        std::fs::read(path).unwrap() == bytes,
        "{path} is stale: rerun with TEIGEODATA_WRITE_FIXTURE=1"
    );
}

/// The store section of `bindings/parity.json`, which every language checks, holds what Rust
/// does: a store set up with the fixture root and a mirror that refuses connections.
#[test]
fn the_store_parity_matches_rust() {
    let parity: serde_json::Value =
        serde_json::from_str(include_str!("../../../bindings/parity.json")).unwrap();
    let p = &parity["store"];
    let s = |v: &serde_json::Value| v.as_str().unwrap().to_owned();
    for (name, code) in [
        ("InvalidArgument", ErrorCode::InvalidArgument),
        ("Cancelled", ErrorCode::Cancelled),
        ("Internal", ErrorCode::Internal),
        ("Io", ErrorCode::Io),
        ("Corrupt", ErrorCode::Corrupt),
        ("MissingComponent", ErrorCode::MissingComponent),
        ("Network", ErrorCode::Network),
        ("Refused", ErrorCode::Refused),
    ] {
        assert_eq!(parity["error_codes"][name], code as i32, "{name}");
    }
    let d = dir("parity");
    let root = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../bindings")
            .join(s(&p["root"])),
    )
    .unwrap();
    let at = |x: &str| d.join(x).to_str().unwrap().to_owned();
    assert_eq!(
        Store::open(&at("missing")).unwrap_err().code,
        ErrorCode::Refused
    );
    let mirror = s(&p["mirror"]);
    assert_eq!(
        Store::create(&at("bad"), &mirror, &mirror, b"{}")
            .unwrap_err()
            .code,
        ErrorCode::Refused
    );
    drop(Store::create(&at("s"), &mirror, &mirror, &root).unwrap());
    let store = Store::open(&at("s")).unwrap();
    assert_eq!(store.damaged(), "");
    assert_eq!(rows(&store.catalog().unwrap()), 0);
    assert_eq!(rows(&store.status(false).unwrap()), 0);
    assert_eq!(store.engine().unwrap().layers(), 0);
    let pack = s(&p["pack"]);
    let bad = InstallOptions {
        components: s(&p["bad_components"]),
        ..InstallOptions::default()
    };
    assert_eq!(
        store.install(&pack, &bad).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(store.remove(&pack).unwrap_err().code, ErrorCode::Refused);
    let job = store.refresh(&pack);
    assert_eq!(job.result().unwrap_err().code, ErrorCode::Network);
    assert!(job.is_ended());
    assert_eq!(job.done_bytes(), job.total_bytes());
    assert_eq!(job.wait_progress(job.seq(), 60_000), job.seq());
}

fn rows(blob: &Blob) -> usize {
    Reader::new(blob.as_bytes()).unwrap().rows()
}
