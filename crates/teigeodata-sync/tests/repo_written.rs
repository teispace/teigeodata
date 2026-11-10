//! What `repo` writes is a TUF repository python-tuf reads (D-162). This test writes a fixed
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
    RoleKeys, TargetFile, root, sign, snapshot, target_hashes, targets, timestamp,
};
use teigeodata_sync::trust::{RoleName, Trusted};

const NOW: i64 = 1_794_268_800; // 2026-11-10
const YEAR: i64 = 365 * 86_400;
const ASSET: &[u8] = b"stands in for a transport asset";

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn written() -> BTreeMap<&'static str, Vec<u8>> {
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
    let mut sizes = BTreeMap::new();
    sizes.insert(
        teigeodata_format::component::Component::Point,
        Size { wire: 20, disk: 64 },
    );
    let entry = Entry {
        address: "np".into(),
        component: "places".into(),
        release: "2026.11.0".into(),
        file: file_name("np", "places", "2026.11.0", &hashes["sha256"]),
        pack_length: 128,
        head: [0xab; 16],
        seek_table: 9,
        head_frame: Size { wire: 2, disk: 64 },
        block_hashes: None,
        sizes,
        deps: vec![],
        min_reader: 0,
        bbox: None,
        licence: "ODbL-1.0".into(),
    };
    let mut files = BTreeMap::new();
    files.insert(
        entry.path(),
        TargetFile {
            length: ASSET.len() as u64,
            hashes,
            custom: Some(entry.to_custom()),
        },
    );
    let root1 = sign(&root(1, NOW + YEAR, &roles).unwrap(), &[&key(1), &key(2)]).unwrap();
    let tg = sign(&targets(1, NOW + YEAR, &files).unwrap(), &[&key(30)]).unwrap();
    let sn = sign(&snapshot(1, NOW + YEAR, &tg, 1).unwrap(), &[&key(20)]).unwrap();
    let ts = sign(
        &timestamp(1, NOW + 14 * 86_400, &sn, 1).unwrap(),
        &[&key(10)],
    )
    .unwrap();
    BTreeMap::from([
        ("1.root.json", root1),
        ("1.targets.json", tg),
        ("1.snapshot.json", sn),
        ("timestamp.json", ts),
        ("np.places.tgd", ASSET.to_vec()),
    ])
}

#[test]
fn the_written_repository_is_the_checked_in_one_and_verifies() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tuf-written");
    let files = written();
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
    t.verify_target("np.places.tgd", &files["np.places.tgd"])
        .unwrap();
    let target = t.target("np.places.tgd").unwrap();
    Entry::from_target("np.places.tgd", target).unwrap();
    assert_eq!(t.target_name("np.places.tgd").unwrap().len(), 64 + 1 + 13);
    let _ = RoleName::Root;
}
