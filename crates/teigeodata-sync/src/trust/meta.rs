//! The four top-level TUF roles parsed from their JSON envelopes, with their signatures checked
//! against a delegating root. Strict where a lax reading would let a forgery through, lenient
//! where the specification is: unknown fields are kept (they are signed) and ignored.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use ed25519_dalek::{Signature, VerifyingKey};
use serde_json::{Map, Value};

use super::{Error, cjson, time};

/// The roles of the root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleName {
    /// Signs the keys of every role.
    Root,
    /// Signs the snapshot's version and hash, short-lived.
    Timestamp,
    /// Signs the version of every targets metadata file.
    Snapshot,
    /// Signs the target files: length, hashes, custom data.
    Targets,
}

impl RoleName {
    /// The name in metadata (`_type`, the `roles` key).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Timestamp => "timestamp",
            Self::Snapshot => "snapshot",
            Self::Targets => "targets",
        }
    }
}

/// A role's keys and how many must sign.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Role {
    /// Key IDs, as listed.
    pub keyids: Vec<String>,
    /// Distinct keys that must sign, at least 1.
    pub threshold: usize,
}

/// A hash of a file: algorithm and digest. Known: `sha256`, `sha512`, `blake3`.
pub type Hashes = BTreeMap<String, Vec<u8>>;

/// What a timestamp or snapshot says of another metadata file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetaFile {
    /// Its version.
    pub version: u64,
    /// Its length in bytes, when stated.
    pub length: Option<u64>,
    /// Its hashes, possibly none.
    pub hashes: Hashes,
}

/// A target file.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    /// Its exact length.
    pub length: u64,
    /// Its hashes: at least one known algorithm.
    pub hashes: Hashes,
    /// The repository's own data (the catalog's pack details).
    pub custom: Option<Value>,
}

/// The fields every role has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Common {
    /// Its version, at least 1.
    pub version: u64,
    /// When it expires, Unix seconds.
    pub expires: i64,
}

/// Root metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    /// Version and expiry.
    pub common: Common,
    /// Whether files are named by version and hash.
    pub consistent_snapshot: bool,
    /// Ed25519 public keys by key ID.
    pub keys: BTreeMap<String, [u8; 32]>,
    /// Root, timestamp, snapshot and targets, in that order.
    pub roles: [Role; 4],
}

impl Root {
    /// The keys and threshold of `role`.
    #[must_use]
    pub fn role(&self, role: RoleName) -> &Role {
        let [root, timestamp, snapshot, targets] = &self.roles;
        match role {
            RoleName::Root => root,
            RoleName::Timestamp => timestamp,
            RoleName::Snapshot => snapshot,
            RoleName::Targets => targets,
        }
    }

    /// The public keys of `role`, as a set: a rotation compares them.
    #[must_use]
    pub fn keys_of(&self, role: RoleName) -> BTreeSet<[u8; 32]> {
        self.role(role)
            .keyids
            .iter()
            .filter_map(|id| self.keys.get(id).copied())
            .collect()
    }
}

/// Timestamp metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timestamp {
    /// Version and expiry.
    pub common: Common,
    /// What it says of `snapshot.json`.
    pub snapshot: MetaFile,
}

/// Snapshot metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Version and expiry.
    pub common: Common,
    /// What it says of each targets metadata file (`targets.json`).
    pub meta: BTreeMap<String, MetaFile>,
}

/// Targets metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct Targets {
    /// Version and expiry.
    pub common: Common,
    /// The target files by path.
    pub targets: BTreeMap<String, Target>,
    /// The roles it delegates to, if any (only the top-level targets may delegate, D-175).
    pub delegations: Option<Delegations>,
}

/// The roles a targets file delegates to (spec 1.0 §4.5), one level deep: their keys, and the
/// roles in the order a client consults them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delegations {
    /// Ed25519 public keys by key ID.
    pub keys: BTreeMap<String, [u8; 32]>,
    /// The delegated roles, in order.
    pub roles: Vec<Delegated>,
}

