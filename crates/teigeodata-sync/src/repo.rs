//! The repository side of the catalog: writes and signs the four TUF roles that
//! [`trust`](crate::trust) verifies, with the same canonical JSON (D-162), and the catalog's
//! delegated roles, one per country (D-175, [`by_country`]).
//!
//! Every function is pure: the caller reads and writes files and holds the keys. Key IDs are
//! computed as `securesystemslib` computes them (SHA-256 of the canonical key object), so
//! python-tuf reads what this writes. Metadata hashes are SHA-256, which every TUF client knows;
//! targets carry whatever the caller gives ([`target_hashes`]: SHA-256 only, D-162).

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use ed25519_dalek::{Signer as _, SigningKey};
use serde_json::{Map, Value, json};
use sha2::Digest as _;

use crate::catalog::country_of;
use crate::trust::{
    Envelope, Error, Hashes, RoleName, canonical_json, format_time, lower_hex, role_name_ok,
};

/// The TUF specification version written into every role.
pub const SPEC_VERSION: &str = "1.0.31";

/// The key object of an Ed25519 public key, as roots list it.
#[must_use]
pub fn key_object(public: &[u8; 32]) -> Value {
    json!({"keytype": "ed25519", "scheme": "ed25519", "keyval": {"public": lower_hex(public)}})
}

/// The key ID of an Ed25519 public key: SHA-256 of its canonical key object, in hex.
#[must_use]
pub fn key_id(public: &[u8; 32]) -> String {
    // A key object has no floats, so encoding cannot fail.
    let bytes = canonical_json(&key_object(public)).unwrap_or_default();
    lower_hex(&sha2::Sha256::digest(&bytes))
}

/// One role of a root: its public keys and how many of them must sign.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleKeys {
    /// The public keys.
    pub keys: Vec<[u8; 32]>,
    /// How many distinct keys must sign, at least 1 and at most `keys.len()`.
    pub threshold: usize,
}

/// The `signed` object of a root with consistent snapshots.
///
/// # Errors
/// [`Error::Format`] for a threshold of 0 or above the role's keys, a key listed twice, or an
/// expiry outside years 0–9999.
pub fn root(version: u64, expires: i64, roles: &[RoleKeys; 4]) -> Result<Value, Error> {
    let mut keys = Map::new();
    let mut role_map = Map::new();
    let names = [
        RoleName::Root,
        RoleName::Timestamp,
        RoleName::Snapshot,
        RoleName::Targets,
    ];
    for (name, r) in names.iter().zip(roles) {
        role_map.insert(name.as_str().to_string(), role_value(r, &mut keys)?);
    }
    Ok(json!({
        "_type": "root", "spec_version": SPEC_VERSION, "version": version,
        "expires": format_time(expires)?, "consistent_snapshot": true,
        "keys": keys, "roles": role_map,
    }))
}

/// `{"keyids", "threshold"}` of `r`, its keys added to `keys`.
fn role_value(r: &RoleKeys, keys: &mut Map<String, Value>) -> Result<Value, Error> {
    if r.threshold == 0 || r.threshold > r.keys.len() {
        return Err(Error::Format("a threshold of 0 or above the role's keys"));
    }
    let mut ids: Vec<String> = r.keys.iter().map(key_id).collect();
    ids.sort();
    if ids.windows(2).any(|w| w.first() == w.get(1)) {
        return Err(Error::Format("a key listed twice in one role"));
    }
    for k in &r.keys {
        keys.insert(key_id(k), key_object(k));
    }
    Ok(json!({"keyids": ids, "threshold": r.threshold}))
}

/// What a timestamp or snapshot says of `data` at `version`: version, length and SHA-256.
fn meta_of(version: u64, data: &[u8]) -> Value {
    json!({"version": version, "length": data.len(),
           "hashes": {"sha256": lower_hex(&sha2::Sha256::digest(data))}})
}

/// The `signed` object of a timestamp naming `snapshot` (its bytes) at `snapshot_version`.
///
/// # Errors
/// [`Error::Format`] for an expiry outside years 0–9999.
pub fn timestamp(
    version: u64,
    expires: i64,
    snapshot: &[u8],
    snapshot_version: u64,
) -> Result<Value, Error> {
    Ok(json!({
        "_type": "timestamp", "spec_version": SPEC_VERSION, "version": version,
        "expires": format_time(expires)?,
        "meta": {"snapshot.json": meta_of(snapshot_version, snapshot)},
    }))
}

