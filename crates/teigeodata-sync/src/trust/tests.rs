//! The client against a repository built here with fixed keys: the happy path, then each
//! attack of spec §1.5.2 the client must refuse.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: fixtures built here"
)]

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use sha2::Digest;

use super::{Error, RoleName, Trusted, canonical_json};

const NOW: i64 = 1_794_268_800; // 2026-11-10T00:00:00Z
const LATER: &str = "2027-11-10T00:00:00Z";
const PAST: &str = "2026-11-09T00:00:00Z";

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn hexs(b: &[u8]) -> String {
    super::lower_hex(b)
}

fn id(seed: u8) -> String {
    alloc::format!("k{seed}")
}

/// `signed` wrapped and signed by each of `seeds`, under the IDs `k<seed>`.
fn sign(signed: &Value, seeds: &[u8]) -> Vec<u8> {
    let bytes = canonical_json(signed).unwrap();
    let sigs: Vec<Value> = seeds
        .iter()
        .map(|&s| json!({"keyid": id(s), "sig": hexs(&key(s).sign(&bytes).to_bytes())}))
        .collect();
    serde_json::to_vec(&json!({"signed": signed, "signatures": sigs})).unwrap()
}

fn key_obj(seed: u8) -> Value {
    json!({"keytype": "ed25519", "scheme": "ed25519",
           "keyval": {"public": hexs(key(seed).verifying_key().as_bytes())}})
}

/// A root of `version`: root keys `root`, threshold `t`; one key each for the others.
fn root(version: u64, root: &[u8], t: u64, ts: u8, expires: &str) -> Value {
    let mut keys = serde_json::Map::new();
    for &s in root.iter().chain([ts, 20, 30].iter()) {
        keys.insert(id(s), key_obj(s));
    }
    json!({
        "_type": "root", "spec_version": "1.0.31", "version": version, "expires": expires,
        "consistent_snapshot": true, "keys": keys,
        "roles": {
            "root": {"keyids": root.iter().map(|&s| id(s)).collect::<Vec<_>>(), "threshold": t},
            "timestamp": {"keyids": [id(ts)], "threshold": 1},
            "snapshot": {"keyids": [id(20)], "threshold": 1},
            "targets": {"keyids": [id(30)], "threshold": 1},
        }
    })
}

fn meta(version: u64, data: &[u8]) -> Value {
    json!({"version": version, "length": data.len(),
           "hashes": {"sha256": hexs(&sha2::Sha256::digest(data))}})
}

fn timestamp(version: u64, snapshot: &[u8], snapshot_version: u64, expires: &str) -> Vec<u8> {
    sign(
        &json!({"_type": "timestamp", "spec_version": "1.0.31", "version": version,
                "expires": expires, "meta": {"snapshot.json": meta(snapshot_version, snapshot)}}),
        &[10],
    )
}

fn snapshot(version: u64, targets: &[u8], targets_version: u64, expires: &str) -> Vec<u8> {
    sign(
        &json!({"_type": "snapshot", "spec_version": "1.0.31", "version": version,
                "expires": expires, "meta": {"targets.json": meta(targets_version, targets)}}),
        &[20],
    )
}

const PACK: &[u8] = b"a pack's bytes";

fn targets(version: u64, expires: &str) -> Vec<u8> {
    sign(
        &json!({"_type": "targets", "spec_version": "1.0.31", "version": version,
                "expires": expires, "targets": {"np/np-places-main.tgd": {
                    "length": PACK.len(),
                    "hashes": {"blake3": hexs(blake3::hash(PACK).as_bytes())},
                    "custom": {"cc": "np"}}}}),
        &[30],
    )
}

/// Root 1 (keys 1, 2, 3; threshold 2) and a full current chain: timestamp 1, snapshot 1,
/// targets 1.
struct Repo {
    root1: Vec<u8>,
    targets: Vec<u8>,
    snapshot: Vec<u8>,
    timestamp: Vec<u8>,
}

fn repo() -> Repo {
    let targets = targets(1, LATER);
    let snapshot = snapshot(1, &targets, 1, LATER);
    Repo {
        root1: sign(&root(1, &[1, 2, 3], 2, 10, LATER), &[1, 2]),
        timestamp: timestamp(1, &snapshot, 1, LATER),
        snapshot,
        targets,
    }
}