/// One delegated targets role.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delegated {
    /// Its name: the metadata file is `NAME.json` (`V.NAME.json` with consistent snapshots).
    pub name: String,
    /// Its keys and threshold.
    pub role: Role,
    /// The target paths it may sign: shell-style patterns, `*` and `?`, matched one path
    /// segment at a time (`*` never crosses a `/`), as python-tuf matches them.
    pub paths: Vec<String>,
    /// Whether a target it matches is never looked for in a later role.
    pub terminating: bool,
}

impl Delegated {
    /// Whether `path` is one this role may sign.
    #[must_use]
    pub fn matches(&self, path: &str) -> bool {
        self.paths.iter().any(|p| path_matches(p, path))
    }
}

impl Delegations {
    /// The roles to consult for `path`, in order: every role that matches it, up to and
    /// including the first terminating one.
    pub fn matching<'a, 'p>(
        &'a self,
        path: &'p str,
    ) -> impl Iterator<Item = &'a Delegated> + use<'a, 'p> {
        let mut done = false;
        self.roles.iter().filter(move |r| {
            if done || !r.matches(path) {
                return false;
            }
            done = r.terminating;
            true
        })
    }

    /// The role named `name`.
    #[must_use]
    pub fn role(&self, name: &str) -> Option<&Delegated> {
        self.roles.iter().find(|r| r.name == name)
    }
}

/// Whether `path` matches `pattern`: the same number of `/`-separated segments, each matched
/// by a shell-style glob where `*` is any run of characters and `?` exactly one.
#[must_use]
pub fn path_matches(pattern: &str, path: &str) -> bool {
    pattern.split('/').count() == path.split('/').count()
        && pattern
            .split('/')
            .zip(path.split('/'))
            .all(|(p, s)| glob(p.as_bytes(), s.as_bytes()))
}

/// The length of the UTF-8 character that starts with byte `b`.
const fn char_len(b: u8) -> usize {
    match b {
        0xF0..=0xFF => 4,
        0xE0..=0xEF => 3,
        0xC0..=0xDF => 2,
        _ => 1,
    }
}

/// A glob match with one backtracking point (the last `*`): linear in practice, never
/// exponential.
fn glob(p: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while let Some(&c) = s.get(si) {
        match p.get(pi) {
            Some(b'*') => {
                star = Some((pi, si));
                pi += 1;
                continue;
            }
            Some(b'?') => {
                pi += 1;
                si += char_len(c);
                continue;
            }
            Some(&q) if q == c => {
                pi += 1;
                si += 1;
                continue;
            }
            _ => {}
        }
        let Some((sp, ss)) = star else {
            return false;
        };
        let next = ss + s.get(ss).map_or(1, |&b| char_len(b));
        star = Some((sp, next));
        pi = sp + 1;
        si = next;
    }
    p.get(pi..)
        .is_some_and(|rest| rest.iter().all(|&c| c == b'*'))
}

/// A parsed envelope: the `signed` object, its canonical bytes and the signatures.
#[derive(Debug)]
pub struct Envelope {
    signed: Map<String, Value>,
    canonical: Vec<u8>,
    signatures: Vec<(String, Vec<u8>)>,
}

impl Envelope {
    /// Parses `{"signed": …, "signatures": […]}` and checks `_type` and `spec_version`.
    ///
    /// # Errors
    /// [`Error::Format`] for anything else.
    pub fn parse(data: &[u8], role: RoleName) -> Result<Self, Error> {
        let v: Value = serde_json::from_slice(data).map_err(|_| Error::Format("not JSON"))?;
        let Value::Object(mut top) = v else {
            return Err(Error::Format("not an object"));
        };
        let Some(Value::Object(signed)) = top.remove("signed") else {
            return Err(Error::Format("no signed object"));
        };
        let Some(Value::Array(sigs)) = top.remove("signatures") else {
            return Err(Error::Format("no signatures"));
        };
        if str_field(&signed, "_type")? != role.as_str() {
            return Err(Error::Format("the wrong _type"));
        }
        // Spec 1.0.x: the major version must be one this client implements.
        let spec = str_field(&signed, "spec_version")?;
        if spec.split('.').next() != Some("1") {
            return Err(Error::Format("an unsupported spec_version"));
        }
        let mut signatures = Vec::with_capacity(sigs.len());
        for s in sigs {
            let Value::Object(s) = s else {
                return Err(Error::Format("a signature that is not an object"));
            };
            let keyid = str_field(&s, "keyid")?.to_string();
            let sig = hex(str_field(&s, "sig")?)?;
            signatures.push((keyid, sig));
        }
        let canonical = cjson::encode(&Value::Object(signed.clone()))?;
        Ok(Self {
            signed,
            canonical,
            signatures,
        })
    }

