//! The catalog's trust chain: a TUF 1.0 client for the four top-level roles and one level of
//! delegated targets roles, matched by `paths` patterns (ADR-0012, D-159, D-175,
//! [offline loading and trust](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/offline-loading-and-trust.md)).
//!
//! Sans-I/O: the caller fetches each file (at most [`Trusted::max_len`] bytes) and hands its
//! bytes in; [`Trusted`] verifies and keeps it, as python-tuf's `TrustedMetadataSet` does. The
//! order is the specification's client workflow (§5):
//!
//! 1. [`Trusted::new`] with the root the app ships (it must sign itself), then
//!    [`Trusted::update_root`] with `N+1.root.json`, `N+2.root.json` … until one is missing;
//! 2. [`Trusted::update_timestamp`] with `timestamp.json`;
//! 3. [`Trusted::update_snapshot`] with the snapshot the timestamp names;
//! 4. [`Trusted::update_targets`] with the targets the snapshot names;
//! 5. [`Trusted::update_delegated`] with each delegated role the app needs (the catalog is
//!    delegated by country, D-175; [`Trusted::roles_for`] names the roles a path needs);
//! 6. [`Trusted::verify_target`] on a downloaded target file.
//!
//! Each verified file is kept by the caller. On the next start the stored timestamp, snapshot
//! and targets are loaded with the same calls (the snapshot with `stored: true`), so a
//! repository can never roll the client back: a lower version is refused. Metadata is kept even
//! when it has expired, and the call then returns [`Error::Expired`]: expiry blocks installs
//! and updates, never reading what is installed (the offline-first rule).
//!
//! Guarded attacks (spec §1.5.2): arbitrary installation and wrong software (signatures, hashes
//! and lengths), endless data (length caps), rollback and fast-forward (persisted versions,
//! [`Trusted::keys_rotated`]), indefinite freeze (expiry), mix-and-match (the snapshot's hash
//! and version in the timestamp, the targets' in the snapshot), key compromise below threshold
//! (distinct keys counted once).

mod cjson;
mod meta;
#[cfg(test)]
mod tests;
mod time;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::{self, Write as _};

pub use cjson::encode as canonical_json;
pub use meta::{
    Common, Delegated, Delegations, Envelope, Hashes, MetaFile, Role, RoleName, Root, Snapshot,
    Target, Targets, Timestamp, check_hashes, known, path_matches, role_name_ok,
};
#[cfg(feature = "repo")]
pub(crate) use time::format as format_time;

/// The largest root, timestamp, snapshot and targets accepted when no signed length is known
/// (python-tuf's defaults).
pub const MAX_ROOT_LEN: u64 = 512_000;
/// See [`MAX_ROOT_LEN`].
pub const MAX_TIMESTAMP_LEN: u64 = 16_384;
/// See [`MAX_ROOT_LEN`].
pub const MAX_SNAPSHOT_LEN: u64 = 2_000_000;
/// See [`MAX_ROOT_LEN`], for the top-level targets and each delegated role: a world catalog of
/// about 5,000 targets in one file is about 11 MB, delegated by country about 3 MB at the top
/// and well under 1 MB for the largest country (D-175).
pub const MAX_TARGETS_LEN: u64 = 5_000_000;
/// The most root versions one update walks (python-tuf's limit).
pub const MAX_ROOT_ROTATIONS: u64 = 256;