fn loaded(r: &Repo) -> Trusted {
    let mut t = Trusted::new(&r.root1, NOW).unwrap();
    t.update_timestamp(&r.timestamp).unwrap();
    t.update_snapshot(&r.snapshot, false).unwrap();
    t.update_targets(&r.targets).unwrap();
    t
}

#[test]
fn the_whole_chain_verifies_and_names_its_files() {
    let r = repo();
    let t = loaded(&r);
    assert_eq!(t.next_root_name(), "2.root.json");
    assert_eq!(t.snapshot_name().unwrap(), "1.snapshot.json");
    assert_eq!(t.targets_name().unwrap(), "1.targets.json");
    let path = "np/np-places-main.tgd";
    assert_eq!(
        t.target_name(path).unwrap(),
        alloc::format!("np/{}.np-places-main.tgd", blake3::hash(PACK).to_hex())
    );
    assert_eq!(t.target(path).unwrap().custom, Some(json!({"cc": "np"})));
    t.verify_target(path, PACK).unwrap();
    assert_eq!(t.verify_target(path, b"a pack's bytez"), Err(Error::Hash));
    assert_eq!(t.verify_target(path, b"short"), Err(Error::Length));
    assert_eq!(t.target("nope"), Err(Error::NoTarget));
    assert_eq!(t.max_len(RoleName::Targets), r.targets.len() as u64);
}

#[test]
fn a_root_rotates_only_with_both_thresholds_and_the_next_version() {
    let r = repo();
    // Root 2 moves to keys 4, 5, 6 and a new timestamp key 11.
    let new = root(2, &[4, 5, 6], 2, 11, LATER);
    let mut t = Trusted::new(&r.root1, NOW).unwrap();
    assert_eq!(
        t.update_root(&sign(&new, &[4, 5])),
        Err(Error::Signature(RoleName::Root)),
        "the old root did not sign"
    );
    assert_eq!(
        t.update_root(&sign(&new, &[1, 2, 4])),
        Err(Error::Signature(RoleName::Root)),
        "below its own threshold"
    );
    assert_eq!(
        t.update_root(&sign(&root(3, &[4, 5, 6], 2, 11, LATER), &[1, 2, 4, 5])),
        Err(Error::Version(RoleName::Root)),
        "a version skipped"
    );
    t.update_root(&sign(&new, &[1, 2, 4, 5])).unwrap();
    assert_eq!(t.root().common.version, 2);
    assert!(t.keys_rotated(), "the timestamp key changed");
    // The old timestamp key no longer signs.
    assert_eq!(
        t.update_timestamp(&r.timestamp),
        Err(Error::Signature(RoleName::Timestamp))
    );
}

#[test]
fn an_expired_root_may_be_walked_through_but_not_used() {
    let r = repo();
    let mut t = Trusted::new(&sign(&root(1, &[1, 2, 3], 2, 10, PAST), &[1, 2]), NOW).unwrap();
    assert_eq!(
        t.update_timestamp(&r.timestamp),
        Err(Error::Expired(RoleName::Root))
    );
    let mut t = Trusted::new(&sign(&root(1, &[1, 2, 3], 2, 10, PAST), &[1, 2]), NOW).unwrap();
    t.update_root(&sign(&root(2, &[1, 2, 3], 2, 10, LATER), &[1, 2]))
        .unwrap();
    assert!(!t.keys_rotated());
    t.update_timestamp(&r.timestamp).unwrap();
}

#[test]
fn rollbacks_are_refused() {
    let r = repo();
    // The client has seen timestamp 5 naming snapshot 3.
    let snap3 = snapshot(3, &targets(3, LATER), 3, LATER);
    let seen = || {
        let mut t = Trusted::new(&r.root1, NOW).unwrap();
        t.update_timestamp(&timestamp(5, &snap3, 3, LATER)).unwrap();
        t
    };
    assert_eq!(
        seen().update_timestamp(&timestamp(4, &snap3, 3, LATER)),
        Err(Error::Version(RoleName::Timestamp))
    );
    assert_eq!(
        seen().update_timestamp(&timestamp(5, &snap3, 3, LATER)),
        Err(Error::Unchanged)
    );
    assert_eq!(
        seen().update_timestamp(&timestamp(6, &r.snapshot, 2, LATER)),
        Err(Error::Version(RoleName::Snapshot)),
        "a newer timestamp naming an older snapshot"
    );
    seen()
        .update_timestamp(&timestamp(6, &snap3, 3, LATER))
        .unwrap();
}

