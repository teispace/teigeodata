#![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "test code")]

use alloc::collections::BTreeMap;
use alloc::string::ToString;
use alloc::vec::Vec;

use ed25519_dalek::SigningKey;

use super::{
    RoleKeys, TargetFile, add_signature, key_id, root, sign, snapshot, target_hashes, targets,
    timestamp,
};
use crate::trust::{Error, RoleName, Trusted};

const NOW: i64 = 1_794_268_800; // 2026-11-10
const YEAR: i64 = 365 * 86_400;
const PACK: &[u8] = b"a pack's bytes";

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn public(seed: u8) -> [u8; 32] {
    key(seed).verifying_key().to_bytes()
}

fn roles(root_keys: &[u8], ts: u8) -> [RoleKeys; 4] {
    let one = |s: u8| RoleKeys {
        keys: alloc::vec![public(s)],
        threshold: 1,
    };
    [
        RoleKeys {
            keys: root_keys.iter().map(|&s| public(s)).collect(),
            threshold: 2,
        },
        one(ts),
        one(20),
        one(30),
    ]
}

/// The chain for one release: targets, snapshot and timestamp at `version`.
fn release(version: u64) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let mut files = BTreeMap::new();
    files.insert(
        "np.places.tgd".to_string(),
        TargetFile {
            length: PACK.len() as u64,
            hashes: target_hashes(PACK),
            custom: None,
        },
    );
    let tg = sign(&targets(version, NOW + YEAR, &files).unwrap(), &[&key(30)]).unwrap();
    let sn = sign(
        &snapshot(version, NOW + YEAR, &tg, version).unwrap(),
        &[&key(20)],
    )
    .unwrap();
    let ts = sign(
        &timestamp(version, NOW + 14 * 86_400, &sn, version).unwrap(),
        &[&key(10)],
    )
    .unwrap();
    (tg, sn, ts)
}

#[test]
fn key_ids_are_securesystemslib_s() {
    // securesystemslib 1.6.0: SSlibKey.from_crypto(Ed25519 public key of seed [7; 32]).keyid
    assert_eq!(
        key_id(&public(7)),
        "bfe1a83869e5bf574e3ff3acbb83f840de84a990d10059f3096f0bf4f2b90c10"
    );
}

#[test]
fn what_the_writer_signs_the_client_verifies() {
    let root1 = sign(
        &root(1, NOW + YEAR, &roles(&[1, 2, 3], 10)).unwrap(),
        &[&key(1), &key(2)],
    )
    .unwrap();
    let (tg, sn, ts) = release(1);
    let mut t = Trusted::new(&root1, NOW).unwrap();
    t.update_timestamp(&ts).unwrap();
    t.update_snapshot(&sn, false).unwrap();
    t.update_targets(&tg).unwrap();
    t.verify_target("np.places.tgd", PACK).unwrap();
    assert_eq!(
        t.verify_target("np.places.tgd", b"a pack.s bytes"),
        Err(Error::Hash)
    );
}

/// The rotation drill: root 2 replaces root key 3 with 4 and the timestamp key 10 with 11,
/// signed by two old and two new root keys; a client holding root 1 walks to it, sees the
/// timestamp key changed, and trusts only timestamps of the new key.
#[test]
fn the_rotation_drill() {
    let root1 = sign(
        &root(1, NOW + YEAR, &roles(&[1, 2, 3], 10)).unwrap(),
        &[&key(1), &key(2)],
    )
    .unwrap();
    let unsigned = sign(&root(2, NOW + YEAR, &roles(&[1, 2, 4], 11)).unwrap(), &[]).unwrap();
    // Signatures arrive one ceremony at a time; a signer may sign twice.
    let mut root2 = unsigned;
    for k in [1, 4, 1, 2] {
        root2 = add_signature(&root2, RoleName::Root, &key(k)).unwrap();
    }
    let mut t = Trusted::new(&root1, NOW).unwrap();
    // Only the old keys' signatures: 1 and 2 are in both roots, so it verifies; only the new
    // key 4 with key 3 missing would not.
    t.update_root(&root2).unwrap();
    assert!(t.keys_rotated());
    let (tg, sn, _) = release(1);
    let old = sign(&timestamp(1, NOW + 86_400, &sn, 1).unwrap(), &[&key(10)]).unwrap();
    assert_eq!(
        t.update_timestamp(&old),
        Err(Error::Signature(RoleName::Timestamp))
    );
    let new = sign(&timestamp(1, NOW + 86_400, &sn, 1).unwrap(), &[&key(11)]).unwrap();
    t.update_timestamp(&new).unwrap();
    t.update_snapshot(&sn, false).unwrap();
    t.update_targets(&tg).unwrap();

    // A root 3 signed only by the removed key 3 and one current key fails root 2's threshold.
    let root3 = sign(
        &root(3, NOW + YEAR, &roles(&[1, 2, 4], 11)).unwrap(),
        &[&key(3), &key(1)],
    )
    .unwrap();
    let mut t = Trusted::new(&root1, NOW).unwrap();
    t.update_root(&root2).unwrap();
    assert_eq!(t.update_root(&root3), Err(Error::Signature(RoleName::Root)));
}

#[test]
fn bad_roots_are_not_written() {
    let mut r = roles(&[1, 2, 3], 10);
    r[0].threshold = 4;
    assert!(root(1, NOW, &r).is_err());
    r[0].threshold = 0;
    assert!(root(1, NOW, &r).is_err());
    let r = roles(&[1, 1, 3], 10);
    assert!(root(1, NOW, &r).is_err());
    assert!(root(1, 253_402_300_800, &roles(&[1, 2, 3], 10)).is_err());
}

#[test]
fn a_signature_is_added_once_per_key_and_never_to_garbage() {
    let unsigned = sign(&root(1, NOW + YEAR, &roles(&[1, 2, 3], 10)).unwrap(), &[]).unwrap();
    let twice = add_signature(
        &add_signature(&unsigned, RoleName::Root, &key(1)).unwrap(),
        RoleName::Root,
        &key(1),
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&twice).unwrap();
    assert_eq!(v["signatures"].as_array().unwrap().len(), 1);
    // One signature is below the threshold of 2.
    assert_eq!(
        Trusted::new(&twice, NOW).err(),
        Some(Error::Signature(RoleName::Root))
    );
    assert!(add_signature(b"{}", RoleName::Root, &key(1)).is_err());
    // A timestamp is not a root.
    let (_, _, ts) = release(1);
    assert!(add_signature(&ts, RoleName::Root, &key(1)).is_err());
}