/// Why metadata or a target was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// Malformed metadata.
    Format(&'static str),
    /// A required field is missing or of the wrong type.
    Field(&'static str),
    /// Fewer than the threshold of the role's keys signed.
    Signature(RoleName),
    /// A version is not the one required: lower than trusted (rollback), or not the one the
    /// delegating metadata names.
    Version(RoleName),
    /// The same timestamp version as the trusted one: nothing new, the trusted one stays.
    Unchanged,
    /// The role has expired. It was kept (its version still guards against rollback), but no
    /// later role may be loaded and nothing may be installed.
    Expired(RoleName),
    /// Longer than allowed.
    Length,
    /// A hash does not match, or no known hash is given.
    Hash,
    /// The calls came out of order (a root after the timestamp, a snapshot before it).
    Order(&'static str),
    /// The snapshot drops a targets file the trusted one lists.
    Dropped,
    /// No such target.
    NoTarget,
    /// The targets delegate no role of that name, or the snapshot does not list it.
    NoRole,
    /// A delegated role that must be consulted for this path is not loaded yet
    /// ([`Trusted::roles_for`] names it).
    Unloaded,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(p) => write!(f, "malformed metadata: {p}"),
            Self::Field(k) => write!(f, "malformed metadata: field `{k}`"),
            Self::Signature(r) => write!(f, "{} is not signed by its threshold", r.as_str()),
            Self::Version(r) => write!(f, "{} has the wrong version", r.as_str()),
            Self::Unchanged => f.write_str("the timestamp has not changed"),
            Self::Expired(r) => write!(f, "{} has expired", r.as_str()),
            Self::Length => f.write_str("longer than allowed"),
            Self::Hash => f.write_str("a hash does not match"),
            Self::Order(p) => write!(f, "out of order: {p}"),
            Self::Dropped => f.write_str("the snapshot drops a trusted targets file"),
            Self::NoTarget => f.write_str("no such target"),
            Self::NoRole => f.write_str("no such delegated role"),
            Self::Unloaded => f.write_str("a delegated role it needs is not loaded"),
        }
    }
}

impl core::error::Error for Error {}

/// Verified metadata, built up in the specification's order.
#[derive(Clone, Debug)]
pub struct Trusted {
    now: i64,
    first_root: Root,
    root: Root,
    timestamp: Option<Timestamp>,
    snapshot: Option<Snapshot>,
    targets: Option<Targets>,
    /// The delegated roles loaded, by name.
    delegated: BTreeMap<String, Targets>,
}

impl Trusted {
    /// Starts from a trusted root (shipped with the app, or the last one stored), which must be
    /// signed by its own root threshold. Its expiry is checked by [`Self::update_timestamp`],
    /// after the chain has been walked. `now` is Unix seconds, fixed for the whole update.
    ///
    /// # Errors
    /// Malformed, too long, or not signed by itself.
    pub fn new(root: &[u8], now: i64) -> Result<Self, Error> {
        cap(root, MAX_ROOT_LEN)?;
        let env = Envelope::parse(root, RoleName::Root)?;
        let r = env.root()?;
        env.verify(&r, RoleName::Root)?;
        Ok(Self {
            now,
            first_root: r.clone(),
            root: r,
            timestamp: None,
            snapshot: None,
            targets: None,
            delegated: BTreeMap::new(),
        })
    }

    /// The trusted root.
    #[must_use]
    pub const fn root(&self) -> &Root {
        &self.root
    }

    /// The trusted timestamp, if loaded.
    #[must_use]
    pub const fn timestamp(&self) -> Option<&Timestamp> {
        self.timestamp.as_ref()
    }

    /// The trusted snapshot, if loaded.
    #[must_use]
    pub const fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    /// The trusted targets, if loaded.
    #[must_use]
    pub const fn targets(&self) -> Option<&Targets> {
        self.targets.as_ref()
    }

    /// The next root to fetch: `N+1.root.json`.
    #[must_use]
    pub fn next_root_name(&self) -> String {
        alloc::format!("{}.root.json", self.root.common.version + 1)
    }