/// The `signed` object of a snapshot naming `targets` (its bytes) at `targets_version`.
///
/// # Errors
/// [`Error::Format`] for an expiry outside years 0–9999.
pub fn snapshot(
    version: u64,
    expires: i64,
    targets: &[u8],
    targets_version: u64,
) -> Result<Value, Error> {
    snapshot_listing(version, expires, &[("targets", targets, targets_version)])
}

/// The `signed` object of a snapshot naming each of `roles`: the top-level targets
/// (`"targets"`) and every delegated role, as (name, its bytes, its version).
///
/// # Errors
/// [`Error::Format`] for an expiry outside years 0–9999, a name listed twice, or a list
/// without the top-level targets.
pub fn snapshot_listing(
    version: u64,
    expires: i64,
    roles: &[(&str, &[u8], u64)],
) -> Result<Value, Error> {
    let mut meta = Map::new();
    for &(name, data, v) in roles {
        if meta
            .insert(alloc::format!("{name}.json"), meta_of(v, data))
            .is_some()
        {
            return Err(Error::Format("a role listed twice"));
        }
    }
    if !meta.contains_key("targets.json") {
        return Err(Error::Format("a snapshot without targets.json"));
    }
    Ok(json!({
        "_type": "snapshot", "spec_version": SPEC_VERSION, "version": version,
        "expires": format_time(expires)?, "meta": meta,
    }))
}

/// A delegated targets role, as the top-level targets list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelegatedRole {
    /// Its name ([`role_name_ok`]).
    pub name: String,
    /// Its keys and threshold.
    pub keys: RoleKeys,
    /// The target path patterns it may sign.
    pub paths: Vec<String>,
    /// Whether a target it matches is never looked for in a later role.
    pub terminating: bool,
}

/// The delegated role of country `cc`: named `cc`, signing `cc.*` and `cc-*` (the country's
/// own packs and those of its parts), terminating, with `keys` (D-175).
#[must_use]
pub fn country_role(cc: [u8; 2], keys: RoleKeys) -> DelegatedRole {
    let name: String = cc.iter().map(|&b| char::from(b)).collect();
    DelegatedRole {
        paths: alloc::vec![alloc::format!("{name}.*"), alloc::format!("{name}-*")],
        name,
        keys,
        terminating: true,
    }
}

/// The catalog split by country (D-175): the files of no country (an address that is not a
/// two-letter code or such a code and a part, `ae-du`), kept by the top-level targets, and
/// each country's files, for its role.
#[must_use]
pub fn by_country(files: Files) -> (Files, BTreeMap<[u8; 2], Files>) {
    let mut top = BTreeMap::new();
    let mut countries: BTreeMap<[u8; 2], Files> = BTreeMap::new();
    for (path, f) in files {
        let address = path.split('.').next().unwrap_or_default();
        match country_of(address) {
            Some(cc) => {
                countries.entry(cc).or_default().insert(path, f);
            }
            None => {
                top.insert(path, f);
            }
        }
    }
    (top, countries)
}

/// The `signed` object of the top-level targets listing `files` and delegating `roles`, in
/// order. With no roles it is [`targets`].
///
/// # Errors
/// As [`targets`]; [`Error::Format`] for a role name that is not a safe file name or is listed
/// twice, a role without patterns, a pattern the client would refuse, or a bad threshold.
pub fn targets_delegating(
    version: u64,
    expires: i64,
    files: &BTreeMap<String, TargetFile>,
    roles: &[DelegatedRole],
) -> Result<Value, Error> {
    let mut v = targets(version, expires, files)?;
    if roles.is_empty() {
        return Ok(v);
    }
    let mut keys = Map::new();
    let mut list = Vec::with_capacity(roles.len());
    let mut names = alloc::collections::BTreeSet::new();
    for r in roles {
        if !role_name_ok(&r.name) || !names.insert(r.name.as_str()) {
            return Err(Error::Format(
                "a delegated role name unsafe or listed twice",
            ));
        }
        if r.paths.is_empty()
            || r.paths
                .iter()
                .any(|p| p.is_empty() || p.contains(['[', ']', '\\']))
        {
            return Err(Error::Format("a delegated role without plain patterns"));
        }
        let Value::Object(mut role) = role_value(&r.keys, &mut keys)? else {
            return Err(Error::Format("a role that is not an object"));
        };
        role.insert("name".to_string(), Value::String(r.name.clone()));
        role.insert("paths".to_string(), json!(r.paths));
        role.insert("terminating".to_string(), Value::Bool(r.terminating));
        list.push(Value::Object(role));
    }
    if let Value::Object(m) = &mut v {
        m.insert(
            "delegations".to_string(),
            json!({"keys": keys, "roles": list}),
        );
    }
    Ok(v)
}

