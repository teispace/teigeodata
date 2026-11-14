//! A signed repository written as a release writes it, and a local HTTP host serving it with
//! range requests: shared by the tests that drive a store end to end (the command line's and
//! the API's). Included by path, beside the engine's `tests/common` as `crate::common`.

#![allow(
    dead_code,
    reason = "each test crate that includes this uses some of it"
)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use ed25519_dalek::SigningKey;
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer};
use teigeodata_sync::repo::{
    self, Files, RoleKeys, TargetFile, country_role, sign, snapshot_listing, targets,
    targets_delegating, timestamp,
};

pub(crate) const YEAR: i64 = 365 * 86_400;

pub(crate) fn now() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap()
}

pub(crate) fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

pub(crate) fn one(seed: u8) -> RoleKeys {
    RoleKeys {
        keys: vec![key(seed).verifying_key().to_bytes()],
        threshold: 1,
    }
}

/// Root keys 1 (threshold 1), timestamp 10, snapshot 20, targets 30.
pub(crate) fn root_file() -> Vec<u8> {
    let roles = [one(1), one(10), one(20), one(30)];
    sign(&repo::root(1, now() + YEAR, &roles).unwrap(), &[&key(1)]).unwrap()
}

/// The synthetic Nepal pack at `release`, with a licence and attribution; `shift` moves one
/// place so releases differ.
pub(crate) fn pack(release: &str, shift: bool) -> Vec<u8> {
    pack_of(700, release, shift)
}

/// [`pack`] with `n` places.
pub(crate) fn pack_of(n: usize, release: &str, shift: bool) -> Vec<u8> {
    let mut model = crate::common::synthetic(n, 3);
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
pub(crate) type Host = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

/// Release `v` of `packs` on `host`: assets, Nepal's role, top-level targets, snapshot and
/// timestamp, as `tuf release` lays them out (consistent snapshots).
pub(crate) fn publish(host: &Host, v: u64, packs: &[Vec<u8>]) {
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
pub(crate) fn serve(host: Host) -> String {
    serve_at(host, 0)
}

/// [`serve`], sending bodies at about `rate` bytes a second (0: at once), so a client can be
/// killed mid-download.
pub(crate) fn serve_at(host: Host, rate: usize) -> String {
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
            if rate == 0 {
                let _ = c.write_all(&reply);
            } else {
                for chunk in reply.chunks((rate / 20).max(1)) {
                    if c.write_all(chunk).is_err() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
            let _ = c.flush();
            let mut sink = [0_u8; 1];
            let _ = c.set_read_timeout(Some(std::time::Duration::from_millis(5)));
            let _ = c.read(&mut sink);
        }
    });
    format!("http://{addr}")
}