    /// Takes root `N+1`: signed by a threshold of the trusted root's root keys and of its own,
    /// version exactly one higher. Expiry is not checked here (an old app may walk through
    /// expired roots to the current one).
    ///
    /// # Errors
    /// After the timestamp; malformed, too long, a wrong version or signatures.
    pub fn update_root(&mut self, data: &[u8]) -> Result<(), Error> {
        if self.timestamp.is_some() {
            return Err(Error::Order("a root after the timestamp"));
        }
        if self.root.common.version - self.first_root.common.version >= MAX_ROOT_ROTATIONS {
            return Err(Error::Order("too many root rotations in one update"));
        }
        cap(data, MAX_ROOT_LEN)?;
        let env = Envelope::parse(data, RoleName::Root)?;
        env.verify(&self.root, RoleName::Root)?;
        let new = env.root()?;
        if new.common.version != self.root.common.version + 1 {
            return Err(Error::Version(RoleName::Root));
        }
        env.verify(&new, RoleName::Root)?;
        self.root = new;
        Ok(())
    }

    /// Whether the roots walked in this update changed the timestamp or snapshot keys. If so,
    /// the stored timestamp and snapshot are deleted, not loaded: that is how a repository
    /// recovers from a fast-forward attack (spec 5.3.11).
    #[must_use]
    pub fn keys_rotated(&self) -> bool {
        [RoleName::Timestamp, RoleName::Snapshot]
            .into_iter()
            .any(|r| self.first_root.keys_of(r) != self.root.keys_of(r))
    }

    /// Takes a timestamp: the root must not have expired; signed by the root's timestamp keys;
    /// not older than the trusted timestamp, nor naming an older snapshot. It is kept even when
    /// expired.
    ///
    /// # Errors
    /// [`Error::Unchanged`] when it is the trusted version (nothing to do); [`Error::Expired`]
    /// when it (kept) or the root has expired; any other refusal.
    pub fn update_timestamp(&mut self, data: &[u8]) -> Result<(), Error> {
        if self.snapshot.is_some() {
            return Err(Error::Order("a timestamp after the snapshot"));
        }
        if self.root.common.expires <= self.now {
            return Err(Error::Expired(RoleName::Root));
        }
        cap(data, MAX_TIMESTAMP_LEN)?;
        let env = Envelope::parse(data, RoleName::Timestamp)?;
        env.verify(&self.root, RoleName::Timestamp)?;
        let new = env.timestamp()?;
        if let Some(old) = &self.timestamp {
            if new.common.version < old.common.version {
                return Err(Error::Version(RoleName::Timestamp));
            }
            if new.common.version == old.common.version {
                return Err(Error::Unchanged);
            }
            if new.snapshot.version < old.snapshot.version {
                return Err(Error::Version(RoleName::Snapshot));
            }
        }
        let expires = new.common.expires;
        self.timestamp = Some(new);
        if expires <= self.now {
            return Err(Error::Expired(RoleName::Timestamp));
        }
        Ok(())
    }

    /// The most bytes to fetch for `role`: its signed length when the delegating metadata
    /// states one, else the default cap.
    #[must_use]
    pub fn max_len(&self, role: RoleName) -> u64 {
        let signed = match role {
            RoleName::Root | RoleName::Timestamp => None,
            RoleName::Snapshot => self.timestamp.as_ref().and_then(|t| t.snapshot.length),
            RoleName::Targets => self
                .snapshot
                .as_ref()
                .and_then(|s| s.meta.get("targets.json"))
                .and_then(|m| m.length),
        };
        signed.unwrap_or(match role {
            RoleName::Root => MAX_ROOT_LEN,
            RoleName::Timestamp => MAX_TIMESTAMP_LEN,
            RoleName::Snapshot => MAX_SNAPSHOT_LEN,
            RoleName::Targets => MAX_TARGETS_LEN,
        })
    }

    /// The snapshot to fetch: `V.snapshot.json` with consistent snapshots, else `snapshot.json`.
    ///
    /// # Errors
    /// Before the timestamp.
    pub fn snapshot_name(&self) -> Result<String, Error> {
        let t = self
            .timestamp
            .as_ref()
            .ok_or(Error::Order("the snapshot before the timestamp"))?;
        Ok(self.versioned(t.snapshot.version, "snapshot.json"))
    }