    /// Checks that `threshold` distinct keys of `role` in `root` signed. A key listed under
    /// two IDs counts once, and a signature that does not verify is ignored (spec 5.3.4).
    ///
    /// # Errors
    /// [`Error::Signature`] below the threshold.
    pub fn verify(&self, root: &Root, role: RoleName) -> Result<(), Error> {
        if self.signed_by(&root.keys, root.role(role)) {
            Ok(())
        } else {
            Err(Error::Signature(role))
        }
    }

    /// Checks that the delegated role `role` of `delegations` signed, as [`Self::verify`] does.
    ///
    /// # Errors
    /// [`Error::Signature`] (of the targets role) below the threshold.
    pub fn verify_delegated(
        &self,
        delegations: &Delegations,
        role: &Delegated,
    ) -> Result<(), Error> {
        if self.signed_by(&delegations.keys, &role.role) {
            Ok(())
        } else {
            Err(Error::Signature(RoleName::Targets))
        }
    }

    fn signed_by(&self, keys: &BTreeMap<String, [u8; 32]>, r: &Role) -> bool {
        let mut good: BTreeSet<[u8; 32]> = BTreeSet::new();
        for id in &r.keyids {
            let Some(public) = keys.get(id) else {
                continue;
            };
            let Ok(key) = VerifyingKey::from_bytes(public) else {
                continue;
            };
            let signed = self.signatures.iter().any(|(kid, sig)| {
                kid == id
                    && Signature::from_slice(sig)
                        .is_ok_and(|s| key.verify_strict(&self.canonical, &s).is_ok())
            });
            if signed {
                good.insert(*public);
            }
        }
        good.len() >= r.threshold
    }

    fn common(&self) -> Result<Common, Error> {
        let version = u64_field(&self.signed, "version")?;
        if version == 0 {
            return Err(Error::Format("version 0"));
        }
        Ok(Common {
            version,
            expires: time::parse(str_field(&self.signed, "expires")?)?,
        })
    }

    /// The root it holds.
    ///
    /// # Errors
    /// [`Error::Format`] or [`Error::Field`] for a malformed root.
    pub fn root(&self) -> Result<Root, Error> {
        let common = self.common()?;
        let consistent_snapshot = match self.signed.get("consistent_snapshot") {
            Some(Value::Bool(b)) => *b,
            None => false,
            Some(_) => return Err(Error::Format("consistent_snapshot is not a boolean")),
        };
        let keys = keys(obj_field(&self.signed, "keys")?)?;
        let roles_obj = obj_field(&self.signed, "roles")?;
        let role = |name: RoleName| role(obj_field(roles_obj, name.as_str())?);
        Ok(Root {
            common,
            consistent_snapshot,
            keys,
            roles: [
                role(RoleName::Root)?,
                role(RoleName::Timestamp)?,
                role(RoleName::Snapshot)?,
                role(RoleName::Targets)?,
            ],
        })
    }

    /// The timestamp it holds.
    ///
    /// # Errors
    /// [`Error::Format`] or [`Error::Field`] for a malformed timestamp.
    pub fn timestamp(&self) -> Result<Timestamp, Error> {
        let common = self.common()?;
        let meta = obj_field(&self.signed, "meta")?;
        let snapshot = meta_file(
            meta.get("snapshot.json")
                .ok_or(Error::Format("a timestamp without snapshot.json"))?,
        )?;
        Ok(Timestamp { common, snapshot })
    }

    /// The snapshot it holds.
    ///
    /// # Errors
    /// [`Error::Format`] or [`Error::Field`] for a malformed snapshot.
    pub fn snapshot(&self) -> Result<Snapshot, Error> {
        let common = self.common()?;
        let meta = obj_field(&self.signed, "meta")?
            .iter()
            .map(|(k, v)| Ok((k.clone(), meta_file(v)?)))
            .collect::<Result<_, Error>>()?;
        Ok(Snapshot { common, meta })
    }