#[test]
fn a_snapshot_must_keep_every_targets_file_at_its_version_or_higher() {
    let r = repo();
    let snap3 = snapshot(3, &targets(3, LATER), 3, LATER);
    let older = snapshot(4, &targets(2, LATER), 2, LATER);
    let dropped = sign(
        &json!({"_type": "snapshot", "spec_version": "1.0.31", "version": 4,
                "expires": LATER, "meta": {"other.json": {"version": 1}}}),
        &[20],
    );
    let newer = snapshot(4, &targets(4, LATER), 4, LATER);
    for (next, want) in [
        (older, Err(Error::Version(RoleName::Targets))),
        (dropped, Err(Error::Dropped)),
        (newer, Ok(())),
    ] {
        // As a client reloads: the stored timestamp, the new one, the stored snapshot (kept,
        // though no longer the one named), then the new snapshot.
        let mut t = Trusted::new(&r.root1, NOW).unwrap();
        t.update_timestamp(&timestamp(5, &snap3, 3, LATER)).unwrap();
        t.update_timestamp(&timestamp(6, &next, 4, LATER)).unwrap();
        assert_eq!(
            t.update_snapshot(&snap3, true),
            Err(Error::Version(RoleName::Snapshot))
        );
        assert_eq!(t.update_snapshot(&next, false), want);
    }
}

#[test]
fn mix_and_match_is_refused_by_hashes_and_versions() {
    let r = repo();
    let mut t = Trusted::new(&r.root1, NOW).unwrap();
    t.update_timestamp(&r.timestamp).unwrap();
    // Another snapshot of the same version, validly signed: its hash is not the one named.
    let other = snapshot(1, &targets(2, LATER), 2, LATER);
    assert_eq!(t.update_snapshot(&other, false), Err(Error::Hash));
    t.update_snapshot(&r.snapshot, false).unwrap();
    // Targets of another version, validly signed.
    assert_eq!(t.update_targets(&targets(2, LATER)), Err(Error::Hash));
}

#[test]
fn targets_need_the_current_snapshot_and_the_version_it_names() {
    let r = repo();
    // A stored snapshot that a newer timestamp has replaced is kept, but no targets follow it.
    let next = snapshot(2, &r.targets, 1, LATER);
    let mut t = Trusted::new(&r.root1, NOW).unwrap();
    t.update_timestamp(&timestamp(2, &next, 2, LATER)).unwrap();
    assert_eq!(
        t.update_snapshot(&r.snapshot, true),
        Err(Error::Version(RoleName::Snapshot))
    );
    assert_eq!(
        t.update_targets(&r.targets),
        Err(Error::Version(RoleName::Snapshot))
    );
    // A snapshot may name the targets by version alone (no length, no hashes).
    let by_version = sign(
        &json!({"_type": "snapshot", "spec_version": "1.0.31", "version": 1, "expires": LATER,
                "meta": {"targets.json": {"version": 2}}}),
        &[20],
    );
    let mut t = Trusted::new(&r.root1, NOW).unwrap();
    t.update_timestamp(&timestamp(1, &by_version, 1, LATER))
        .unwrap();
    t.update_snapshot(&by_version, false).unwrap();
    assert_eq!(
        t.update_targets(&r.targets),
        Err(Error::Version(RoleName::Targets))
    );
    t.update_targets(&targets(2, LATER)).unwrap();
}