    /// The targets to fetch: `V.targets.json` with consistent snapshots, else `targets.json`.
    ///
    /// # Errors
    /// Before the snapshot, or a snapshot without `targets.json`.
    pub fn targets_name(&self) -> Result<String, Error> {
        let m = self.targets_meta()?;
        Ok(self.versioned(m.version, "targets.json"))
    }

    fn versioned(&self, version: u64, name: &str) -> String {
        if self.root.consistent_snapshot {
            alloc::format!("{version}.{name}")
        } else {
            String::from(name)
        }
    }

    fn targets_meta(&self) -> Result<&MetaFile, Error> {
        self.snapshot
            .as_ref()
            .ok_or(Error::Order("the targets before the snapshot"))?
            .meta
            .get("targets.json")
            .ok_or(Error::Format("a snapshot without targets.json"))
    }

    /// Takes a snapshot: the timestamp must not have expired; its length and hashes as the
    /// timestamp says (not for a `stored` copy, which a newer timestamp may have replaced);
    /// signed by the root's snapshot keys; no targets file dropped or older than in the
    /// trusted snapshot. Kept even when expired, or when its version is not yet the
    /// timestamp's (a stored one).
    ///
    /// # Errors
    /// [`Error::Expired`] or [`Error::Version`] when kept but not current; any other refusal.
    pub fn update_snapshot(&mut self, data: &[u8], stored: bool) -> Result<(), Error> {
        if self.targets.is_some() {
            return Err(Error::Order("a snapshot after the targets"));
        }
        let t = self
            .timestamp
            .as_ref()
            .ok_or(Error::Order("the snapshot before the timestamp"))?;
        if t.common.expires <= self.now {
            return Err(Error::Expired(RoleName::Timestamp));
        }
        let want = t.snapshot.clone();
        // A stored copy was checked when it was fetched; the timestamp may now name another.
        if stored {
            cap(data, MAX_SNAPSHOT_LEN)?;
        } else {
            cap(data, want.length.unwrap_or(MAX_SNAPSHOT_LEN))?;
            check_hashes(data, &want.hashes, false)?;
        }
        let env = Envelope::parse(data, RoleName::Snapshot)?;
        env.verify(&self.root, RoleName::Snapshot)?;
        let new = env.snapshot()?;
        if let Some(old) = &self.snapshot {
            for (name, m) in &old.meta {
                let n = new.meta.get(name).ok_or(Error::Dropped)?;
                if n.version < m.version {
                    return Err(Error::Version(RoleName::Targets));
                }
            }
        }
        let (expires, version) = (new.common.expires, new.common.version);
        self.snapshot = Some(new);
        if expires <= self.now {
            return Err(Error::Expired(RoleName::Snapshot));
        }
        if version != want.version {
            return Err(Error::Version(RoleName::Snapshot));
        }
        Ok(())
    }

    /// Takes the targets: the snapshot must be current and not expired; length, hashes and
    /// version as the snapshot says; signed by the root's targets keys; not expired.
    ///
    /// # Errors
    /// Any refusal; nothing is kept.
    pub fn update_targets(&mut self, data: &[u8]) -> Result<(), Error> {
        let (Some(s), Some(t)) = (&self.snapshot, &self.timestamp) else {
            return Err(Error::Order("the targets before the snapshot"));
        };
        if s.common.expires <= self.now {
            return Err(Error::Expired(RoleName::Snapshot));
        }
        if s.common.version != t.snapshot.version {
            return Err(Error::Version(RoleName::Snapshot));
        }
        let want = self.targets_meta()?.clone();
        cap(data, want.length.unwrap_or(MAX_TARGETS_LEN))?;
        check_hashes(data, &want.hashes, false)?;
        let env = Envelope::parse(data, RoleName::Targets)?;
        env.verify(&self.root, RoleName::Targets)?;
        let new = env.targets()?;
        if new.common.version != want.version {
            return Err(Error::Version(RoleName::Targets));
        }
        if new.common.expires <= self.now {
            return Err(Error::Expired(RoleName::Targets));
        }
        // Roles loaded under other targets were delegated by them, not by these.
        self.delegated.clear();
        self.targets = Some(new);
        Ok(())
    }