    /// The targets it holds, with the roles it delegates to (`None` when it delegates to none).
    ///
    /// # Errors
    /// [`Error::Format`] or [`Error::Field`] for malformed targets or delegations.
    pub fn targets(&self) -> Result<Targets, Error> {
        let common = self.common()?;
        let delegations = match self.signed.get("delegations") {
            None => None,
            Some(Value::Object(d)) => delegations(d)?,
            Some(_) => return Err(Error::Format("delegations that are not an object")),
        };
        let mut targets = BTreeMap::new();
        for (path, t) in obj_field(&self.signed, "targets")? {
            let Value::Object(t) = t else {
                return Err(Error::Format("a target that is not an object"));
            };
            let hashes = hashes(t.get("hashes"))?;
            if !hashes.keys().any(|a| known(a)) {
                return Err(Error::Format("a target without a known hash"));
            }
            targets.insert(
                path.clone(),
                Target {
                    length: u64_field(t, "length")?,
                    hashes,
                    custom: t.get("custom").cloned(),
                },
            );
        }
        Ok(Targets {
            common,
            targets,
            delegations,
        })
    }
}

/// Ed25519 public keys by ID. Only Ed25519 keys are trusted; another type is ignored, as an
/// unknown key would be: it can never count towards a threshold.
fn keys(obj: &Map<String, Value>) -> Result<BTreeMap<String, [u8; 32]>, Error> {
    let mut keys = BTreeMap::new();
    for (id, k) in obj {
        let Value::Object(k) = k else {
            return Err(Error::Format("a key that is not an object"));
        };
        if str_field(k, "keytype")? != "ed25519" || str_field(k, "scheme")? != "ed25519" {
            continue;
        }
        let public = hex(str_field(obj_field(k, "keyval")?, "public")?)?;
        let public: [u8; 32] = public
            .try_into()
            .map_err(|_| Error::Format("an Ed25519 key that is not 32 bytes"))?;
        keys.insert(id.clone(), public);
    }
    Ok(keys)
}