/// One target file: its length, hashes and the repository's `custom` object.
#[derive(Clone, Debug, PartialEq)]
pub struct TargetFile {
    /// Its length in bytes.
    pub length: u64,
    /// Its hashes.
    pub hashes: Hashes,
    /// The catalog entry, as [`crate::catalog::Entry::to_custom`] writes it.
    pub custom: Option<Value>,
}

/// Target files by path.
pub type Files = BTreeMap<String, TargetFile>;

/// The `signed` object of targets listing `files` by path.
///
/// # Errors
/// [`Error::Format`] for an expiry outside years 0–9999 or a target without hashes.
pub fn targets(
    version: u64,
    expires: i64,
    files: &BTreeMap<String, TargetFile>,
) -> Result<Value, Error> {
    let mut out = Map::new();
    for (path, f) in files {
        if f.hashes.is_empty() {
            return Err(Error::Format("a target without hashes"));
        }
        let hashes: Map<String, Value> = f
            .hashes
            .iter()
            .map(|(a, d)| (a.clone(), Value::String(lower_hex(d))))
            .collect();
        let mut t = json!({"length": f.length, "hashes": hashes});
        if let (Some(c), Value::Object(m)) = (&f.custom, &mut t) {
            m.insert("custom".to_string(), c.clone());
        }
        out.insert(path.clone(), t);
    }
    Ok(json!({
        "_type": "targets", "spec_version": SPEC_VERSION, "version": version,
        "expires": format_time(expires)?, "targets": out,
    }))
}

/// An envelope of `signed` with a signature by each of `keys`, as pretty JSON with a final
/// newline (the signatures cover the canonical form, so the layout is free).
///
/// # Errors
/// [`Error::Format`] when `signed` holds a float.
pub fn sign(signed: &Value, keys: &[&SigningKey]) -> Result<Vec<u8>, Error> {
    let bytes = canonical_json(signed)?;
    let signatures: Vec<Value> = keys.iter().map(|k| signature(&bytes, k)).collect();
    write(&json!({"signed": signed, "signatures": signatures}))
}

/// `envelope` with a signature by `key` added, or replaced if `key` had signed it. The envelope
/// is parsed as the client parses it, so a malformed one is refused here.
///
/// # Errors
/// [`Error::Format`] for an envelope the client would refuse.
pub fn add_signature(envelope: &[u8], role: RoleName, key: &SigningKey) -> Result<Vec<u8>, Error> {
    Envelope::parse(envelope, role)?;
    let mut v: Value = serde_json::from_slice(envelope).map_err(|_| Error::Format("not JSON"))?;
    let bytes = canonical_json(v.get("signed").ok_or(Error::Field("signed"))?)?;
    let id = key_id(key.verifying_key().as_bytes());
    let sigs = v
        .get_mut("signatures")
        .and_then(Value::as_array_mut)
        .ok_or(Error::Field("signatures"))?;
    sigs.retain(|s| s.get("keyid").and_then(Value::as_str) != Some(id.as_str()));
    sigs.push(signature(&bytes, key));
    write(&v)
}

fn signature(canonical: &[u8], key: &SigningKey) -> Value {
    json!({"keyid": key_id(key.verifying_key().as_bytes()),
           "sig": lower_hex(&key.sign(canonical).to_bytes())})
}

fn write(v: &Value) -> Result<Vec<u8>, Error> {
    let mut out = serde_json::to_vec_pretty(v).map_err(|_| Error::Format("unwritable JSON"))?;
    out.push(b'\n');
    Ok(out)
}

/// Hashes of a target file: SHA-256 alone. python-tuf refuses a target that lists an algorithm
/// it does not know (`blake3`, measured 2026-11-10), so every TUF client can fetch ours; a
/// pack's sections carry their own BLAKE3 inside the signed head.
#[must_use]
pub fn target_hashes(data: &[u8]) -> Hashes {
    let mut h = Hashes::new();
    h.insert("sha256".to_string(), sha2::Sha256::digest(data).to_vec());
    h
}

#[cfg(test)]
mod tests;
