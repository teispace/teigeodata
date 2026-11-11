//! What `repo` writes is a TUF repository python-tuf reads (D-162), delegated by country as
//! `tuf release` writes it (D-176): Nepal's pack in the role `np`, a pack of no country in the
//! top-level targets. This test writes a fixed
//! repository (fixed keys and times; Ed25519 and the JSON layout are deterministic) and compares
//! it with `tests/tuf-written/`; the fast-check job `tuf-oracle` loads those files with
//! python-tuf's `TrustedMetadataSet` (`tools/tuf-oracle/read_written.py`).
//! `TEIGEODATA_BLESS=1` rewrites the files.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "test code")]

use std::collections::BTreeMap;
use std::path::Path;

use ed25519_dalek::SigningKey;
use teigeodata_sync::catalog::{Entry, Size, file_name};
use teigeodata_sync::repo::{
    RoleKeys, TargetFile, by_country, country_role, root, sign, snapshot_listing, target_hashes,
    targets, targets_delegating, timestamp,
};
use teigeodata_sync::trust::Trusted;

const NOW: i64 = 1_794_268_800; // 2026-11-10
const YEAR: i64 = 365 * 86_400;
const ASSET: &[u8] = b"stands in for a transport asset";

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

/// The repository's files by name; the asset under its catalog file name, as releases store it.
fn written() -> BTreeMap<String, Vec<u8>> {
    let public = |s: u8| key(s).verifying_key().to_bytes();
    let one = |s: u8| RoleKeys {
        keys: vec![public(s)],
        threshold: 1,
    };
    let roles = [
        RoleKeys {
            keys: vec![public(1), public(2), public(3)],
            threshold: 2,
        },
        one(10),
        one(20),
        one(30),
    ];
    let hashes = target_hashes(ASSET);
    let mut files = BTreeMap::new();
    let mut assets = Vec::new();
    for address in ["np", "earth"] {
        let entry = entry(address, &hashes["sha256"]);
        assets.push(entry.file.clone());
        files.insert(
            entry.path(),
            TargetFile {
                length: ASSET.len() as u64,
                hashes: hashes.clone(),
                custom: Some(entry.to_custom()),
            },
        );
    }
    let (top, countries) = by_country(files);
    let np = sign(
        &targets(1, NOW + YEAR, &countries[b"np"]).unwrap(),
        &[&key(30)],
    )
    .unwrap();
    let delegated = [country_role(*b"np", one(30))];
    let root1 = sign(&root(1, NOW + YEAR, &roles).unwrap(), &[&key(1), &key(2)]).unwrap();
    let tg = sign(
        &targets_delegating(1, NOW + YEAR, &top, &delegated).unwrap(),
        &[&key(30)],
    )
    .unwrap();
    let sn = sign(
        &snapshot_listing(1, NOW + YEAR, &[("targets", &tg, 1), ("np", &np, 1)]).unwrap(),
        &[&key(20)],
    )
    .unwrap();
    let ts = sign(
        &timestamp(1, NOW + 14 * 86_400, &sn, 1).unwrap(),
        &[&key(10)],
    )
    .unwrap();
    let mut out = BTreeMap::from([
        ("1.root.json".to_string(), root1),
        ("1.targets.json".to_string(), tg),
        ("1.np.json".to_string(), np),
        ("1.snapshot.json".to_string(), sn),
        ("timestamp.json".to_string(), ts),
    ]);
    for name in assets {
        out.insert(name, ASSET.to_vec());
    }
    out
}

/// The catalog entry of the stand-in asset at `address`.
fn entry(address: &str, sha256: &[u8]) -> Entry {
    let mut sizes = BTreeMap::new();
    sizes.insert(
        teigeodata_format::component::Component::Point,
        Size { wire: 20, disk: 64 },
    );
    Entry {
        address: address.into(),
        component: "places".into(),
        release: "2026.11.0".into(),
        file: file_name(address, "places", "2026.11.0", sha256),
        pack_length: 64 + 64 + teigeodata_format::FOOTER_SIZE as u64,
        head: [0xab; 16],
        seek_table: 9,
        head_frame: Size { wire: 2, disk: 64 },
        block_hashes: None,
        sizes,
        deps: vec![],
        min_reader: 0,
        bbox: None,
        licence: "ODbL-1.0".into(),
    }
}
#[test]
fn the_written_repository_is_the_checked_in_one_and_verifies() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tuf-written");
    let files = written();
    assert_eq!(files.len(), 7);
    if std::env::var_os("TEIGEODATA_BLESS").is_some() {
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in &files {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
    }
    for (name, bytes) in &files {
        let on_disk = std::fs::read(dir.join(name)).unwrap();
        assert!(
            on_disk == *bytes,
            "{name} differs; rerun with TEIGEODATA_BLESS=1"
        );
    }
    let mut t = Trusted::new(&files["1.root.json"], NOW).unwrap();
    t.update_timestamp(&files["timestamp.json"]).unwrap();
    t.update_snapshot(&files["1.snapshot.json"], false).unwrap();
    t.update_targets(&files["1.targets.json"]).unwrap();
    assert_eq!(t.roles_for("np.places.tgd"), ["np"]);
    t.update_delegated("np", &files["1.np.json"]).unwrap();
    let earth = t.target("earth.places.tgd").unwrap();
    let earth = Entry::from_target("earth.places.tgd", earth).unwrap();
    t.verify_target("earth.places.tgd", &files[&earth.file])
        .unwrap();
    let target = t.target("np.places.tgd").unwrap();
    let entry = Entry::from_target("np.places.tgd", target).unwrap();
    t.verify_target("np.places.tgd", &files[&entry.file])
        .unwrap();
    assert_eq!(t.target_name("np.places.tgd").unwrap().len(), 64 + 1 + 13);
}
