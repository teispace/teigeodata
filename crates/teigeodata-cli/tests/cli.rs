//! `tgd` end to end: the binary run on packs written here, its output read, its exit codes
//! checked (0 done, 1 refused, 2 usage).

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::many_single_char_names,
    clippy::format_collect,
    reason = "tests: failures panic with their message; counts are small; short names for a path and a pack"
)]

#[path = "../../teigeodata-engine/tests/common/mod.rs"]
mod common;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer, key_id, signer};

const ATTRIBUTION: &str = "© OpenStreetMap contributors, ODbL 1.0\nGeoNames, CC BY 4.0";

fn key() -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::from_bytes(&[7; 32])
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A scratch directory for one test.
fn dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The model's pack again, with attribution and licence, signed by [`key`].
fn signed(model_pack: &[u8]) -> Vec<u8> {
    let p = Pack::open(model_pack).unwrap();
    let sections: Vec<(Kind, Encoding, u64, &[u8])> = p
        .sections()
        .filter(|s| s.kind() != Kind::BLOCK_HASHES)
        .map(|s| (s.kind(), s.encoding(), s.item_count(), s.bytes()))
        .collect();
    let mut w = Writer::new("np", "places", "2026.11.0").unwrap();
    w.manifest(Tag::LICENCE, b"odbl").unwrap();
    w.manifest(Tag::ATTRIBUTION, ATTRIBUTION.as_bytes())
        .unwrap();
    if let Some(b) = p.manifest().get(Tag::BBOX) {
        w.manifest(Tag::BBOX, b).unwrap();
    }
    let key = key();
    w.pack(&sections, Some(&signer(&key))).unwrap()
}

/// A signed pack of `n` places in `d`.
fn pack_in(d: &Path, n: usize) -> PathBuf {
    let path = d.join("np.tgd");
    std::fs::write(&path, signed(&common::synthetic(n, 3).encode())).unwrap();
    path
}

fn tgd(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tgd"))
        .args(args)
        .output()
        .unwrap()
}

fn ok(args: &[&str]) -> String {
    let o = tgd(args);
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout).unwrap()
}