    /// The roles the trusted targets delegate to, once loaded.
    #[must_use]
    pub fn delegations(&self) -> Option<&Delegations> {
        self.targets.as_ref()?.delegations.as_ref()
    }

    /// The delegated role `name`, once loaded.
    #[must_use]
    pub fn delegated(&self, name: &str) -> Option<&Targets> {
        self.delegated.get(name)
    }

    /// The delegated roles to load to look up `path`, in order: none when the top-level
    /// targets list it, else every role whose patterns match it, up to the first terminating
    /// one. Most apps load only the roles of the countries they need.
    #[must_use]
    pub fn roles_for<'a>(&'a self, path: &str) -> Vec<&'a str> {
        let Some(t) = &self.targets else {
            return Vec::new();
        };
        if t.targets.contains_key(path) {
            return Vec::new();
        }
        t.delegations.as_ref().map_or_else(Vec::new, |d| {
            d.matching(path).map(|r| r.name.as_str()).collect()
        })
    }

    /// What the snapshot says of the delegated role `name`, and the role.
    fn delegated_meta(&self, name: &str) -> Result<(&MetaFile, &Delegated), Error> {
        let t = self
            .targets
            .as_ref()
            .ok_or(Error::Order("a delegated role before the targets"))?;
        let role = t
            .delegations
            .as_ref()
            .and_then(|d| d.role(name))
            .ok_or(Error::NoRole)?;
        let meta = self
            .snapshot
            .as_ref()
            .and_then(|s| s.meta.get(&alloc::format!("{name}.json")))
            .ok_or(Error::NoRole)?;
        Ok((meta, role))
    }

    /// The file of the delegated role `name` to fetch: `V.NAME.json` with consistent
    /// snapshots, else `NAME.json`.
    ///
    /// # Errors
    /// Before the targets, or a role the targets do not delegate or the snapshot does not list.
    pub fn delegated_name(&self, name: &str) -> Result<String, Error> {
        let (m, _) = self.delegated_meta(name)?;
        Ok(self.versioned(m.version, &alloc::format!("{name}.json")))
    }

    /// The most bytes to fetch for the delegated role `name`: its signed length, else
    /// [`MAX_TARGETS_LEN`].
    #[must_use]
    pub fn max_delegated_len(&self, name: &str) -> u64 {
        self.delegated_meta(name)
            .ok()
            .and_then(|(m, _)| m.length)
            .unwrap_or(MAX_TARGETS_LEN)
    }

    /// Takes the delegated role `name`, as [`Self::update_targets`] takes the targets: length,
    /// hashes and version as the snapshot says; signed by the threshold of the keys the
    /// targets delegate to it; not expired; and delegating to no role itself (one level,
    /// D-175). Its targets are trusted only for the paths its patterns match.
    ///
    /// # Errors
    /// Any refusal; nothing is kept.
    pub fn update_delegated(&mut self, name: &str, data: &[u8]) -> Result<(), Error> {
        let (Some(s), Some(t), Some(top)) = (&self.snapshot, &self.timestamp, &self.targets) else {
            return Err(Error::Order("a delegated role before the targets"));
        };
        if s.common.expires <= self.now {
            return Err(Error::Expired(RoleName::Snapshot));
        }
        if s.common.version != t.snapshot.version {
            return Err(Error::Version(RoleName::Snapshot));
        }
        if top.common.expires <= self.now {
            return Err(Error::Expired(RoleName::Targets));
        }
        let (want, role) = self.delegated_meta(name)?;
        cap(data, want.length.unwrap_or(MAX_TARGETS_LEN))?;
        check_hashes(data, &want.hashes, false)?;
        let env = Envelope::parse(data, RoleName::Targets)?;
        let delegations = top.delegations.as_ref().ok_or(Error::NoRole)?;
        env.verify_delegated(delegations, role)?;
        let new = env.targets()?;
        if new.delegations.is_some() {
            return Err(Error::Format("a delegated role that delegates again"));
        }
        if new.common.version != want.version {
            return Err(Error::Version(RoleName::Targets));
        }
        if new.common.expires <= self.now {
            return Err(Error::Expired(RoleName::Targets));
        }
        self.delegated.insert(String::from(name), new);
        Ok(())
    }

    /// When `role`'s trusted metadata expires (Unix seconds), once loaded.
    #[must_use]
    pub fn expires(&self, role: RoleName) -> Option<i64> {
        match role {
            RoleName::Root => Some(self.root.common.expires),
            RoleName::Timestamp => self.timestamp.as_ref().map(|m| m.common.expires),
            RoleName::Snapshot => self.snapshot.as_ref().map(|m| m.common.expires),
            RoleName::Targets => self.targets.as_ref().map(|m| m.common.expires),
        }
    }

    /// The target at `path`: from the top-level targets, else from the first delegated role
    /// whose patterns match it and that lists it, in order, stopping at a terminating one
    /// (spec 1.0 §5.6.7). A role that lists a path its patterns do not match is not trusted
    /// for it.
    ///
    /// # Errors
    /// Before the targets; [`Error::Unloaded`] when a role to consult is not loaded;
    /// [`Error::NoTarget`] when none lists it.
    pub fn target(&self, path: &str) -> Result<&Target, Error> {
        let top = self
            .targets
            .as_ref()
            .ok_or(Error::Order("a target before the targets"))?;
        if let Some(t) = top.targets.get(path) {
            return Ok(t);
        }
        let Some(d) = &top.delegations else {
            return Err(Error::NoTarget);
        };
        for role in d.matching(path) {
            let loaded = self.delegated.get(&role.name).ok_or(Error::Unloaded)?;
            if let Some(t) = loaded.targets.get(path) {
                return Ok(t);
            }
        }
        Err(Error::NoTarget)
    }

    /// Every target the client trusts, by path, with those of the delegated roles loaded so
    /// far: each path once, from the role [`Self::target`] takes it from. A path whose lookup
    /// needs a role not yet loaded is left out.
    pub fn trusted_targets(&self) -> impl Iterator<Item = (&str, &Target)> {
        let top = self.targets.as_ref();
        let own = top.into_iter().flat_map(|t| t.targets.iter());
        let delegated = self
            .delegated
            .values()
            .flat_map(|t| t.targets.iter())
            .filter(|&(p, t)| self.target(p).is_ok_and(|x| core::ptr::eq(x, t)));
        own.chain(delegated).map(|(p, t)| (p.as_str(), t))
    }

    /// The file name of the target at `path`: `dir/HASH.name` with consistent snapshots (the
    /// first known hash in algorithm order), else `path`.
    ///
    /// # Errors
    /// As [`Self::target`].
    pub fn target_name(&self, path: &str) -> Result<String, Error> {
        let t = self.target(path)?;
        if !self.root.consistent_snapshot {
            return Ok(String::from(path));
        }
        let (_, digest) = t.hashes.iter().find(|(a, _)| known(a)).ok_or(Error::Hash)?;
        let hex = lower_hex(digest);
        Ok(match path.rsplit_once('/') {
            Some((dir, name)) => alloc::format!("{dir}/{hex}.{name}"),
            None => alloc::format!("{hex}.{path}"),
        })
    }

    /// Checks a downloaded target: exactly its length, and every known hash.
    ///
    /// # Errors
    /// No such target, a length or a hash that does not match.
    pub fn verify_target(&self, path: &str, data: &[u8]) -> Result<(), Error> {
        let t = self.target(path)?;
        if data.len() as u64 != t.length {
            return Err(Error::Length);
        }
        check_hashes(data, &t.hashes, true)
    }
}

fn cap(data: &[u8], max: u64) -> Result<(), Error> {
    if data.len() as u64 > max {
        Err(Error::Length)
    } else {
        Ok(())
    }
}

/// Lower-case hex of `bytes`.
pub(crate) fn lower_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}