/// A role's key IDs and threshold (at least 1).
fn role(r: &Map<String, Value>) -> Result<Role, Error> {
    let threshold = usize::try_from(u64_field(r, "threshold")?)
        .map_err(|_| Error::Format("a threshold out of range"))?;
    if threshold == 0 {
        return Err(Error::Format("a threshold of 0"));
    }
    let Some(Value::Array(ids)) = r.get("keyids") else {
        return Err(Error::Format("a role without keyids"));
    };
    let keyids = ids
        .iter()
        .map(|v| {
            v.as_str()
                .map(ToString::to_string)
                .ok_or(Error::Format("a key ID that is not a string"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Role { keyids, threshold })
}

/// Whether `name` may name a delegated role: it becomes a file name, so it is 1 to 64 of
/// `A-Z a-z 0-9 - _ .`, does not start with a dot, and is none of the top-level roles.
#[must_use]
pub fn role_name_ok(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && !["root", "timestamp", "snapshot", "targets"].contains(&name)
}

/// The `delegations` object: `None` when it lists no roles. Only `paths` patterns are read:
/// a role with `path_hash_prefixes` (hash bins, TAP 15) is refused rather than misread.
fn delegations(d: &Map<String, Value>) -> Result<Option<Delegations>, Error> {
    let Some(Value::Array(list)) = d.get("roles") else {
        return Err(Error::Field("roles"));
    };
    if list.is_empty() {
        return Ok(None);
    }
    let keys = keys(obj_field(d, "keys")?)?;
    let mut roles: Vec<Delegated> = Vec::with_capacity(list.len());
    for r in list {
        let Value::Object(r) = r else {
            return Err(Error::Format("a delegated role that is not an object"));
        };
        let name = str_field(r, "name")?;
        if !role_name_ok(name) {
            return Err(Error::Format(
                "a delegated role name that is not a safe file name",
            ));
        }
        if roles.iter().any(|o| o.name == name) {
            return Err(Error::Format("two delegated roles of one name"));
        }
        if r.contains_key("path_hash_prefixes") {
            return Err(Error::Format("path_hash_prefixes are not supported"));
        }
        let Some(Value::Array(ps)) = r.get("paths") else {
            return Err(Error::Field("paths"));
        };
        let paths = ps
            .iter()
            .map(|p| match p.as_str() {
                Some(p) if !p.is_empty() && !p.contains(['[', ']', '\\']) => Ok(p.to_string()),
                _ => Err(Error::Format("a path pattern that is not a plain glob")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let terminating = match r.get("terminating") {
            None => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err(Error::Format("terminating is not a boolean")),
        };
        roles.push(Delegated {
            name: name.to_string(),
            role: role(r)?,
            paths,
            terminating,
        });
    }
    Ok(Some(Delegations { keys, roles }))
}

/// Whether the client can check this hash algorithm.
#[must_use]
pub fn known(algorithm: &str) -> bool {
    matches!(algorithm, "sha256" | "sha512" | "blake3")
}

/// Checks `data` against every known hash in `hashes` (an unknown algorithm is ignored); with
/// `required`, at least one must be known.
///
/// # Errors
/// [`Error::Hash`] for a mismatch, or no known algorithm when `required`.
pub fn check_hashes(data: &[u8], hashes: &Hashes, required: bool) -> Result<(), Error> {
    use sha2::Digest;
    let mut checked = false;
    for (algorithm, want) in hashes {
        let got: Vec<u8> = match algorithm.as_str() {
            "sha256" => sha2::Sha256::digest(data).to_vec(),
            "sha512" => sha2::Sha512::digest(data).to_vec(),
            "blake3" => blake3::hash(data).as_bytes().to_vec(),
            _ => continue,
        };
        if &got != want {
            return Err(Error::Hash);
        }
        checked = true;
    }
    if required && !checked {
        return Err(Error::Hash);
    }
    Ok(())
}

fn meta_file(v: &Value) -> Result<MetaFile, Error> {
    let Value::Object(m) = v else {
        return Err(Error::Format("meta that is not an object"));
    };
    let version = u64_field(m, "version")?;
    if version == 0 {
        return Err(Error::Format("version 0"));
    }
    let length = match m.get("length") {
        None => None,
        Some(l) => Some(
            l.as_u64()
                .ok_or(Error::Format("a length that is not a count"))?,
        ),
    };
    Ok(MetaFile {
        version,
        length,
        hashes: hashes(m.get("hashes"))?,
    })
}

fn hashes(v: Option<&Value>) -> Result<Hashes, Error> {
    let Some(v) = v else {
        return Ok(Hashes::new());
    };
    let Value::Object(h) = v else {
        return Err(Error::Format("hashes that are not an object"));
    };
    h.iter()
        .map(|(k, d)| {
            let d = d
                .as_str()
                .ok_or(Error::Format("a hash that is not a string"))?;
            Ok((k.clone(), hex(d)?))
        })
        .collect()
}

fn str_field<'a>(m: &'a Map<String, Value>, k: &'static str) -> Result<&'a str, Error> {
    m.get(k).and_then(Value::as_str).ok_or(Error::Field(k))
}

fn u64_field(m: &Map<String, Value>, k: &'static str) -> Result<u64, Error> {
    m.get(k).and_then(Value::as_u64).ok_or(Error::Field(k))
}

fn obj_field<'a>(
    m: &'a Map<String, Value>,
    k: &'static str,
) -> Result<&'a Map<String, Value>, Error> {
    m.get(k).and_then(Value::as_object).ok_or(Error::Field(k))
}

/// Lower- or upper-case hex.
fn hex(s: &str) -> Result<Vec<u8>, Error> {
    let b = s.as_bytes();
    if !b.len().is_multiple_of(2) {
        return Err(Error::Format("odd-length hex"));
    }
    let nib = |c: u8| match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(Error::Format("not hex")),
    };
    b.as_chunks::<2>()
        .0
        .iter()
        .map(|&[h, l]| Ok((nib(h)? << 4) | nib(l)?))
        .collect()
}