fn json(args: &[&str]) -> Value {
    serde_json::from_str(&ok(args)).unwrap()
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn inspect_shows_the_manifest_sections_and_components() {
    let d = dir("inspect");
    let p = pack_in(&d, 400);
    let j = json(&["inspect", s(&p), "--json"]);
    assert_eq!(j["manifest"]["address"], "np");
    assert_eq!(j["manifest"]["licence"], "odbl");
    assert_eq!(j["manifest"]["attribution"][1], "GeoNames, CC BY 4.0");
    assert_eq!(j["signed_by"], hex(&key_id(&key().verifying_key())));
    let sections = j["sections"].as_array().unwrap();
    assert!(
        sections
            .iter()
            .any(|x| x["kind"] == "PLACES" && x["component"] == "point")
    );
    let total: u64 = j["components"]
        .as_object()
        .unwrap()
        .values()
        .map(|v| v.as_u64().unwrap())
        .sum();
    let listed: u64 = sections.iter().map(|x| x["bytes"].as_u64().unwrap()).sum();
    assert_eq!(total, listed, "every section's bytes are counted once");
    let text = ok(&["inspect", s(&p)]);
    assert!(text.starts_with("np places 2026.11.0"), "{text}");
    assert!(text.contains("attribution: 2 notices"), "{text}");
}

#[test]
fn verify_checks_hashes_values_and_the_signature() {
    let d = dir("verify");
    let p = pack_in(&d, 400);
    let pk = hex(key().verifying_key().as_bytes());
    let out = ok(&["verify", s(&p), "--deep", "--key", &pk]);
    assert!(
        out.contains("checked value by value") && out.contains("signed by trusted key"),
        "{out}"
    );
    // Another key: refused.
    let other = hex(ed25519_dalek::SigningKey::from_bytes(&[8; 32])
        .verifying_key()
        .as_bytes());
    let o = tgd(&["verify", s(&p), "--key", &other]);
    assert_eq!(o.status.code(), Some(1));
    // A malformed key: refused before anything is read.
    assert_eq!(
        tgd(&["verify", s(&p), "--key", "abc"]).status.code(),
        Some(1)
    );
    // One flipped byte inside PLACES: named.
    let bytes = std::fs::read(&p).unwrap();
    let places = Pack::open(&bytes).unwrap().section(Kind::PLACES).unwrap();
    let at = places.bytes().as_ptr() as usize - bytes.as_ptr() as usize + 3;
    let mut bad = bytes.clone();
    bad[at] ^= 1;
    let q = d.join("bad.tgd");
    std::fs::write(&q, bad).unwrap();
    let o = tgd(&["verify", s(&q)]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("PLACES"),
        "{o:?}"
    );
    let j: Value = serde_json::from_slice(&tgd(&["verify", s(&q), "--json"]).stdout).unwrap();
    assert_eq!(j["ok"], false);
}

#[test]
fn diff_counts_each_change_by_id() {
    let d = dir("diff");
    let mut model = common::synthetic(500, 3);
    let old = d.join("old.tgd");
    std::fs::write(&old, signed(&model.encode())).unwrap();
    let same = json(&["diff", s(&old), s(&old), "--json"]);
    for what in ["added", "removed", "moved", "renamed", "retyped"] {
        assert_eq!(same["places"][what]["count"], 0, "{what}");
    }
    assert!(
        same["sections"]
            .as_array()
            .unwrap()
            .iter()
            .all(|x| x["change"] == "same")
    );
    // Row 7 moves about 111 m north; rows 0..10 except the linked ones are dropped.
    let moved_id = model.ids[7];
    let p = model.points[7];
    model.points[7] =
        teigeodata_core::Point::from_degrees(p.lat_degrees() + 0.001, p.lon_degrees()).unwrap();
    let linked = model.linked_rows();
    let keep: Vec<u32> = (0..model.ids.len() as u32)
        .filter(|r| *r >= 10 || *r == 7 || linked.contains(r))
        .collect();
    let dropped = model.ids.len() - keep.len();
    let new_model = model.subset("np", &keep, |_| false);
    let new = d.join("new.tgd");
    std::fs::write(&new, signed(&new_model.encode())).unwrap();
    let j = json(&["diff", s(&old), s(&new), "--json", "--limit", "3"]);
    assert_eq!(j["places"]["removed"]["count"], dropped as u64);
    assert_eq!(j["places"]["added"]["count"], 0);
    assert_eq!(j["places"]["moved"]["count"], 1);
    assert_eq!(j["places"]["moved"]["first"][0]["id"], moved_id.to_string());
    let m = j["places"]["moved"]["first"][0]["meters"].as_f64().unwrap();
    assert!((m - 111.2).abs() < 1.0, "{m}");
    assert!(j["places"]["removed"]["first"].as_array().unwrap().len() <= 3);
    assert!(ok(&["diff", s(&old), s(&new)]).contains("moved: 1"));
    let back = json(&["diff", s(&new), s(&old), "--json"]);
    assert_eq!(back["places"]["added"]["count"], dropped as u64);
    assert_eq!(back["places"]["removed"]["count"], 0);
}

#[test]
fn extract_by_component_copies_the_closed_set_as_stored() {
    let d = dir("extract-components");
    let p = pack_in(&d, 600);
    let out = d.join("names.tgd");
    ok(&["extract", s(&p), "-o", s(&out), "--components", "search"]);
    let j = json(&["inspect", s(&out), "--json"]);
    let kinds: Vec<&str> = j["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["kind"].as_str().unwrap())
        .collect();
    for k in ["IDMAP", "PLACES", "NAMES", "SEARCH"] {
        assert!(kinds.contains(&k), "{k} missing: {kinds:?}");
    }
    for k in ["ELEVATION", "HIERARCHY", "PLACE_TZ"] {
        assert!(!kinds.contains(&k), "{k} kept: {kinds:?}");
    }
    assert_eq!(j["manifest"]["component"], "extract");
    assert_eq!(
        j["manifest"]["attribution"][0],
        "© OpenStreetMap contributors, ODbL 1.0"
    );
    assert_eq!(j["signed_by"], Value::Null);
    // Same bytes as the source's sections.
    let src = json(&["inspect", s(&p), "--json"]);
    let hash = |v: &Value, k: &str| {
        v["sections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["kind"] == k)
            .unwrap()["hash"]
            .clone()
    };
    assert_eq!(hash(&j, "NAMES"), hash(&src, "NAMES"));
    ok(&["verify", s(&out), "--deep"]);
    // Asking for nothing, or for an unknown component, is refused.
    assert_eq!(
        tgd(&["extract", s(&p), "-o", s(&out)]).status.code(),
        Some(1)
    );
    assert_eq!(
        tgd(&["extract", s(&p), "-o", s(&out), "--components", "roads"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn extract_by_box_keeps_the_places_in_it_with_their_chains() {
    let d = dir("extract-box");
    let p = pack_in(&d, 1_500);
    let full: Vec<Value> = ok(&["export", s(&p)])
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    // The box around the middle of the places.
    let mut lats: Vec<f64> = full.iter().map(|r| r["lat"].as_f64().unwrap()).collect();
    let mut lons: Vec<f64> = full.iter().map(|r| r["lon"].as_f64().unwrap()).collect();
    lats.sort_by(f64::total_cmp);
    lons.sort_by(f64::total_cmp);
    let (s0, n0) = (lats[lats.len() / 4], lats[lats.len() * 3 / 4]);
    let (w0, e0) = (lons[lons.len() / 4], lons[lons.len() * 3 / 4]);
    let bbox = format!("{w0},{s0},{e0},{n0}");
    let out = d.join("box.tgd");
    ok(&["extract", s(&p), "-o", s(&out), "--bbox", &bbox]);
    ok(&["verify", s(&out), "--deep"]);
    let inside = |r: &Value| {
        let (la, lo) = (r["lat"].as_f64().unwrap(), r["lon"].as_f64().unwrap());
        (s0..=n0).contains(&la) && (w0..=e0).contains(&lo)
    };
    let kept: Vec<Value> = ok(&["export", s(&out)])
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let want = full.iter().filter(|r| inside(r)).count();
    assert_eq!(
        kept.iter().filter(|r| inside(r)).count(),
        want,
        "every place in the box"
    );
    assert!(kept.len() < full.len());
    // A place outside the box is there only as an ancestor or a timezone area of one inside.
    let ids: std::collections::BTreeSet<&str> =
        kept.iter().map(|r| r["id"].as_str().unwrap()).collect();
    for r in &kept {
        if let Some(parent) = r["parent"].as_str() {
            assert!(ids.contains(parent), "{r}: its parent is not kept");
        }
        let same = full.iter().find(|f| f["id"] == r["id"]).unwrap();
        assert_eq!(r["parent"], same["parent"], "{r}: another parent");
        assert_eq!(r["name"], same["name"]);
        assert_eq!(r["timezone"], same["timezone"]);
    }
}

#[test]
fn export_streams_every_place_as_ndjson_or_geojson() {
    let d = dir("export");
    let p = pack_in(&d, 800);
    let rows = json(&["inspect", s(&p), "--json"])["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["kind"] == "PLACES")
        .unwrap()["items"]
        .as_u64()
        .unwrap();
    let ndjson = ok(&["export", s(&p)]);
    assert_eq!(ndjson.lines().count() as u64, rows);
    let first: Value = serde_json::from_str(ndjson.lines().next().unwrap()).unwrap();
    for f in [
        "id",
        "name",
        "kind",
        "admin_level",
        "lat",
        "lon",
        "elevation",
        "parent",
        "timezone",
    ] {
        assert!(first.get(f).is_some(), "{f} missing in {first}");
    }
    let out = d.join("all.geojson");
    ok(&["export", s(&p), "--format", "geojson", "-o", s(&out)]);
    let g: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(g["type"], "FeatureCollection");
    assert_eq!(g["attribution"], ATTRIBUTION);
    let features = g["features"].as_array().unwrap();
    assert_eq!(features.len() as u64, rows);
    assert_eq!(features[0]["geometry"]["coordinates"][0], first["lon"]);
    assert_eq!(features[0]["properties"]["name"], first["name"]);
    assert!(features[0]["properties"].get("lat").is_none());
    // A reader that stops early ends the export quietly.
    let mut child = Command::new(env!("CARGO_BIN_EXE_tgd"))
        .args(["export", s(&p)])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(0));
}

#[test]
fn usage_and_missing_files_have_their_exit_codes() {
    assert_eq!(tgd(&["inspect"]).status.code(), Some(2));
    assert_eq!(tgd(&["nonsense"]).status.code(), Some(2));
    let o = tgd(&["inspect", "/no/such/pack.tgd"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("tgd: /no/such/pack.tgd"));
    assert!(ok(&["--help"]).contains("extract"));
}

#[test]
fn only_the_deep_check_finds_a_well_hashed_inconsistency() {
    let d = dir("verify-deep");
    let model = common::synthetic(300, 3);
    let bytes = model.encode();
    let p = Pack::open(&bytes).unwrap();
    // ELEVATION one row short: its hash is right, its row count is not.
    let short = vec![Some(9_i16); model.ids.len() - 1];
    let body = teigeodata_format::elevation::encode(&short)
        .unwrap()
        .unwrap();
    let sections: Vec<(Kind, Encoding, u64, &[u8])> = p
        .sections()
        .filter(|s| s.kind() != Kind::BLOCK_HASHES)
        .map(|s| {
            if s.kind() == Kind::ELEVATION {
                (s.kind(), s.encoding(), short.len() as u64, body.as_slice())
            } else {
                (s.kind(), s.encoding(), s.item_count(), s.bytes())
            }
        })
        .collect();
    let q = d.join("short.tgd");
    let w = Writer::new("np", "places", "2026.11.0").unwrap();
    std::fs::write(&q, w.pack(&sections, None).unwrap()).unwrap();
    ok(&["verify", s(&q)]);
    let o = tgd(&["verify", s(&q), "--deep"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("ELEVATION"),
        "{o:?}"
    );
}

#[test]
fn a_tiny_box_keeps_its_places_whole_chain() {
    let d = dir("extract-tiny");
    let p = pack_in(&d, 1_500);
    let full: Vec<Value> = ok(&["export", s(&p)])
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let by_id = |id: &str| full.iter().find(|r| r["id"] == id).unwrap();
    // A place with a grandparent: its box holds it alone, its chain lies outside.
    let leaf = full
        .iter()
        .find(|r| {
            r["parent"]
                .as_str()
                .is_some_and(|p| by_id(p)["parent"].is_string())
        })
        .unwrap();
    let (la, lo) = (leaf["lat"].as_f64().unwrap(), leaf["lon"].as_f64().unwrap());
    let bbox = format!("{},{},{},{}", lo - 1e-6, la - 1e-6, lo + 1e-6, la + 1e-6);
    let out = d.join("tiny.tgd");
    ok(&["extract", s(&p), "-o", s(&out), "--bbox", &bbox]);
    let kept: Vec<Value> = ok(&["export", s(&out)])
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let mut at = leaf;
    while let Some(parent) = at["parent"].as_str() {
        let k = kept.iter().find(|r| r["id"] == at["id"]).unwrap();
        assert_eq!(k["parent"], parent, "{k}: the chain is cut");
        at = by_id(parent);
    }
}