#[test]
fn freeze_is_detected_but_what_was_verified_is_kept() {
    let r = repo();
    let mut t = Trusted::new(&r.root1, NOW).unwrap();
    let stale = timestamp(1, &r.snapshot, 1, PAST);
    assert_eq!(
        t.update_timestamp(&stale),
        Err(Error::Expired(RoleName::Timestamp))
    );
    assert_eq!(
        t.timestamp().unwrap().common.version,
        1,
        "kept for rollback checks"
    );
    assert_eq!(
        t.update_snapshot(&r.snapshot, false),
        Err(Error::Expired(RoleName::Timestamp))
    );
    // An expired targets file is refused and not kept.
    let tg = targets(1, PAST);
    let mut t = Trusted::new(&r.root1, NOW).unwrap();
    let snap = snapshot(1, &tg, 1, LATER);
    t.update_timestamp(&timestamp(1, &snap, 1, LATER)).unwrap();
    t.update_snapshot(&snap, false).unwrap();
    assert_eq!(
        t.update_targets(&tg),
        Err(Error::Expired(RoleName::Targets))
    );
    assert!(t.targets().is_none());
}

#[test]
fn one_key_under_two_ids_counts_once() {
    // Root threshold 2 over IDs k1 and an alias of k1's key.
    let mut r = root(1, &[1, 2, 3], 2, 10, LATER);
    r["keys"]["alias"] = key_obj(1);
    r["roles"]["root"]["keyids"] = json!(["k1", "alias"]);
    let bytes = canonical_json(&r).unwrap();
    let sig = hexs(&key(1).sign(&bytes).to_bytes());
    let env = serde_json::to_vec(&json!({"signed": r, "signatures": [
        {"keyid": "k1", "sig": sig}, {"keyid": "alias", "sig": sig}]}))
    .unwrap();
    assert_eq!(
        Trusted::new(&env, NOW).unwrap_err(),
        Error::Signature(RoleName::Root)
    );
}

#[test]
fn a_changed_unknown_field_breaks_the_signature() {
    let mut r = root(1, &[1, 2, 3], 2, 10, LATER);
    r["x-note"] = json!("signed too");
    let good = sign(&r, &[1, 2]);
    Trusted::new(&good, NOW).unwrap();
    let mut v: Value = serde_json::from_slice(&good).unwrap();
    v["signed"]["x-note"] = json!("changed");
    let bad = serde_json::to_vec(&v).unwrap();
    assert_eq!(
        Trusted::new(&bad, NOW).unwrap_err(),
        Error::Signature(RoleName::Root)
    );
}

#[test]
fn endless_data_is_refused() {
    let r = repo();
    let mut t = Trusted::new(&r.root1, NOW).unwrap();
    let mut big = r.timestamp.clone();
    big.extend(vec![b' '; 20_000]);
    assert_eq!(t.update_timestamp(&big), Err(Error::Length));
    t.update_timestamp(&r.timestamp).unwrap();
    // The snapshot's signed length is exact: one more byte (whitespace) is refused.
    let mut longer = r.snapshot.clone();
    longer.push(b' ');
    assert_eq!(t.update_snapshot(&longer, false), Err(Error::Length));
}

#[test]
fn malformed_metadata_is_refused() {
    let r = repo();
    for bad in [
        &b"not json"[..],
        br#"{"signed": {}, "signatures": []}"#,
        br#"{"signatures": []}"#,
    ] {
        assert!(Trusted::new(bad, NOW).is_err());
    }
    // A timestamp handed in as the root.
    assert!(matches!(
        Trusted::new(&r.timestamp, NOW),
        Err(Error::Format("the wrong _type"))
    ));
    // A spec 2 document.
    let mut v = root(1, &[1, 2, 3], 2, 10, LATER);
    v["spec_version"] = json!("2.0.0");
    assert!(Trusted::new(&sign(&v, &[1, 2]), NOW).is_err());
    // A threshold of 0 would trust anything.
    let mut v = root(1, &[1, 2, 3], 2, 10, LATER);
    v["roles"]["targets"]["threshold"] = json!(0);
    assert!(Trusted::new(&sign(&v, &[1, 2]), NOW).is_err());
}

#[test]
fn calls_out_of_order_are_refused() {
    let r = repo();
    let mut t = Trusted::new(&r.root1, NOW).unwrap();
    assert!(matches!(
        t.update_snapshot(&r.snapshot, false),
        Err(Error::Order(_))
    ));
    assert!(matches!(t.update_targets(&r.targets), Err(Error::Order(_))));
    t.update_timestamp(&r.timestamp).unwrap();
    assert!(matches!(t.update_root(&r.root1), Err(Error::Order(_))));
}
