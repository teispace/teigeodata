//! [`trust`]: the update chain on metadata an attacker shaped (D-331).
//!
//! A valid repository is built with fixed keys (root, targets with one catalog entry, snapshot,
//! timestamp). One role is then attacked: its file replaced by the fuzz bytes, or one value of
//! its `signed` part changed (a number, a string, a type, a key removed or added) and signed
//! again with the role's key, or with a key the root does not list. The roles after it are
//! rebuilt on the attacked file, so their hashes and lengths hold and the checks behind them
//! are reached. The chain is then loaded as a client loads it (`Trusted::new`, then timestamp,
//! snapshot, targets) and the catalog read from what it trusts.
//!
//! Properties: nothing panics; an untouched repository loads whole; and every entry the
//! catalog accepts reads back from its own `custom` value unchanged.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use ed25519_dalek::SigningKey;
use serde_json::{Map, Value};
use sha2::Digest;
use teigeodata_format::component::Component;

use super::Cursor;
use crate::catalog::{self, Catalog, Entry, Size};
use crate::repo::{self, RoleKeys, TargetFile};
use crate::trust::{RoleName, Trusted};

/// 2026-11-10T00:00:00Z, the client's clock.
const NOW: i64 = 1_794_268_800;
/// A year later: every role's expiry.
const EXPIRES: i64 = NOW + 365 * 86_400;
/// The pack asset the one target names (its bytes do not matter, only its hashes).
const ASSET: &[u8] = &[7; 1024];

fn key(role: usize) -> SigningKey {
    SigningKey::from_bytes(&[u8::try_from(role + 1).unwrap_or(1); 32])
}

/// A key no role lists.
fn stranger() -> SigningKey {
    SigningKey::from_bytes(&[0xee; 32])
}

fn entry() -> Entry {
    let sha: [u8; 32] = sha2::Sha256::digest(ASSET).into();
    let mut sizes = BTreeMap::new();
    sizes.insert(
        Component::Point,
        Size {
            wire: 900,
            disk: 256,
        },
    );
    Entry {
        address: "zz".to_string(),
        component: "places".to_string(),
        release: "2026.11.0".to_string(),
        file: catalog::file_name("zz", "places", "2026.11.0", &sha),
        pack_length: 512,
        head: [0xab; 16],
        seek_table: 16,
        head_frame: Size {
            wire: 64,
            disk: 128,
        },
        block_hashes: None,
        sizes,
        parts: Vec::new(),
        deps: Vec::new(),
        min_reader: 0,
        bbox: None,
        licence: "CC-BY-4.0".to_string(),
    }
}

/// One value of `v` changed by the fuzz bytes: a path of up to four steps into objects and
/// arrays, then a new value, a removed key or an added one.
fn edit_value(c: &mut Cursor<'_>, v: &mut Value) {
    // The path is chosen on a shared borrow, then walked on the mutable one.
    let mut path = Vec::new();
    let mut at: &Value = v;
    for _ in 0..(c.u8() % 5) {
        let len = match at {
            Value::Object(m) => m.len(),
            Value::Array(a) => a.len(),
            _ => 0,
        };
        if len == 0 {
            break;
        }
        let i = usize::try_from(c.below(len as u64)).unwrap_or(0);
        let next = match at {
            Value::Object(m) => m.values().nth(i),
            Value::Array(a) => a.get(i),
            _ => None,
        };
        let Some(next) = next else { break };
        path.push(i);
        at = next;
    }
    let node = walk(v, &path);
    let text = |c: &mut Cursor<'_>| String::from_utf8_lossy(c.bytes(24)).into_owned();
    match c.u8() % 10 {
        0 => *node = Value::Null,
        1 => *node = Value::Bool(c.u8() & 1 == 1),
        2 => *node = Value::from(u64::from(c.u32()) << (c.u8() % 33)),
        3 => *node = Value::from(-i64::from(c.u32())),
        4 => *node = Value::from(f64::from(c.u32()) / 7.0),
        5 => *node = Value::String(text(c)),
        6 => *node = Value::Array(vec![node.clone(), node.clone()]),
        7 => *node = Value::Object(Map::new()),
        8 => {
            if let Value::Object(m) = node {
                let k = usize::try_from(c.below(m.len() as u64)).unwrap_or(0);
                if let Some(key) = m.keys().nth(k).cloned() {
                    m.remove(&key);
                }
            }
        }
        _ => {
            if let Value::Object(m) = node {
                m.insert(text(c), Value::from(u64::from(c.u8())));
            }
        }
    }
}

/// The value at `path` (child indices in objects and arrays) of `v`.
fn walk<'v>(v: &'v mut Value, path: &[usize]) -> &'v mut Value {
    let Some((&i, rest)) = path.split_first() else {
        return v;
    };
    let inside = match &*v {
        Value::Object(m) => i < m.len(),
        Value::Array(a) => i < a.len(),
        _ => false,
    };
    if !inside {
        return v;
    }
    match v {
        Value::Object(m) => {
            let child = m.values_mut().nth(i);
            walk(
                child.unwrap_or_else(|| panic!("an index below the length")),
                rest,
            )
        }
        Value::Array(a) => walk(&mut a[i], rest),
        _ => unreachable!("checked above"),
    }
}

/// The repository's files: root, targets, snapshot, timestamp, with role `attack` (0 to 3 in
/// that order; 4 for none) attacked as the module says.
fn files(c: &mut Cursor<'_>, attack: usize) -> Option<[Vec<u8>; 4]> {
    let roles: [RoleKeys; 4] = core::array::from_fn(|r| RoleKeys {
        keys: vec![key(r).verifying_key().to_bytes()],
        threshold: 1,
    });
    // RoleKeys are in root, timestamp, snapshot, targets order; files in build order.
    let signer = |file: usize| match file {
        0 => key(0),
        1 => key(3),
        2 => key(2),
        _ => key(1),
    };
    let mode = c.u8();
    let attacked = |c: &mut Cursor<'_>, file: usize, mut signed: Value| -> Option<Vec<u8>> {
        if file != attack {
            return repo::sign(&signed, &[&signer(file)]).ok();
        }
        match mode % 4 {
            0 => Some(c.bytes(4096).to_vec()),
            1 => {
                edit_value(c, &mut signed);
                repo::sign(&signed, &[&stranger()]).ok()
            }
            _ => {
                edit_value(c, &mut signed);
                if mode & 0x10 != 0 {
                    edit_value(c, &mut signed);
                }
                repo::sign(&signed, &[&signer(file)]).ok()
            }
        }
    };
    let root = attacked(c, 0, repo::root(1, EXPIRES, &roles).ok()?)?;
    let e = entry();
    let mut targets = BTreeMap::new();
    targets.insert(
        e.path(),
        TargetFile {
            length: ASSET.len() as u64,
            hashes: repo::target_hashes(ASSET),
            custom: Some(e.to_custom()),
        },
    );
    let targets = attacked(c, 1, repo::targets(1, EXPIRES, &targets).ok()?)?;
    let snapshot = attacked(c, 2, repo::snapshot(1, EXPIRES, &targets, 1).ok()?)?;
    let timestamp = attacked(c, 3, repo::timestamp(1, EXPIRES, &snapshot, 1).ok()?)?;
    Some([root, targets, snapshot, timestamp])
}

/// The update chain and the catalog on a repository with one role attacked (module docs).
pub fn trust(data: &[u8]) {
    let mut c = Cursor(data);
    let attack = usize::from(c.u8() % 5);
    let Some([root, targets, snapshot, timestamp]) = files(&mut c, attack) else {
        return;
    };
    let whole = attack == 4;
    let loaded = (|| {
        let mut t = Trusted::new(&root, NOW)?;
        t.update_timestamp(&timestamp)?;
        t.update_snapshot(&snapshot, false)?;
        t.update_targets(&targets)?;
        Ok::<_, crate::trust::Error>(t)
    })();
    let t = match loaded {
        Ok(t) => t,
        Err(e) => {
            assert!(!whole, "an untouched repository is refused: {e}");
            return;
        }
    };
    let _ = (
        t.keys_rotated(),
        t.next_root_name(),
        t.snapshot_name(),
        t.targets_name(),
    );
    for role in [
        RoleName::Root,
        RoleName::Timestamp,
        RoleName::Snapshot,
        RoleName::Targets,
    ] {
        let _ = (t.expires(role), t.max_len(role));
    }
    let (cat, refused) = Catalog::trusted(&t);
    assert!(
        !whole || (refused.is_empty() && cat.entries().len() == 1),
        "an untouched catalog loses its entry: {refused:?}"
    );
    for (path, e) in cat.entries() {
        let _ = (
            t.target_name(path),
            t.verify_target(path, ASSET),
            t.roles_for(path),
        );
        let custom = e.to_custom();
        let back = t.target(path).ok().map(|target| crate::trust::Target {
            custom: Some(custom),
            ..target.clone()
        });
        if let Some(target) = back {
            assert_eq!(
                Entry::from_target(path, &target).as_ref(),
                Ok(e),
                "an accepted entry does not read back from its own custom value"
            );
        }
        let _ = cat.packs_at(
            *b"ZZ",
            teigeodata_core::Point::from_units(0, 0).unwrap_or_else(|| panic!("origin")),
        );
    }
}
